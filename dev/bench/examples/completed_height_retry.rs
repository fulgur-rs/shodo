//! Compare completed height retries, including exact events and resource controls.
#[path = "support/completed_height_snapshot.rs"]
mod snapshot;
use serde_json::{Value, json};
use shodo::limits::Limits;
use shodo::node::{NodeId, OutOfFlowKind, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineOptions, ParagraphStyle, TextTransform, WordBreak,
};
use shodo::{
    AtomicSizes, LayoutContext, LineConstraint, LineResult, Paragraph, ParagraphBuilder, Ruby,
    RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle,
    RubyVisibility,
};
use shodo_bench::{self as workload, Operation, Workload};
use std::time::Instant;
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);
// Archived disposable observer only; counter-free shipping example has no hooks.
fn trace_reset(_role: usize) {}
fn trace_read() -> Value {
    Value::Null
}
fn trace_role<T>(_role: usize, f: impl FnOnce() -> T) -> T {
    f()
}
fn measured<T>(role: usize, f: impl FnOnce() -> T) -> (T, Value) {
    trace_reset(role);
    #[cfg(feature = "allocation-counting")]
    let scope = ALLOC.begin().unwrap();
    let start = Instant::now();
    let value = f();
    std::hint::black_box(&value);
    let ns = start.elapsed().as_nanos() as u64;
    #[cfg(feature = "allocation-counting")]
    let allocation = Some(scope.finish());
    #[cfg(not(feature = "allocation-counting"))]
    let allocation: Option<Value> = None;
    (
        value,
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
            "run-one" => l.max_shaping_run_bytes = Some(1),
            "shape-cache-zero" => l.max_shaper_cache_entries = Some(0),
            "edge-zero-warning-zero" => {
                l.max_reshape_window_bytes = Some(0);
                l.max_warnings = Some(0);
            }
            "grapheme-three" | "warning-zero" => {
                if self.budget == "warning-zero" {
                    l.max_warnings = Some(0);
                }
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
            font_families: vec![FontFamily::Named(
                shodo_fixtures::FONTS[usize::from(ruby)].family.into(),
            )],
            word_break: if self.budget.starts_with("edge-zero") {
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
fn warnings(cx: &mut LayoutContext) -> String {
    format!("{:?}", cx.take_warnings())
}
fn standard(
    w: &Workload,
    op: Operation,
    fonts: &shodo_fixtures::FixtureFonts,
    inspect: bool,
) -> Value {
    let limits = Limits::default();
    let mut build = LayoutContext::new();
    let paragraphs = w.build(&mut build, fonts, &limits).unwrap();
    let mut cx = LayoutContext::new();
    let (run, cost) = measured(4, || {
        workload::layout(w, &paragraphs, &mut cx, fonts, &limits, op).unwrap()
    });
    let warn = warnings(&mut cx);
    let count = run.lines.len();
    if op == Operation::PageRetry {
        assert_eq!(
            run.height_retries, count,
            "original PageRetry rejects every actual line"
        );
    }
    let output = if inspect {
        json!(run.lines.iter().map(snapshot::line).collect::<Vec<_>>())
    } else {
        Value::Null
    };
    if inspect {
        let mut fresh = LayoutContext::new();
        let direct = workload::layout(
            w,
            &paragraphs,
            &mut fresh,
            fonts,
            &limits,
            Operation::AllLines,
        )
        .unwrap();
        assert_eq!(direct.lines.len(), count);
        assert_eq!(
            json!(direct.lines.iter().map(snapshot::line).collect::<Vec<_>>()),
            output,
            "actual original PageRetry vs direct full output"
        );
        assert_eq!(
            warnings(&mut fresh),
            warn,
            "original PageRetry warning equality"
        );
        assert_eq!(run.float_reports, direct.float_reports);
    }
    let retries = run.height_retries;
    let float_reports = run.float_reports;
    let (_, release) = measured(8, || drop(run));
    let (_, context_release) = measured(8, || drop(cx));
    json!({"cost":cost,"release":release,"context_release":context_release,"output":if inspect{compact_output(output)}else{Value::Null},"warnings":warn,"lines":count,"height_retries":retries,"float_reports":float_reports})
}
struct Accepted {
    results: Vec<LineResult>,
    rejects: usize,
    floats: usize,
    needed: Vec<u32>,
    warnings: Vec<String>,
    events: Vec<Value>,
}
fn custom_layout(
    p: &Paragraph,
    c: &Case,
    cx: &mut LayoutContext,
    reject_count: usize,
    inspect: bool,
) -> Accepted {
    let options = LineOptions::default();
    let mut token = p.start_token();
    let mut cursor = None;
    let mut offset = 0.;
    let mut results = Vec::new();
    let mut rejects = 0;
    let mut floats = 0;
    let mut needed = Vec::new();
    let mut warn = Vec::new();
    let mut events = Vec::new();
    for _ in 0..c.repeats * 256 + 256 {
        let mut con = LineConstraint::new(c.width);
        con.block_offset = offset;
        con.floats_placed_through = cursor;
        if c.budget == "grapheme-three" {
            con.max_graphemes = Some(3);
        }
        let mut early = None;
        let mut expected = None;
        for _ in 0..reject_count {
            con.max_block_size = Some(0.);
            let r = trace_role(5, || {
                p.next_line(cx, token, &options, &con, &AtomicSizes::EMPTY)
            });
            // Warnings are drained per public call to preserve replay/cap observations.
            let warnings = cx.take_warnings();
            if inspect {
                events.push(snapshot::event(&r));
                warn.push(format!("{warnings:?}"));
            }
            match r {
                LineResult::BlockSizeExceeded { needed_block_size } => {
                    assert!(needed_block_size > 0.);
                    let bits = needed_block_size.to_bits();
                    if let Some(prev) = expected {
                        assert_eq!(prev, bits);
                    }
                    expected = Some(bits);
                    rejects += 1;
                    if inspect {
                        needed.push(bits);
                        let mut fresh = LayoutContext::new();
                        let other =
                            p.next_line(&mut fresh, token, &options, &con, &AtomicSizes::EMPTY);
                        let LineResult::BlockSizeExceeded {
                            needed_block_size: other,
                        } = other
                        else {
                            panic!("fresh height rejection")
                        };
                        assert_eq!(other.to_bits(), bits);
                        assert_eq!(
                            fresh.take_warnings(),
                            warnings,
                            "fresh reject warning contract"
                        );
                    }
                }
                other => {
                    early = Some((other, warnings));
                    break;
                }
            }
        }
        con.max_block_size = None;
        let (r, call_warnings) = if let Some(pair) = early {
            pair
        } else {
            let r = trace_role(if reject_count == 0 { 4 } else { 6 }, || {
                p.next_line(cx, token, &options, &con, &AtomicSizes::EMPTY)
            });
            let warnings = cx.take_warnings();
            if inspect {
                events.push(snapshot::event(&r));
                warn.push(format!("{warnings:?}"));
            }
            (r, warnings)
        };
        match r {
            LineResult::Line(l) => {
                assert_ne!(l.break_token(), token, "accepted token progresses");
                if let Some(bits) = expected {
                    assert_eq!(bits, l.block_size().to_bits());
                }
                if inspect {
                    let mut fresh = LayoutContext::new();
                    let other = p.next_line(&mut fresh, token, &options, &con, &AtomicSizes::EMPTY);
                    let LineResult::Line(other) = other else {
                        panic!("fresh accept")
                    };
                    assert_eq!(snapshot::line(&other), snapshot::line(&l));
                    assert_eq!(
                        fresh.take_warnings(),
                        call_warnings,
                        "fresh accepted warnings"
                    );
                }
                token = l.break_token();
                offset = ((offset * 64.).round() + (l.block_size() * 64.).round()) / 64.;
                results.push(LineResult::Line(l));
            }
            LineResult::BlockInInline { node, token_after } => {
                assert_ne!(token_after, token);
                token = token_after;
                results.push(LineResult::BlockInInline { node, token_after });
            }
            LineResult::FloatEncountered { float_cursor, .. } => {
                assert_ne!(cursor, Some(float_cursor));
                cursor = Some(float_cursor);
                floats += 1;
            }
            LineResult::Done => {
                results.push(LineResult::Done);
                return Accepted {
                    results,
                    rejects,
                    floats,
                    needed,
                    warnings: warn,
                    events,
                };
            }
            other => panic!("unexpected retry result: {other:?}"),
        }
    }
    panic!("bounded progress exhausted")
}
fn custom(c: &Case, retry: usize, fonts: &shodo_fixtures::FixtureFonts, inspect: bool) -> Value {
    let limits = c.limits();
    // Font-layer resource limits belong to the collection, not just paragraph data.
    let limited_fonts =
        (c.budget == "shape-cache-zero").then(|| shodo_fixtures::load_fonts(&limits).unwrap());
    let fonts = limited_fonts.as_ref().unwrap_or(fonts);
    let mut build = LayoutContext::new();
    let p = c
        .builder(&limits)
        .build(&mut build, &fonts.collection)
        .unwrap();
    let mut cx = LayoutContext::new();
    // Fresh semantic controls run before the separate timing invocation; never
    // snapshot/allocate oracle state inside a measured retry operation.
    let oracle = if inspect {
        let mut proof = LayoutContext::new();
        let actual = custom_layout(&p, c, &mut proof, retry, true);
        let mut fresh = LayoutContext::new();
        let direct = custom_layout(&p, c, &mut fresh, 0, true);
        assert_eq!(
            snapshot::results(&actual.results),
            snapshot::results(&direct.results)
        );
        assert_eq!(actual.floats, direct.floats);
        let count = actual
            .results
            .iter()
            .filter(|r| matches!(r, LineResult::Line(_)))
            .count();
        assert_eq!(actual.rejects, count * retry);
        let plan_control = if !matches!(c.kind, "block" | "float") && c.budget != "grapheme-three" {
            let options = LineOptions::default();
            let mut plan_cx = LayoutContext::new();
            let plan = p.plan_breaks(&mut plan_cx, &options, c.width, &AtomicSizes::EMPTY);
            let mut con = LineConstraint::new(c.width);
            con.break_plan = Some(&plan);
            con.max_block_size = Some(0.);
            let mut height_cx = LayoutContext::new();
            let reject = p.next_line(
                &mut height_cx,
                p.start_token(),
                &options,
                &con,
                &AtomicSizes::EMPTY,
            );
            let LineResult::BlockSizeExceeded { needed_block_size } = reject else {
                panic!("plan height0");
            };
            con.max_block_size = None;
            let accept = p.next_line(
                &mut height_cx,
                p.start_token(),
                &options,
                &con,
                &AtomicSizes::EMPTY,
            );
            let LineResult::Line(accept) = accept else {
                panic!("planned same-token accept");
            };
            let LineResult::Line(first) = &direct.results[0] else {
                panic!("direct first line");
            };
            assert_eq!(snapshot::line(&accept), snapshot::line(first));
            assert_eq!(needed_block_size.to_bits(), accept.block_size().to_bits());
            json!({"plan":snapshot::plan(&plan),"needed_bits":needed_block_size.to_bits(),"warnings":warnings(&mut height_cx)})
        } else {
            Value::Null
        };
        json!({"output":compact_output(snapshot::results(&actual.results)),"rejects":actual.rejects,"floats":actual.floats,"needed_bits":actual.needed,"warnings_per_call":actual.warnings,"events":actual.events,"plan_height_control":plan_control})
    } else {
        Value::Null
    };
    let (out, cost) = measured(4, || custom_layout(&p, c, &mut cx, retry, false));
    let count = out
        .results
        .iter()
        .filter(|r| matches!(r, LineResult::Line(_)))
        .count();
    assert_eq!(out.rejects, count * retry);
    let rejects = out.rejects;
    let floats = out.floats;
    let (_, release) = measured(8, || drop(out));
    let (_, context_release) = measured(8, || drop(cx));
    json!({"cost":cost,"release":release,"context_release":context_release,"output":oracle,"lines":count,"height_retries":rejects,"float_reports":floats})
}
fn main() {
    let samples = std::env::var("SHODO_COMPLETED_SAMPLES")
        .ok()
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(5);
    assert!(samples > 0);
    let reverse = std::env::var_os("SHODO_COMPLETED_REVERSE").is_some();
    let selected = std::env::var("SHODO_COMPLETED_CASE").ok();
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    let mut rows = Vec::new();
    let mut standard_cases = workload::workloads();
    let mut operations = [Operation::AllLines, Operation::PageRetry];
    if reverse {
        standard_cases.reverse();
        operations.reverse();
    }
    for w in standard_cases {
        for op in operations {
            let key = format!("standard/{}/{}/{op:?}", w.id, w.scale);
            if selected.as_ref().is_some_and(|s| s != &key) {
                continue;
            }
            let oracle = standard(&w, op, &fonts, true);
            for _ in 0..2 {
                std::hint::black_box(standard(&w, op, &fonts, false));
            }
            let values = (0..samples)
                .map(|_| standard(&w, op, &fonts, false))
                .collect::<Vec<_>>();
            rows.push(json!({"key":format!("standard/{}/{}/{op:?}",w.id,w.scale),"kind":"standard","settings":w.settings(),"operation":format!("{op:?}"),"oracle":oracle,"samples":values}));
        }
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
        kind: "plain",
        repeats: 1,
        width: 80.,
        budget: "edge-zero",
    });
    for budget in [
        "run-one",
        "shape-cache-zero",
        "edge-zero-warning-zero",
        "grapheme-three",
        "warning-zero",
    ] {
        for width in [80., 240.] {
            cases.push(Case {
                kind: "plain",
                repeats: 1,
                width,
                budget,
            });
        }
    }
    let mut retries = [0, 1, 4];
    if reverse {
        cases.reverse();
        retries.reverse();
    }
    for c in cases {
        for retry in retries {
            let key = format!("rich/{}/{retry}", c.key("height"));
            if selected.as_ref().is_some_and(|s| s != &key) {
                continue;
            }
            let oracle = custom(&c, retry, &fonts, true);
            for _ in 0..2 {
                std::hint::black_box(custom(&c, retry, &fonts, false));
            }
            let values = (0..samples)
                .map(|_| custom(&c, retry, &fonts, false))
                .collect::<Vec<_>>();
            rows.push(json!({"key":format!("rich/{}/{retry}",c.key("height")),"kind":c.kind,"repeats":c.repeats,"width":c.width,"budget":c.budget,"retry_count":retry,"font_layer_max_shaper_cache_entries":if c.budget == "shape-cache-zero" {0}else{64},"limits":format!("{:#?}",c.limits()),"oracle":oracle,"samples":values}));
        }
    }
    println!(
        "{}",
        json!({"schema":1,"samples":samples,"reverse":reverse,"rows":rows})
    );
}
