//! Byte-pinned real-font shared ligature diagnostic; shipping core unchanged.
#[path = "support/ligature_snapshot.rs"]
mod snapshot;
use serde_json::{Value, json};
use shodo::font::FontFaceDescriptor;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, LineOptions, ParagraphStyle, WordBreak};
use shodo::{
    AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, Paragraph, ParagraphBuilder,
};
use std::time::Instant;
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);
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
    break_all: bool,
    budget: &'static str,
}
impl Case {
    fn word(&self) -> &'static str {
        match self.kind {
            "fi" => "fi",
            "ffi" => "ffi",
            "icon24" => "settings_input_component",
            "icon43" => "signal_cellular_connected_no_internet_4_bar",
            _ => unreachable!(),
        }
    }
    fn key(&self) -> String {
        format!(
            "{}/{}/{}/{}/{}",
            self.kind,
            self.repeats,
            self.width,
            if self.break_all {
                "break-all"
            } else {
                "normal"
            },
            self.budget
        )
    }
    fn limits(&self) -> Limits {
        let mut l = Limits::default();
        match self.budget {
            "default" => {}
            "window-zero" => l.max_reshape_window_bytes = Some(0),
            "glyph-one" => l.max_shaped_glyphs = Some(1),
            _ => unreachable!(),
        };
        l
    }
    fn builder(&self) -> ParagraphBuilder {
        let mut s = ParagraphStyle::default();
        s.root.font_size = 24.;
        s.root.font_families = vec![FontFamily::Named(
            if self.kind.starts_with("icon") {
                "Material Icons"
            } else {
                shodo_fixtures::FONTS[0].family
            }
            .into(),
        )];
        s.root.word_break = if self.break_all {
            WordBreak::BreakAll
        } else {
            WordBreak::Normal
        };
        let mut b = ParagraphBuilder::new(&s, &self.limits());
        b.with_offset_mapping(true);
        b.push_text(
            TextSource::Dom {
                node: NodeId(100),
                offset: 7,
            },
            &self.word().repeat(self.repeats),
        );
        b
    }
}
fn layout(
    p: &Paragraph,
    width: f32,
    cx: &mut LayoutContext,
    inspect: bool,
) -> (Vec<LineResult>, Vec<String>) {
    let mut token = p.start_token();
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let mut end = 0;
    for _ in 0..8192 {
        let con = LineConstraint::new(width);
        let r = p.next_line(
            cx,
            token,
            &LineOptions::default(),
            &con,
            &AtomicSizes::EMPTY,
        );
        let w = cx.take_warnings();
        if inspect {
            let mut fresh = LayoutContext::new();
            let expected = p.next_line(
                &mut fresh,
                token,
                &LineOptions::default(),
                &con,
                &AtomicSizes::EMPTY,
            );
            assert_eq!(
                snapshot::results(std::slice::from_ref(&r)),
                snapshot::results(std::slice::from_ref(&expected))
            );
            assert_eq!(w, fresh.take_warnings());
            let mut retry = LayoutContext::new();
            let mut rejected = con;
            rejected.max_block_size = Some(0.);
            if let LineResult::Line(l) = &r {
                let LineResult::BlockSizeExceeded { needed_block_size } = p.next_line(
                    &mut retry,
                    token,
                    &LineOptions::default(),
                    &rejected,
                    &AtomicSizes::EMPTY,
                ) else {
                    panic!("height rejection")
                };
                assert_eq!(needed_block_size.to_bits(), l.block_size().to_bits());
                retry.take_warnings();
                let accepted = p.next_line(
                    &mut retry,
                    token,
                    &LineOptions::default(),
                    &con,
                    &AtomicSizes::EMPTY,
                );
                assert_eq!(
                    snapshot::results(std::slice::from_ref(&r)),
                    snapshot::results(std::slice::from_ref(&accepted))
                );
                assert_eq!(w, retry.take_warnings());
            }
        }
        // Serializing warning messages is outside measured invocations (inspect=false).
        if inspect {
            warnings.push(format!("{w:?}"));
        }
        match &r {
            LineResult::Line(l) => {
                assert_ne!(token, l.break_token());
                assert_eq!(l.text_range().start, end);
                assert!(l.text_range().end > end);
                end = l.text_range().end;
                token = l.break_token();
            }
            LineResult::Done => {
                out.push(r);
                return (out, warnings);
            }
            other => panic!("unexpected {other:?}"),
        }
        out.push(r);
    }
    panic!("bounded progress")
}
fn run(c: &Case, fonts: &shodo_fixtures::FixtureFonts, inspect: bool) -> Value {
    let b = c.builder();
    let mut build_cx = LayoutContext::new();
    let (p, build) = measured(|| b.build(&mut build_cx, &fonts.collection).unwrap());
    let build_warnings = format!("{:?}", p.warnings());
    let build_context_warnings = format!("{:?}", build_cx.take_warnings());
    let mut cx = LayoutContext::new();
    let (out, cost) = measured(|| layout(&p, c.width, &mut cx, false));
    let lines = out
        .0
        .iter()
        .filter(|r| matches!(r, LineResult::Line(_)))
        .count();
    let oracle = if inspect {
        let output = snapshot::results(&out.0);
        let mut fresh = LayoutContext::new();
        let (check, warnings) = layout(&p, c.width, &mut fresh, true);
        assert_eq!(output, snapshot::results(&check));
        let mut other_build = LayoutContext::new();
        let other = c
            .builder()
            .build(&mut other_build, &fonts.collection)
            .unwrap();
        assert_eq!(format!("{:?}", other_build.take_warnings()), build_warnings);
        assert_eq!(
            format!("{:?}", other_build.take_warnings()),
            build_context_warnings
        );
        if c.budget == "window-zero" || (c.budget == "glyph-one" && c.kind.starts_with("icon")) {
            assert!(!p.warnings().is_empty(), "actual build fallback warning");
        }
        let mut other_cx = LayoutContext::new();
        let (other_out, other_warnings) = layout(&other, c.width, &mut other_cx, true);
        assert_eq!(output, snapshot::results(&other_out));
        assert_eq!(warnings, other_warnings);
        let last = out
            .0
            .iter()
            .filter_map(|r| {
                if let LineResult::Line(l) = r {
                    Some(l.text_range().end)
                } else {
                    None
                }
            })
            .next_back()
            .unwrap();
        assert_eq!(last, c.word().len() * c.repeats);
        if !c.break_all && c.repeats == 1 && c.width == 1024. {
            let glyphs = out
                .0
                .iter()
                .filter_map(|r| {
                    if let LineResult::Line(l) = r {
                        Some(l)
                    } else {
                        None
                    }
                })
                .flat_map(|l| l.fragments())
                .flat_map(|f| match f {
                    Fragment::GlyphRun(r) => {
                        r.glyphs().map(|g| (g.id, g.cluster)).collect::<Vec<_>>()
                    }
                    _ => vec![],
                })
                .collect::<Vec<_>>();
            assert_eq!(glyphs.len(), 1, "actual whole-word real ligature");
            assert_eq!(glyphs[0].1, 0);
        }
        json!({"output":output,"warnings_per_call":warnings,"build_warnings":build_warnings,"build_context_warnings":build_context_warnings,"height_and_fresh_controls":true})
    } else {
        Value::Null
    };
    let (_, output_release) = measured(|| drop(out));
    let (_, context_release) = measured(|| drop(cx));
    let (_, paragraph_release) = measured(|| drop(p));
    let (_, build_context_release) = measured(|| drop(build_cx));
    json!({"build":build,"cost":cost,"output_release":output_release,"context_release":context_release,"paragraph_release":paragraph_release,"build_context_release":build_context_release,"lines":lines,"oracle":oracle})
}
fn main() {
    let samples = std::env::var("SHODO_LIGATURE_SAMPLES")
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(5);
    assert!(samples > 0);
    let reverse = std::env::var_os("SHODO_LIGATURE_REVERSE").is_some();
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    fonts
        .collection
        .register_face(
            include_bytes!("support/material-icons/MaterialIcons-Regular.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Material Icons".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut cases = Vec::new();
    for kind in ["fi", "ffi", "icon24", "icon43"] {
        for repeats in [1, 8] {
            for width in [2., 8., 24., 48., 1024.] {
                for break_all in [false, true] {
                    cases.push(Case {
                        kind,
                        repeats,
                        width,
                        break_all,
                        budget: "default",
                    });
                }
            }
        }
    }
    for kind in ["fi", "icon24", "icon43"] {
        for budget in ["window-zero", "glyph-one"] {
            cases.push(Case {
                kind,
                repeats: 1,
                width: 8.,
                break_all: true,
                budget,
            });
        }
    }
    if reverse {
        cases.reverse();
    }
    let mut rows = Vec::new();
    for c in cases {
        let oracle = run(&c, &fonts, true);
        for _ in 0..2 {
            std::hint::black_box(run(&c, &fonts, false));
        }
        let costs = (0..samples)
            .map(|_| run(&c, &fonts, false))
            .collect::<Vec<_>>();
        rows.push(json!({"key":c.key(),"kind":c.kind,"text":c.word().repeat(c.repeats),"word_bytes":c.word().len(),"repeats":c.repeats,"width":c.width,"break_all":c.break_all,"budget":c.budget,"limits":format!("{:?}",c.limits()),"oracle":oracle,"samples":costs}));
    }
    println!(
        "{}",
        json!({"schema":1,"samples":samples,"reverse":reverse,"rows":rows})
    );
}
