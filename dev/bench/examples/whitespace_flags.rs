//! Paired paragraph-build measurements for first-line whitespace flag reuse.
//! Run without features for time, then with `allocation-counting` for allocs.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{InlineStyle, ParagraphStyle, WhiteSpaceCollapse};
use shodo::{
    AtomicSizes, LayoutContext, ParagraphBuilder, Ruby, RubyAnnotation, RubyBase, RubyContent,
    RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
use std::hint::black_box;
#[cfg(not(feature = "allocation-counting"))]
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const WARMUP: usize = 3;
const TIME_SAMPLES: usize = 21;
const ALLOCATION_SAMPLES: usize = 9;

#[derive(Clone, Copy)]
enum Fixture {
    SmallFirstLine,
    SmallNoFirstLine,
    PreservedWhitespace,
    LargeFirstLine,
    RubyAnnotation,
}

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    fixture: Fixture,
}

const CASES: [Case; 5] = [
    Case {
        name: "small-first-line",
        fixture: Fixture::SmallFirstLine,
    },
    Case {
        name: "small-no-first-line",
        fixture: Fixture::SmallNoFirstLine,
    },
    Case {
        name: "preserved-whitespace-first-line",
        fixture: Fixture::PreservedWhitespace,
    },
    Case {
        name: "large-first-line",
        fixture: Fixture::LargeFirstLine,
    },
    Case {
        name: "ruby-annotation-first-line",
        fixture: Fixture::RubyAnnotation,
    },
];

fn family_style() -> InlineStyle {
    InlineStyle {
        font_families: vec![shodo::style::FontFamily::Named(
            shodo_fixtures::FONTS[1].family.into(),
        )],
        ..Default::default()
    }
}

fn paragraph_style(fixture: Fixture) -> ParagraphStyle {
    let mut root = family_style();
    if matches!(fixture, Fixture::PreservedWhitespace) {
        root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    }
    ParagraphStyle {
        first_line: (!matches!(fixture, Fixture::SmallNoFirstLine)).then(|| InlineStyle {
            font_size: 18.0,
            ..root.clone()
        }),
        root,
        ..Default::default()
    }
}

fn annotation(node: u64, text: &str, style: &InlineStyle, limits: &Limits) -> RubyContent {
    let paragraph = ParagraphStyle {
        root: style.clone(),
        first_line: Some(InlineStyle {
            font_size: style.font_size + 1.0,
            ..style.clone()
        }),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph, limits);
    builder.push_text(TextSource::Generated { node: NodeId(node) }, text);
    RubyContent::from_builder(builder)
}

fn builder(case: Case, limits: &Limits) -> ParagraphBuilder {
    let style = paragraph_style(case.fixture);
    let mut builder = ParagraphBuilder::new(&style, limits);
    match case.fixture {
        Fixture::SmallFirstLine | Fixture::SmallNoFirstLine => {
            let text = "a  日\n b ".repeat(24);
            builder.push_text(TextSource::Generated { node: NodeId(1) }, &text);
        }
        Fixture::PreservedWhitespace => {
            let text = "a \t\n日  b\n".repeat(24);
            builder.push_text(TextSource::Generated { node: NodeId(1) }, &text);
        }
        Fixture::LargeFirstLine => {
            let text = "a  日\n b ".repeat(4096);
            builder.push_text(TextSource::Generated { node: NodeId(1) }, &text);
        }
        Fixture::RubyAnnotation => {
            let base_style = family_style();
            let base = RubyContent::text(
                TextSource::Generated { node: NodeId(10) },
                "日本",
                &base_style,
                limits,
            );
            let reading = InlineStyle {
                font_size: 8.0,
                ..family_style()
            };
            let reading = annotation(11, "に ほん\n", &reading, limits);
            let ruby = Ruby::new(
                vec![RubyBase {
                    node: NodeId(10),
                    content: base,
                    align: Default::default(),
                }],
                vec![RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(11),
                        content: reading,
                        span: RubySpan::All,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle::default(),
                }],
            )
            .unwrap();
            builder
                .push_text(TextSource::Generated { node: NodeId(1) }, "a ")
                .push_ruby(NodeId(12), &base_style, ruby)
                .push_text(
                    TextSource::Generated { node: NodeId(2) },
                    &" b 日本".repeat(24),
                );
        }
    }
    builder
}

fn sample(case: Case, mode: &str, fonts: &shodo_fixtures::FixtureFonts, index: usize) -> Value {
    let limits = Limits::default();
    let input = builder(case, &limits);
    let mut context = LayoutContext::new();
    #[cfg(feature = "allocation-counting")]
    let (paragraph, measurement) = {
        let scope = ALLOC.begin().unwrap();
        let paragraph = black_box(input)
            .build(&mut context, &fonts.collection)
            .unwrap();
        let counts = scope.finish();
        (paragraph, json!({"allocations": counts}))
    };
    #[cfg(not(feature = "allocation-counting"))]
    let (paragraph, measurement) = {
        let start = Instant::now();
        let paragraph = black_box(input)
            .build(&mut context, &fonts.collection)
            .unwrap();
        let elapsed = start.elapsed().as_nanos() as u64;
        (paragraph, json!({"ns": elapsed}))
    };
    black_box(&paragraph);
    let build_warnings = format!("{:?}", paragraph.warnings());
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        if matches!(case.fixture, Fixture::LargeFirstLine) {
            800.0
        } else {
            180.0
        },
        &AtomicSizes::EMPTY,
    );
    let layout_warnings = format!("{:?}", context.take_warnings());
    let output: Vec<_> = lines.iter().map(snapshot::line).collect();
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
    json!({
        "schema": 1,
        "mode": mode,
        "fixture": case.name,
        "sample": index,
        "output_sha256": digest,
        "build_warnings": build_warnings,
        "layout_warnings": layout_warnings,
        "measurement": measurement,
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "time".into());
    let only_case = args.next();
    let expected_mode = if cfg!(feature = "allocation-counting") {
        "alloc"
    } else {
        "time"
    };
    assert!(
        mode == expected_mode || (!cfg!(feature = "allocation-counting") && mode == "single"),
        "use separate time and allocation builds; `single` is available without allocation counting"
    );
    let (warmups, samples) = if mode == "single" {
        (0, 1)
    } else if mode == "time" {
        (WARMUP, TIME_SAMPLES)
    } else {
        (WARMUP, ALLOCATION_SAMPLES)
    };
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    for case in CASES
        .into_iter()
        .filter(|case| only_case.as_deref().is_none_or(|name| name == case.name))
    {
        for _ in 0..warmups {
            black_box(sample(case, &mode, &fonts, usize::MAX));
        }
        for index in 0..samples {
            println!(
                "{}",
                serde_json::to_string(&sample(case, &mode, &fonts, index)).unwrap()
            );
        }
    }
}
