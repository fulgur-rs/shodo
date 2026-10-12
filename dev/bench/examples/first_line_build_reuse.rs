//! First-line analysis/build/shape cost, with a separate complete output oracle.
//! Arguments: `samples` (default 7), `timing|oracle` (default timing).
//! Build with allocation-counting only for separate allocation diagnostics.
#[path = "support/completed_height_snapshot.rs"]
mod snapshot;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::geometry::{Direction, WritingMode};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, LineOptions, ParagraphStyle, TextCombineUpright, TextDecoration, TextTransform,
    TextWrapStyle,
};
use shodo::{
    AtomicIntrinsics, AtomicSizes, LayoutContext, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan,
    RubyStyle, RubyVisibility,
};
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn measured<T>(operation: impl FnOnce() -> T) -> (T, Value) {
    #[cfg(feature = "allocation-counting")]
    let scope = ALLOC.begin().unwrap();
    let start = Instant::now();
    let result = operation();
    let ns = start.elapsed().as_nanos() as u64;
    #[cfg(feature = "allocation-counting")]
    let allocation = Some(scope.finish());
    #[cfg(not(feature = "allocation-counting"))]
    let allocation: Option<Value> = None;
    (result, json!({"ns":ns,"allocation":allocation}))
}

struct Case {
    kind: &'static str,
    words: usize,
    text: String,
    style: ParagraphStyle,
}
impl Case {
    fn new(kind: &'static str, words: usize) -> Self {
        let cjk = matches!(kind, "cjk" | "vertical" | "combined" | "ruby");
        let font = if cjk {
            1
        } else if kind == "rtl" {
            2
        } else {
            0
        };
        let mut style = ParagraphStyle::default();
        style.root.font_families =
            vec![FontFamily::Named(shodo_fixtures::FONTS[font].family.into())];
        if matches!(kind, "vertical" | "combined") {
            style.writing_mode = WritingMode::VerticalRl;
        }
        if kind == "rtl" {
            style.direction = Direction::Rtl;
            style.root.direction = Direction::Rtl;
        }
        let mut first = style.root.clone();
        match kind {
            "none" => {}
            "same" => {
                style.first_line = Some(first);
            }
            "size" => {
                first.font_size = 22.0;
                style.first_line = Some(first);
            }
            "transform" => {
                first.text_transform = TextTransform::Uppercase;
                style.first_line = Some(first);
            }
            _ => {
                first.paint.color = [0, 0, 255, 255];
                if kind == "decoration" {
                    first.paint.underline = Some(TextDecoration {
                        color: Some([255, 0, 0, 255]),
                        offset: Some(2.0),
                        thickness: Some(1.0),
                    });
                    first.paint.strikethrough = Some(TextDecoration::default());
                }
                style.first_line = Some(first);
            }
        }
        let text = (0..words)
            .map(|i| {
                if cjk {
                    "日本語"
                } else if kind == "rtl" {
                    ["مرحبا", "بالعالم"][i % 2]
                } else {
                    [
                        "office", "affinity", "first", "fluffy", "waffle", "flow", "official",
                    ][i % 7]
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        Self {
            kind,
            words,
            text,
            style,
        }
    }

    fn builder(&self, limits: &Limits) -> ParagraphBuilder {
        let mut builder = ParagraphBuilder::new(&self.style, limits);
        let root = &self.style.root;
        let dom = |node, offset| TextSource::Dom {
            node: NodeId(node),
            offset,
        };
        if self.kind == "ruby" {
            let annotation = shodo::style::InlineStyle {
                font_size: 8.0,
                ..root.clone()
            };
            let base = RubyContent::text(dom(12, 4), "日本語", root, limits);
            let reading = RubyContent::text(dom(14, 7), "にほんご", &annotation, limits);
            let ruby = Ruby::new(
                vec![RubyBase {
                    node: NodeId(11),
                    content: base,
                    align: RubyAlign::SpaceAround,
                }],
                vec![RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(13),
                        content: reading,
                        span: RubySpan::All,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle::default(),
                }],
            )
            .unwrap();
            builder.push_ruby(NodeId(10), root, ruby);
        }
        if self.kind == "combined" {
            let combined = shodo::style::InlineStyle {
                text_combine_upright: TextCombineUpright::All,
                ..root.clone()
            };
            builder
                .open_inline(NodeId(20), &combined, Default::default())
                .push_text(dom(21, 2), "12")
                .close_inline();
        }
        if self.kind == "structural" {
            let child = shodo::style::InlineStyle {
                font_weight: 600.0,
                ..root.clone()
            };
            let mut first = child.clone();
            first.paint.color = [255, 0, 0, 255];
            builder
                .push_text(dom(30, 0), "of")
                .open_inline_with_first_line(NodeId(31), &child, &first, Default::default())
                .push_text(dom(32, 2), "fice ")
                .close_inline();
            builder.push_forced_break(NodeId(33));
        }
        builder.push_text(dom(1, 7), &self.text);
        builder
    }
}

fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}
fn public_summary(paragraph: &Paragraph) -> String {
    digest(
        &json!({"text":paragraph.text(),"mapping":format!("{:?}",paragraph.offset_mapping()),"warnings":format!("{:?}",paragraph.warnings())}),
    )
}

// Lossless interning keeps every byte of text and mapping once per distinct
// value, rather than serializing an entire paragraph again for every line.
// Ruby descendants recurse through the same table.
fn intern_shared(value: &mut Value, table: &mut Vec<Value>) {
    match value {
        Value::Object(object) => {
            if object.contains_key("text") && object.contains_key("range") {
                for field in ["text", "mapping"] {
                    if let Some(shared) = object.remove(field) {
                        let index = table
                            .iter()
                            .position(|entry| *entry == shared)
                            .unwrap_or_else(|| {
                                table.push(shared);
                                table.len() - 1
                            });
                        object.insert(format!("{field}_index"), json!(index));
                    }
                }
            }
            for child in object.values_mut() {
                intern_shared(child, table);
            }
        }
        Value::Array(values) => {
            for child in values {
                intern_shared(child, table);
            }
        }
        _ => {}
    }
}

fn oracle(paragraph: &Paragraph) -> Value {
    let mut hash = Sha256::new();
    let mut entries = 0;
    let mut shared = Vec::new();
    let mut feed = |mut value: Value| {
        intern_shared(&mut value, &mut shared);
        hash.update(value.to_string().as_bytes());
        hash.update(b"\n");
        entries += 1;
    };
    feed(json!({"summary":public_summary(paragraph)}));
    let mut cx = LayoutContext::new();
    let sizes =
        paragraph.intrinsic_sizes(&mut cx, &LineOptions::default(), &AtomicIntrinsics::EMPTY);
    feed(
        json!({"intrinsic_bits":[sizes.min_content.to_bits(),sizes.max_content.to_bits()],"warnings":format!("{:?}",cx.take_warnings())}),
    );
    for width in [0.0_f32, 32.0, 240.0, f32::MAX] {
        for mode in [
            TextWrapStyle::Auto,
            TextWrapStyle::Balance,
            TextWrapStyle::Pretty,
        ] {
            let options = LineOptions {
                text_wrap_style: mode,
                ..Default::default()
            };
            let mut cx = LayoutContext::new();
            let plan = paragraph.plan_breaks(&mut cx, &options, width, &AtomicSizes::EMPTY);
            feed(
                json!({"width_bits":width.to_bits(),"mode":format!("{mode:?}"),"plan":snapshot::plan(&plan),"warnings":format!("{:?}",cx.take_warnings())}),
            );
            let mut token = paragraph.start_token();
            loop {
                let mut constraint = LineConstraint::new(width);
                constraint.break_plan = Some(&plan);
                let result =
                    paragraph.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY);
                match result {
                    LineResult::Line(line) => {
                        assert_ne!(line.break_token(), token);
                        let mut output = snapshot::line(&line);
                        output["paint_spans"] = json!(format!("{:?}", line.paint_spans()));
                        output["forced_break"] = json!(format!("{:?}", line.forced_break()));
                        output["variation_bits"] = json!(
                            line.fragments()
                                .filter_map(|fragment| match fragment {
                                    shodo::Fragment::GlyphRun(run) => Some(
                                        run.variations()
                                            .iter()
                                            .map(|v| (v.tag, v.value.to_bits()))
                                            .collect::<Vec<_>>()
                                    ),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                        );
                        feed(output);
                        feed(json!({"layout_warnings":format!("{:?}",cx.take_warnings())}));
                        let mut rejected = LineConstraint::new(width);
                        rejected.break_plan = Some(&plan);
                        rejected.max_block_size = Some((line.block_size() - 1.0 / 64.0).max(0.0));
                        let mut retry = LayoutContext::new();
                        let event = paragraph.next_line(
                            &mut retry,
                            token,
                            &options,
                            &rejected,
                            &AtomicSizes::EMPTY,
                        );
                        assert!(matches!(event, LineResult::BlockSizeExceeded { .. }));
                        feed(
                            json!({"height_reject":snapshot::event(&event),"warnings":format!("{:?}",retry.take_warnings())}),
                        );
                        token = line.break_token();
                    }
                    LineResult::Done => {
                        feed(snapshot::results(&[LineResult::Done]));
                        break;
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
        }
    }
    hash.update(json!(shared).to_string().as_bytes());
    json!({"sha256":format!("{:x}",hash.finalize()),"entries":entries})
}
fn main() {
    let mut args = std::env::args().skip(1);
    let samples = args.next().map_or(7, |s| s.parse::<usize>().unwrap());
    let inspect = args.next().is_some_and(|mode| mode == "oracle");
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let mut cases = Vec::new();
    for words in [8, 4000] {
        for kind in [
            "none",
            "same",
            "paint",
            "decoration",
            "size",
            "transform",
            "cjk",
            "rtl",
            "vertical",
            "combined",
            "structural",
            "ruby",
        ] {
            let case = Case::new(kind, words);
            // Warm font/shaper collection caches before any measured operation.
            case.builder(&limits)
                .build(&mut LayoutContext::new(), &fonts.collection)
                .unwrap();
            let mut rows = Vec::new();
            let mut output = Value::Null;
            for sample in 0..samples {
                let builder = case.builder(&limits);
                let (analysis, analysis_cost) = measured(|| builder.analyze().unwrap());
                let mut cx = LayoutContext::new();
                let (shaped, shape_cost) =
                    measured(|| analysis.shape(&mut cx, &fonts.collection).unwrap());
                let builder = case.builder(&limits);
                let mut cx = LayoutContext::new();
                let (paragraph, build_cost) =
                    measured(|| builder.build(&mut cx, &fonts.collection).unwrap());
                assert_eq!(public_summary(&shaped), public_summary(&paragraph));
                if inspect && sample == 0 {
                    eprintln!("oracle {}/{}", case.kind, case.words);
                    output = oracle(&paragraph);
                    assert_eq!(output, oracle(&shaped), "analyze+shape vs build output");
                }
                rows.push(json!({"analyze":analysis_cost,"shape":shape_cost,"build":build_cost,"summary":public_summary(&paragraph)}));
            }
            cases.push(json!({"kind":case.kind,"words":case.words,"text_sha256":format!("{:x}",Sha256::digest(case.text.as_bytes())),"bytes":case.text.len(),"samples":rows,"oracle":output}));
        }
    }
    println!(
        "{}",
        json!({"allocation_counting":cfg!(feature="allocation-counting"),"oracle":inspect,"cases":cases})
    );
}
