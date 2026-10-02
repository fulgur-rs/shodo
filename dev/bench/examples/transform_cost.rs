//! Fixed-font transform build cost. `time` is counter-free; `alloc` needs
//! allocation-counting. Builders and output serialization are outside scopes.
use serde_json::{Value, json};
use shodo::font::FontCollection;
use shodo::geometry::WritingMode;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineOptions, ParagraphStyle, TextCombineUpright, TextTransform,
    WordSpaceTransform,
};
use shodo::{AtomicSizes, Fragment, LayoutContext, Paragraph, ParagraphBuilder};
use std::{hint::black_box, time::Instant};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

struct Case {
    name: &'static str,
    text: String,
    transform: TextTransform,
    word: WordSpaceTransform,
    lang: Option<&'static str>,
    partial: bool,
    first: bool,
    combine: bool,
}
fn cases() -> Vec<Case> {
    use TextTransform::*;
    [
        (
            "none",
            "a b c 日本語 ",
            None,
            WordSpaceTransform::None,
            Option::None,
        ),
        (
            "partial",
            "a b c 日本語 ",
            Uppercase,
            WordSpaceTransform::None,
            Some("en"),
        ),
        (
            "width",
            "aA 1 ｶﾞ ぁ ",
            FullWidth,
            WordSpaceTransform::None,
            Some("ja"),
        ),
        (
            "kana",
            "ぁぃゃゅょっ ｧｨｬｭｮｯ ",
            FullSizeKana,
            WordSpaceTransform::None,
            Some("ja"),
        ),
        (
            "width-kana",
            "aA 1 ｶﾞ ぁ ",
            FullWidthFullSizeKana,
            WordSpaceTransform::None,
            Some("ja"),
        ),
        (
            "word-space",
            "a\u{200b}b 日本語 ",
            None,
            WordSpaceTransform::Space,
            Option::None,
        ),
        (
            "word-warning",
            "a\u{200b}b ",
            None,
            WordSpaceTransform::SpaceAutoPhrase,
            Some("!invalid"),
        ),
        (
            "greek",
            "άι ά ή ΟΣ ΟΣΑ ",
            Uppercase,
            WordSpaceTransform::None,
            Some("el"),
        ),
        (
            "sigma",
            "ΟΣ ΟΣΑ ",
            Lowercase,
            WordSpaceTransform::None,
            Some("el"),
        ),
        (
            "turkic",
            "I\u{0307} ıi ",
            Lowercase,
            WordSpaceTransform::None,
            Some("tr"),
        ),
        (
            "lithuanian",
            "I\u{0301} J\u{0300} ",
            Lowercase,
            WordSpaceTransform::None,
            Some("lt"),
        ),
        (
            "dutch",
            "ijSSEL ijssel ",
            Capitalize,
            WordSpaceTransform::None,
            Some("nl"),
        ),
        (
            "first-line",
            "aA 1 ｶﾞ ぁ ",
            FullWidthFullSizeKana,
            WordSpaceTransform::None,
            Some("ja"),
        ),
        (
            "width-origin",
            "a1ｶﾞ ",
            FullWidth,
            WordSpaceTransform::None,
            Some("ja"),
        ),
        (
            "case-width",
            "straße ij ぁ ｶﾞ ",
            UppercaseFullWidthFullSizeKana,
            WordSpaceTransform::None,
            Some("de"),
        ),
    ]
    .into_iter()
    .map(|(name, text, transform, word, lang)| Case {
        name,
        text: text.repeat(16),
        transform,
        word,
        lang,
        partial: name == "partial",
        first: name == "first-line",
        combine: name == "width-origin",
    })
    .collect()
}
fn builder(c: &Case, limits: &Limits) -> ParagraphBuilder {
    let normal = InlineStyle {
        font_families: shodo_fixtures::FONTS
            .iter()
            .map(|f| FontFamily::Named(f.family.into()))
            .collect(),
        lang: c.lang.map(str::to_owned),
        text_transform: if c.partial || c.first {
            TextTransform::None
        } else {
            c.transform
        },
        word_space_transform: c.word,
        text_combine_upright: if c.combine {
            TextCombineUpright::All
        } else {
            TextCombineUpright::None
        },
        ..Default::default()
    };
    let ps = ParagraphStyle {
        root: normal.clone(),
        first_line: c.first.then(|| InlineStyle {
            text_transform: c.transform,
            ..normal.clone()
        }),
        writing_mode: if c.combine {
            WritingMode::VerticalRl
        } else {
            WritingMode::HorizontalTb
        },
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&ps, limits);
    let mut at = 0;
    // Split scalars across transparent nodes to exercise voicing/case context.
    for (i, part) in c.text.split_inclusive(' ').enumerate() {
        let mut style = normal.clone();
        if c.partial && i == 0 {
            style.text_transform = c.transform;
        }
        b.open_inline(NodeId(i as u64 + 1), &style, InlineEdges::default());
        b.push_text(
            TextSource::Dom {
                node: NodeId(i as u64 + 1),
                offset: at,
            },
            part,
        );
        b.close_inline();
        at += part.len() as u32;
    }
    b
}
fn snapshot(p: &Paragraph, cx: &mut LayoutContext) -> Value {
    let mut widths = Vec::new();
    for width in [48., 160., 1024.] {
        let lines = p.break_all(cx, &LineOptions::default(), width, &AtomicSizes::EMPTY);
        widths.push(lines.iter().map(|l| {
            let runs=l.fragments().map(|f|match f {
                Fragment::GlyphRun(r)=>json!({"kind":"glyph","font":[r.font().layer(),r.font().index()],"style":r.style_index(),"script":r.script(),"language":r.language(),"range":r.text_range(),"bidi":r.bidi_level(),"metrics":format!("{:?}",r.metrics()),"vertical":format!("{:?}",r.vertical_metrics()),"variations":format!("{:?}",r.variations()),"coords":format!("{:?}",r.normalized_coords()),"paint":format!("{:?}",r.paint_style()),"transform":format!("{:?}",r.glyph_transform()),"orientation":format!("{:?}",r.orientation()),"source":format!("{:?}",r.source()),"embolden":r.embolden(),"skew":r.skew().map(f32::to_bits),"placement":[r.inline_start().to_bits(),r.inline_size().to_bits(),r.baseline().to_bits(),r.font_size().to_bits()],"clusters":format!("{:?}",r.clusters().collect::<Vec<_>>()),"glyphs":r.glyphs().enumerate().map(|(i,g)|json!([g.id,g.cluster,g.inline_position.to_bits(),g.block_offset.to_bits(),g.advance.to_bits(),r.glyph_origin(i).map(|(x,y)|[x.to_bits(),y.to_bits()])])).collect::<Vec<_>>()}),
                _=>json!({"kind":"other","debug":format!("{f:?}")}),
            }).collect::<Vec<_>>();
            json!({"text":l.text(),"range":l.text_range(),"geometry":[l.inline_size().to_bits(),l.block_size().to_bits(),l.block_offset().to_bits(),l.hang_start().to_bits(),l.hang_end().to_bits()],"reason":format!("{:?}",l.break_reason()),"metrics":format!("{:?}",l.metrics()),"overflow":format!("{:?}",l.overflow_rect()),"owners":format!("{:?}",l.owners().collect::<Vec<_>>()),"combinations":format!("{:?}",l.text_combinations().collect::<Vec<_>>()),"runs":runs})
        }).collect::<Vec<_>>());
    }
    json!({"text":p.text(),"mapping":format!("{:?}",p.offset_mapping()),"warnings":format!("{:?}",p.warnings()),"line_warnings":format!("{:?}",cx.take_warnings()),"lines":widths})
}
fn main() {
    let mode = std::env::args().nth(1).expect("time or alloc");
    let fixed = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    let fonts: &FontCollection = &fixed.collection;
    let mut rows = Vec::new();
    for c in cases() {
        let limits = Limits::default();
        let mut cx = LayoutContext::new();
        for _ in 0..4 {
            black_box(builder(&c, &limits).build(&mut cx, fonts).unwrap());
        }
        let mut samples = Vec::new();
        for _ in 0..if mode == "time" { 9 } else { 3 } {
            let b = builder(&c, &limits);
            match mode.as_str() {
                "time" => {
                    let start = Instant::now();
                    let p = b.build(&mut cx, fonts).unwrap();
                    let ns = start.elapsed().as_nanos();
                    black_box(&p);
                    samples.push(json!(ns));
                }
                #[cfg(feature = "allocation-counting")]
                "alloc" => {
                    let scope = ALLOC.begin().unwrap();
                    let p = b.build(&mut cx, fonts).unwrap();
                    let count = scope.finish();
                    black_box(&p);
                    samples.push(json!(count));
                }
                _ => panic!("alloc requires allocation-counting"),
            }
        }
        let p = builder(&c, &limits).build(&mut cx, fonts).unwrap();
        let large_output = json!({"text":p.text(),"mapping":format!("{:?}",p.offset_mapping()),"warnings":format!("{:?}",p.warnings())});
        let small = Case {
            text: c.text.chars().take(c.text.chars().count() / 8).collect(),
            name: c.name,
            transform: c.transform,
            word: c.word,
            lang: c.lang,
            partial: c.partial,
            first: c.first,
            combine: c.combine,
        };
        let small_p = builder(&small, &limits).build(&mut cx, fonts).unwrap();
        let output = snapshot(&small_p, &mut cx);
        let refusals = [0, 1, 8, c.text.len() as u64].map(|cap| {
            let limited = Limits {
                max_text_bytes: Some(cap),
                ..limits.clone()
            };
            match builder(&c, &limited).build(&mut LayoutContext::new(), fonts) {
                Ok(p) => format!("ok:{:?}:{:?}", p.text(), p.warnings()),
                Err(e) => format!("error:{e:?}"),
            }
        });
        rows.push(json!({"case":c.name,"bytes":c.text.len(),"samples":samples,"large_output":large_output,"output":output,"limits":refusals}));
    }
    println!(
        "{}",
        json!({"mode":mode,"warmup":4,"scope":"build consumes prebuilt builder; paragraph/context retained at scope end; serialization/line layout excluded","fonts":shodo_fixtures::FONTS.iter().map(|f|(f.id,f.sha256)).collect::<Vec<_>>(),"rows":rows})
    );
}
