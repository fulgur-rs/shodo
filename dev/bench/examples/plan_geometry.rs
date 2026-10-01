//! Public fixed-font bounded planning diagnostic. No shipping core instrumentation.
#[path = "support/geometry_snapshot.rs"]
mod snapshot;
use serde_json::{Value, json};
use shodo::limits::Limits;
use shodo::node::{NodeId, OutOfFlowKind, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineOptions, ParagraphStyle, TextTransform, TextWrapStyle, WordBreak,
};
use shodo::{
    AtomicSizes, BreakPlan, FloatCursor, LayoutContext, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan,
    RubyStyle, RubyVisibility,
};
use std::time::Instant;
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);
// Only the archived disposable observer replaces these hooks.
fn trace_reset(_role: usize) {}
fn trace_read() -> Value {
    Value::Null
}
fn measured<T>(role: usize, f: impl FnOnce() -> T) -> (T, Value) {
    trace_reset(role);
    #[cfg(feature = "allocation-counting")]
    let scope = ALLOC.begin().unwrap();
    let start = Instant::now();
    let result = f();
    let ns = start.elapsed().as_nanos() as u64;
    #[cfg(feature = "allocation-counting")]
    let allocation = Some(scope.finish());
    #[cfg(not(feature = "allocation-counting"))]
    let allocation: Option<Value> = None;
    (
        result,
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
            word_break: if self.budget == "edge-zero" {
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
fn cursor(p: &Paragraph, c: &Case, o: &LineOptions) -> Option<FloatCursor> {
    let mut cx = LayoutContext::new();
    let mut placed = None;
    for r in p.lines(
        &mut cx,
        p.start_token(),
        o,
        |prev, offset| {
            if let Some(LineResult::FloatEncountered { float_cursor, .. }) = prev {
                placed = Some(*float_cursor);
            }
            let mut con = LineConstraint::new(c.width);
            con.block_offset = offset;
            con.floats_placed_through = placed;
            con
        },
        &AtomicSizes::EMPTY,
    ) {
        assert!(!matches!(r, LineResult::InvalidToken));
    }
    placed
}
fn accepted(
    p: &Paragraph,
    cx: &mut LayoutContext,
    c: &Case,
    o: &LineOptions,
    plan: &BreakPlan,
    placed: Option<FloatCursor>,
) -> Vec<LineResult> {
    p.lines(
        cx,
        p.start_token(),
        o,
        |_, offset| {
            let mut con = LineConstraint::new(c.width);
            con.block_offset = offset;
            con.break_plan = Some(plan);
            con.floats_placed_through = placed;
            con
        },
        &AtomicSizes::EMPTY,
    )
    .take(c.repeats * 128 + 128)
    .collect()
}
fn run(c: &Case, mode: &str, fonts: &shodo_fixtures::FixtureFonts, inspect: bool) -> Value {
    let l = c.limits();
    let b = c.builder(&l);
    let mut cx = LayoutContext::new();
    let (p, build) = measured(0, || b.build(&mut cx, &fonts.collection).unwrap());
    let build_warnings = format!("{:?}", p.warnings());
    let o = LineOptions {
        text_wrap_style: match mode {
            "Start" => TextWrapStyle::Auto,
            "Balance" => TextWrapStyle::Balance,
            "Pretty" => TextWrapStyle::Pretty,
            _ => unreachable!(),
        },
        ..Default::default()
    };
    // Discover opaque float cursors with an independent context outside scopes.
    let placed = if c.kind == "float" {
        cursor(&p, c, &o)
    } else {
        None
    };
    let (plan, planning) = measured(7, || {
        p.plan_breaks(&mut cx, &o, c.width, &AtomicSizes::EMPTY)
    });
    let plan_warnings = cx.take_warnings();
    let (lines, layout) = measured(4, || accepted(&p, &mut cx, c, &o, &plan, placed));
    let layout_warnings = cx.take_warnings();
    assert!(
        matches!(lines.last(), Some(LineResult::Done)),
        "bounded accepted progress"
    );
    let output = if inspect {
        snapshot::results(&lines)
    } else {
        Value::Null
    };
    let mut height = Value::Null;
    if inspect && !matches!(c.kind, "block" | "float") {
        let mut fresh = LayoutContext::new();
        let mut con = LineConstraint::new(c.width);
        con.break_plan = Some(&plan);
        con.max_block_size = Some(0.);
        let (reject, cost) = measured(5, || {
            p.next_line(&mut fresh, p.start_token(), &o, &con, &AtomicSizes::EMPTY)
        });
        let LineResult::BlockSizeExceeded { needed_block_size } = reject else {
            panic!("height0 rejection")
        };
        con.max_block_size = None;
        let (retry, retry_cost) = measured(6, || {
            p.next_line(&mut fresh, p.start_token(), &o, &con, &AtomicSizes::EMPTY)
        });
        let LineResult::Line(retry) = retry else {
            panic!("same-token retry")
        };
        let LineResult::Line(first) = &lines[0] else {
            panic!("first accepted")
        };
        assert_eq!(snapshot::line(&retry), snapshot::line(first));
        height = json!({"needed_bits":needed_block_size.to_bits(),"reject":cost,"retry":retry_cost,"warnings":format!("{:?}",fresh.take_warnings())});
    }
    if inspect {
        let mut fresh = LayoutContext::new();
        let other = accepted(&p, &mut fresh, c, &o, &plan, placed);
        assert_eq!(snapshot::results(&other), output, "fresh planned output");
        assert_eq!(
            fresh.take_warnings(),
            layout_warnings,
            "fresh planned warnings"
        );
        if mode == "Start" && c.kind != "float" {
            let mut fresh = LayoutContext::new();
            let greedy = p.break_all(&mut fresh, &o, c.width, &AtomicSizes::EMPTY);
            let actual = lines
                .iter()
                .filter_map(|r| {
                    if let LineResult::Line(l) = r {
                        Some(snapshot::line(l))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(
                greedy.iter().map(snapshot::line).collect::<Vec<_>>(),
                actual,
                "direct greedy Start oracle"
            );
        }
        if c.budget == "disabled" && mode != "Start" {
            assert!(!plan_warnings.is_empty());
        }
        for warnings in [&plan_warnings, &layout_warnings] {
            if let Some(cap) = l.max_warnings {
                assert!(warnings.len() as u64 <= cap + 1);
                if warnings.len() as u64 > cap {
                    assert_eq!(
                        warnings.last().unwrap().kind,
                        shodo::limits::WarningKind::Suppressed
                    );
                }
            }
        }
    }
    let plan_value = if inspect {
        snapshot::plan(&plan)
    } else {
        String::new()
    };
    let (_, output_release) = measured(8, || drop(lines));
    let (_, plan_release) = measured(8, || drop(plan));
    let (_, paragraph_release) = measured(8, || drop(p));
    let (_, context_release) = measured(8, || drop(cx));
    json!({"build":build,"plan":planning,"accepted":layout,"height":height,"output_release":output_release,"plan_release":plan_release,"paragraph_release":paragraph_release,"context_release":context_release,"output":output,"plan_debug":plan_value,"warnings":{"build":build_warnings,"plan":format!("{plan_warnings:?}"),"accepted":format!("{layout_warnings:?}")}})
}
fn main() {
    let samples = std::env::var("SHODO_PLAN_SAMPLES")
        .ok()
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(5);
    let reverse = std::env::var_os("SHODO_PLAN_REVERSE").is_some();
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
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
    for budget in ["disabled", "one", "edge-zero"] {
        cases.push(Case {
            kind: "plain",
            repeats: 1,
            width: 80.,
            budget,
        });
    }
    let mut modes = ["Start", "Balance", "Pretty"];
    if reverse {
        cases.reverse();
        modes.reverse();
    }
    let mut rows = Vec::new();
    for c in cases {
        for mode in modes {
            let oracle = run(&c, mode, &fonts, true);
            for _ in 0..2 {
                std::hint::black_box(run(&c, mode, &fonts, false));
            }
            let values = (0..samples)
                .map(|_| run(&c, mode, &fonts, false))
                .collect::<Vec<_>>();
            rows.push(json!({"key":c.key(mode),"kind":c.kind,"repeats":c.repeats,"width":c.width,"budget":c.budget,"mode":mode,"limits":format!("{:?}",c.limits()),"oracle":oracle,"samples":values}));
        }
    }
    println!(
        "{}",
        json!({"schema":1,"samples":samples,"reverse":reverse,"rows":rows})
    );
}
