//! Fixed-font before/after probe for cold/warm borrowed edge input.
// Other probes also use the shared LineResult snapshot helper.
#[allow(dead_code)]
#[path = "../../../crates/shodo/src/font/sfnt.rs"]
mod sfnt;
#[allow(dead_code)]
#[path = "support/edge_input_snapshot.rs"]
mod snapshot;
use serde_json::{Value, json};
use shodo::limits::Limits;
use shodo::node::{NodeId, OutOfFlowKind, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineOptions, ParagraphStyle, TextAlign, TextTransform, WordBreak,
};
use shodo::{
    AtomicSizes, LayoutContext, Line, LineConstraint, LineResult, Paragraph, ParagraphBuilder,
    Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle,
    RubyVisibility,
};
use std::time::Instant;
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);
fn selected(key: &str) -> bool {
    std::env::var("SHODO_INPUT_CASE").map_or(true, |wanted| wanted == key)
}
fn variable_font() -> Vec<u8> {
    // Same positive normalized-axis fixture as the core optical-size tests:
    // real fixed Latin outlines plus synthetic wght/opsz fvar axes.
    let bytes = shodo_fixtures::FONTS[0].bytes;
    let mut tables = Vec::new();
    for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
        let at = 12 + n * 16;
        let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
        tables.push((
            bytes[at..at + 4].try_into().unwrap(),
            bytes[offset..offset + len].to_vec(),
        ));
    }
    let mut fvar = Vec::new();
    for field in [1u16, 0, 16, 2, 2, 20, 0, 8] {
        fvar.extend(field.to_be_bytes());
    }
    for (tag, values) in [(b"wght", [100i32, 400, 900]), (b"opsz", [8, 12, 72])] {
        fvar.extend(tag);
        for value in values {
            fvar.extend((value << 16).to_be_bytes());
        }
        fvar.extend([0, 0, 1, 0]);
    }
    tables.push((*b"fvar", fvar));
    sfnt::build_sfnt(&tables)
}
fn trace_reset() {}
fn trace_read() -> Value {
    Value::Null
}
fn measured<T>(f: impl FnOnce() -> T) -> (T, Value) {
    trace_reset();
    #[cfg(feature = "allocation-counting")]
    let scope = ALLOC.begin().unwrap();
    let start = Instant::now();
    let out = f();
    std::hint::black_box(&out);
    let ns = start.elapsed().as_nanos() as u64;
    #[cfg(feature = "allocation-counting")]
    let allocation = Some(scope.finish());
    #[cfg(not(feature = "allocation-counting"))]
    let allocation: Option<Value> = None;
    (
        out,
        json!({"ns":ns,"allocation":allocation,"trace":trace_read()}),
    )
}
#[derive(Clone)]
struct Case {
    kind: &'static str,
    repeats: usize,
    width: f32,
    budget: &'static str,
}
impl Case {
    fn key(&self, mode: &str) -> String {
        format!(
            "{}/{}/{}/{}/{mode}",
            self.kind, self.repeats, self.width, self.budget
        )
    }
    fn limits(&self) -> Limits {
        let mut l = Limits::default();
        match self.budget {
            "disabled" => {
                l.max_balance_iterations = Some(0);
                l.max_pretty_window_lines = Some(0);
            }
            "one" => {
                l.max_balance_iterations = Some(1);
                l.max_pretty_window_lines = Some(1);
            }
            "edge-zero" => {
                l.max_reshape_window_bytes = Some(0);
                l.max_warnings = Some(1);
            }
            "edge-tiny" => {
                l.max_reshape_window_bytes = Some(1);
                l.max_warnings = Some(1);
            }
            "glyph-zero" => {
                l.max_shaped_glyphs = Some(0);
                l.max_warnings = Some(1);
            }
            "glyph-one" => {
                l.max_shaped_glyphs = Some(1);
                l.max_warnings = Some(1);
            }
            "warning-zero" => {
                l.max_reshape_window_bytes = Some(0);
                l.max_warnings = Some(0);
            }
            "shaper-zero" => {
                l.max_shaper_cache_entries = Some(0);
            }
            "run-eight" => {
                l.max_shaping_run_bytes = Some(8);
            }
            "default" => {}
            _ => unreachable!(),
        }
        l
    }
    fn builder(&self, l: &Limits) -> ParagraphBuilder {
        let ruby = self.kind.contains("ruby");
        let inline = InlineStyle {
            font_size: 16.,
            font_families: vec![FontFamily::Named(if self.kind == "variable" {
                "Edge Variable".into()
            } else {
                shodo_fixtures::FONTS[usize::from(ruby)].family.into()
            })],
            font_weight: if self.kind == "variable" { 900. } else { 400. },
            font_variations: if self.kind == "variable" {
                vec![shodo::style::FontVariation {
                    tag: *b"opsz",
                    value: 8.,
                }]
            } else {
                Vec::new()
            },
            word_break: if self.budget == "edge-zero"
                || matches!(self.kind, "edge" | "variable" | "missing" | "giant")
            {
                WordBreak::BreakAll
            } else {
                WordBreak::Normal
            },
            ..Default::default()
        };
        let style = ParagraphStyle {
            root: inline.clone(),
            first_line: self.kind.contains("first-line").then(|| InlineStyle {
                font_size: 32.,
                text_transform: TextTransform::Uppercase,
                paint: shodo::style::PaintStyle {
                    color: [180, 0, 0, 255],
                    ..Default::default()
                },
                ..inline.clone()
            }),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, l);
        b.with_offset_mapping(true);
        for i in 0..self.repeats {
            let id = 100 + i as u64 * 10;
            if ruby {
                let reading = InlineStyle {
                    font_size: 8.,
                    ..inline.clone()
                };
                let content = |node, text, s| {
                    RubyContent::text(
                        TextSource::Dom {
                            node: NodeId(node),
                            offset: 40,
                        },
                        text,
                        s,
                        l,
                    )
                };
                let r = Ruby::new(
                    vec![RubyBase {
                        node: NodeId(id + 1),
                        content: content(id + 1, "日本語", &inline),
                        align: RubyAlign::SpaceAround,
                    }],
                    vec![RubyLevel {
                        annotations: vec![RubyAnnotation {
                            node: NodeId(id + 2),
                            content: content(id + 2, "にほんご", &reading),
                            span: RubySpan::All,
                            visibility: RubyVisibility::Visible,
                        }],
                        style: RubyStyle::default(),
                    }],
                )
                .unwrap();
                b.push_ruby(NodeId(id), &inline, r);
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(id + 3),
                        offset: 7,
                    },
                    " 読み ",
                );
            } else if matches!(self.kind, "hyphen" | "missing" | "giant") {
                let text = match self.kind {
                    "hyphen" => "of\u{ad}fice ab\u{ad}cd ef\u{ad}gh ".to_owned(),
                    "missing" => "a \u{10ffff} b \u{10ffff} ".to_owned(),
                    "giant" => format!("a{} b ", "\u{301}".repeat(128)),
                    _ => unreachable!(),
                };
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(id),
                        offset: 7,
                    },
                    &text,
                );
            } else {
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(id),
                        offset: 7,
                    },
                    "Straße ffi abc def ",
                );
                match self.kind {
                    "forced" => {
                        b.push_forced_break(NodeId(id + 1));
                    }
                    "block" => {
                        b.push_block_in_inline(NodeId(id + 1));
                    }
                    "float" => {
                        b.push_out_of_flow(NodeId(id + 1), OutOfFlowKind::Float);
                    }
                    _ => {}
                }
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(id + 2),
                        offset: 11,
                    },
                    "ghi jkl ",
                );
            }
        }
        b
    }
}
// Lossless interning outside timing/allocation windows. Each line keeps its
// source table identity and all original public fields; nested Ruby recurses.
fn compact_output(mut lines: Value) -> Value {
    fn intern(value: Value, table: &mut Vec<Value>) -> usize {
        if let Some(i) = table.iter().position(|v| v == &value) {
            i
        } else {
            table.push(value);
            table.len() - 1
        }
    }
    fn visit(line: &mut Value, texts: &mut Vec<Value>, mappings: &mut Vec<Value>) {
        if let Some(obj) = line.as_object_mut() {
            if let Some(text) = obj.remove("text") {
                obj.insert("text_index".into(), json!(intern(text, texts)));
            }
            if let Some(mapping) = obj.remove("mapping") {
                obj.insert("mapping_index".into(), json!(intern(mapping, mappings)));
            }
            if let Some(ruby) = obj.get_mut("ruby") {
                for a in ruby.as_array_mut().unwrap() {
                    visit(&mut a["line"], texts, mappings);
                }
            }
        }
    }
    let mut texts = Vec::new();
    let mut mappings = Vec::new();
    for line in lines.as_array_mut().unwrap() {
        visit(line, &mut texts, &mut mappings);
    }
    json!({"texts":texts,"mappings":mappings,"lines":lines})
}

fn manual(
    p: &Paragraph,
    cx: &mut LayoutContext,
    options: &LineOptions,
    width: f32,
    max: Option<usize>,
    atomics: &AtomicSizes,
) -> Vec<Line> {
    let mut token = p.start_token();
    let mut cursor = None;
    let mut offset = 0_f32;
    let mut out = Vec::new();
    for _ in 0..p.text().len() * 4 + 1024 {
        let mut c = LineConstraint::new(width);
        c.block_offset = offset;
        c.max_graphemes = max;
        c.floats_placed_through = cursor;
        match p.next_line(cx, token, options, &c, atomics) {
            LineResult::Line(l) => {
                assert_ne!(l.break_token(), token);
                token = l.break_token();
                offset = ((offset * 64.).round() + (l.block_size() * 64.).round()) / 64.;
                out.push(l);
            }
            LineResult::FloatEncountered { float_cursor, .. } => {
                assert_ne!(cursor, Some(float_cursor));
                cursor = Some(float_cursor);
            }
            LineResult::BlockInInline { token_after, .. } => {
                assert_ne!(token_after, token);
                token = token_after;
            }
            LineResult::Done => return out,
            other => panic!("unexpected fixed-width result: {other:?}"),
        }
    }
    panic!("bounded source progress")
}
fn layout(
    ps: &[Paragraph],
    cx: &mut LayoutContext,
    options: &LineOptions,
    width: f32,
    max: Option<usize>,
    atomics: &AtomicSizes,
    mode: &str,
) -> Vec<Line> {
    let mut all = Vec::new();
    for p in ps {
        let lines = match mode {
            "internal" => match max {
                Some(n) => p.break_all_with_grapheme_limit(cx, options, width, n, atomics),
                None => p.break_all(cx, options, width, atomics),
            },
            "public" => {
                let mut cursor = None;
                p.lines(
                    cx,
                    p.start_token(),
                    options,
                    |previous, offset| {
                        if let Some(LineResult::FloatEncountered { float_cursor, .. }) = previous {
                            cursor = Some(*float_cursor);
                        }
                        let mut c = LineConstraint::new(width);
                        c.block_offset = offset;
                        c.max_graphemes = max;
                        c.floats_placed_through = cursor;
                        c
                    },
                    atomics,
                )
                .filter_map(|r| match r {
                    LineResult::Line(l) => Some(l),
                    _ => None,
                })
                .collect()
            }
            "manual" => manual(p, cx, options, width, max, atomics),
            _ => unreachable!(),
        };
        all.extend(lines);
    }
    all
}
fn run(q: &Query<'_>, mode: &str, inspect: bool, history: &str) -> Value {
    let (ps, options, width, max, atomics) = (q.ps, &q.options, q.width, q.max, q.atomics);
    let mut cx = LayoutContext::new();
    let (_, preparation) = measured(|| {
        if history == "warm" {
            drop(layout(ps, &mut cx, options, width, max, atomics, mode));
            cx.take_warnings();
        }
    });
    let (out, cost) = measured(|| layout(ps, &mut cx, options, width, max, atomics, mode));
    let warnings = format!("{:?}", cx.take_warnings());
    let count = out.len();
    let output = if inspect {
        compact_output(json!(out.iter().map(snapshot::line).collect::<Vec<_>>()))
    } else {
        Value::Null
    };
    let (_, release) = measured(|| drop(out));
    let (_, context_release) = measured(|| drop(cx));
    json!({"preparation":preparation,"cost":cost,"release":release,"context_release":context_release,"output":output,"warnings":warnings,"lines":count})
}
struct Query<'a> {
    key: String,
    ps: &'a [Paragraph],
    options: LineOptions,
    width: f32,
    max: Option<usize>,
    atomics: &'a AtomicSizes,
}
fn capture(q: Query<'_>, samples: usize, reverse: bool, rows: &mut Vec<Value>) {
    let histories = if reverse {
        ["warm", "cold"]
    } else {
        ["cold", "warm"]
    };
    if !selected(&q.key) {
        return;
    }
    for history in histories {
        let oracle = run(&q, "manual", true, history);
        let public = run(&q, "public", true, history);
        let actual = run(&q, "internal", true, history);
        assert_eq!(
            public["output"], oracle["output"],
            "public full output {} {history}",
            q.key
        );
        assert_eq!(
            public["warnings"], oracle["warnings"],
            "public warnings {} {history}",
            q.key
        );
        assert_eq!(
            actual["output"], oracle["output"],
            "internal full output {} {history}",
            q.key
        );
        assert_eq!(
            actual["warnings"], oracle["warnings"],
            "internal warnings {} {history}",
            q.key
        );
        drop(public);
        drop(oracle);
        for _ in 0..2 {
            std::hint::black_box(run(&q, "internal", false, history));
        }
        let values = (0..samples)
            .map(|_| run(&q, "internal", false, history))
            .collect::<Vec<_>>();
        rows.push(json!({"key":format!("{}/{history}",q.key),"mode":"internal","history":history,"public_manual_full_output_checked":true,"width_bits":q.width.to_bits(),"max_graphemes":q.max,"build_warnings":q.ps.iter().map(|p|format!("{:?}",p.warnings())).collect::<Vec<_>>(),"oracle":actual,"samples":values}));
    }
}
fn main() {
    let samples = std::env::var("SHODO_INPUT_SAMPLES")
        .ok()
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(3);
    assert!(samples > 0);
    let reverse = std::env::var_os("SHODO_INPUT_REVERSE").is_some();
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    fonts
        .collection
        .register_face(
            variable_font(),
            0,
            shodo::font::FontFaceDescriptor {
                family: "Edge Variable".into(),
                weight: (100., 900.),
                ..Default::default()
            },
        )
        .unwrap();
    let mut rows = Vec::new();
    let mut build_refusals = Vec::new();
    let mut workloads = shodo_bench::workloads();
    if reverse {
        workloads.reverse();
    }
    for w in workloads {
        let key = format!("standard/{}/{}", w.id, w.scale);
        if !selected(&key) {
            continue;
        }
        let ps = w
            .build(&mut LayoutContext::new(), &fonts, &Limits::default())
            .unwrap();
        let options = LineOptions {
            text_align: if w.id == "justify" {
                TextAlign::Justify
            } else {
                TextAlign::Start
            },
            ..Default::default()
        };
        capture(
            Query {
                key: format!("standard/{}/{}", w.id, w.scale),
                ps: &ps,
                options,
                width: w.width,
                max: None,
                atomics: w.atomics(),
            },
            samples,
            reverse,
            &mut rows,
        );
    }
    let mut cases = Vec::new();
    for kind in [
        "plain",
        "first-line",
        "ruby",
        "first-line-ruby",
        "forced",
        "block",
        "float",
        "edge",
    ] {
        for repeats in [1, 32] {
            for width in [80., 240.] {
                cases.push(Case {
                    kind,
                    repeats,
                    width,
                    budget: "default",
                });
            }
        }
    }
    cases.push(Case {
        kind: "edge",
        repeats: 1,
        width: 8.,
        budget: "edge-zero",
    });
    for budget in [
        "edge-tiny",
        "glyph-zero",
        "glyph-one",
        "warning-zero",
        "shaper-zero",
    ] {
        cases.push(Case {
            kind: "edge",
            repeats: 1,
            width: 8.,
            budget,
        });
    }
    for kind in ["hyphen", "missing", "giant", "variable"] {
        for repeats in [1, 8] {
            for width in [8., 80.] {
                cases.push(Case {
                    kind,
                    repeats,
                    width,
                    budget: "default",
                });
            }
        }
    }
    cases.push(Case {
        kind: "giant",
        repeats: 1,
        width: 8.,
        budget: "run-eight",
    });
    if reverse {
        cases.reverse();
    }
    for c in cases {
        let base = format!("rich/{}", c.key("width"));
        if !selected(&base)
            && !std::env::var("SHODO_INPUT_CASE")
                .is_ok_and(|key| key.starts_with(&format!("{base}/grapheme")))
        {
            continue;
        }
        let limits = c.limits();
        let ps = match c
            .builder(&limits)
            .build(&mut LayoutContext::new(), &fonts.collection)
        {
            Ok(paragraph) => [paragraph],
            Err(error) => {
                assert!(
                    matches!(c.budget, "glyph-zero" | "glyph-one"),
                    "unexpected build refusal {error:?}"
                );
                build_refusals.push(json!({"key":c.key("build"),"max_shaped_glyphs":limits.max_shaped_glyphs,"error":format!("{error:?}")}));
                continue;
            }
        };
        if c.kind == "variable" {
            let LineResult::Line(line) = ps[0].next_line(
                &mut LayoutContext::new(),
                ps[0].start_token(),
                &LineOptions::default(),
                &LineConstraint::new(4096.),
                &AtomicSizes::EMPTY,
            ) else {
                panic!("variable output")
            };
            let run = line
                .fragments()
                .find_map(|f| {
                    if let shodo::Fragment::GlyphRun(r) = f {
                        Some(r)
                    } else {
                        None
                    }
                })
                .unwrap();
            assert_eq!(
                run.normalized_coords()
                    .iter()
                    .map(|c| c.to_f32())
                    .collect::<Vec<_>>(),
                [1., -1.]
            );
        }
        capture(
            Query {
                key: format!("rich/{}", c.key("width")),
                ps: &ps,
                options: LineOptions::default(),
                width: c.width,
                max: None,
                atomics: &AtomicSizes::EMPTY,
            },
            samples,
            reverse,
            &mut rows,
        );
        if c.kind == "edge" && c.repeats == 1 && c.width == 80. {
            for max in [0, 1, 3] {
                capture(
                    Query {
                        key: format!("rich/{}/grapheme{max}", c.key("width")),
                        ps: &ps,
                        options: LineOptions::default(),
                        width: c.width,
                        max: Some(max),
                        atomics: &AtomicSizes::EMPTY,
                    },
                    samples,
                    reverse,
                    &mut rows,
                );
            }
        }
    }
    // Width sanitizers are correctness and cost controls; normal-sized fonts keep
    // the independent manual driver's block offsets inside exact Q26 bounds.
    let c = Case {
        kind: "plain",
        repeats: 1,
        width: 80.,
        budget: "default",
    };
    let ps = [c
        .builder(&Limits::default())
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()];
    for (width, key) in [
        (f32::NAN, "nan"),
        (-1., "negative"),
        (f32::MAX, "saturated"),
    ] {
        capture(
            Query {
                key: format!("control/{key}"),
                ps: &ps,
                options: LineOptions::default(),
                width,
                max: None,
                atomics: &AtomicSizes::EMPTY,
            },
            samples,
            reverse,
            &mut rows,
        );
    }
    assert!(
        !rows.is_empty(),
        "SHODO_INPUT_CASE must select an exact query key"
    );
    println!(
        "{}",
        json!({"schema":1,"samples":samples,"reverse":reverse,
        "selected_case":std::env::var("SHODO_INPUT_CASE").ok(),"rows":rows,"build_refusals":build_refusals})
    );
}
