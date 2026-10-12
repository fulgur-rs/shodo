//! Intrinsic-size cost for paragraphs with and without first-line styles.
//!
//! Run with `cargo run --release -p shodo-bench --example first_line_intrinsic_scale -- [words] [samples]`.
//! Paragraph construction is outside the timed region. Cases rotate on each
//! sample so later cases do not always benefit from the same process state.
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, ParagraphStyle};
use shodo::{AtomicIntrinsics, LayoutContext, Paragraph, ParagraphBuilder};
use std::time::Instant;

const WORDS: &[&str] = &[
    "office",
    "first",
    "affinity",
    "flow",
    "the",
    "fluffy",
    "waffle",
    "of",
    "difficult",
    "effort",
    "and",
    "official",
    "fjord",
    "shuffle",
    "to",
    "baffling",
    "in",
    "fifty",
    "offline",
    "staff",
    "profile",
    "a",
    "efficient",
    "suffix",
    "with",
    "field",
];

struct Case {
    name: &'static str,
    style: ParagraphStyle,
    text: String,
}

fn words(count: usize) -> String {
    (0..count)
        .map(|i| WORDS[(i * 7 + i / 3) % WORDS.len()])
        .collect::<Vec<_>>()
        .join(" ")
}

fn base_style() -> ParagraphStyle {
    let mut style = ParagraphStyle::default();
    style.root.font_size = 16.0;
    style.root.font_families = vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())];
    style
}

fn paragraph(style: &ParagraphStyle, text: &str, fonts: &shodo::font::FontCollection) -> Paragraph {
    let mut builder = ParagraphBuilder::new(style, &Limits::default());
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        text,
    );
    builder.build(&mut LayoutContext::new(), fonts).unwrap()
}

fn main() {
    let word_count = std::env::args()
        .nth(1)
        .map_or(4000, |arg| arg.parse().expect("word count"));
    let sample_count = std::env::args()
        .nth(2)
        .map_or(9, |arg| arg.parse().expect("sample count"));
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    let prose = words(word_count);
    let repetitions = word_count.div_ceil(8).max(1);
    let simple = "1 22 333 4444 55 6 777 8 "
        .repeat(repetitions)
        .trim_end()
        .to_owned();
    let simple_with_trailing_space = format!("{simple} ");

    let mut same = base_style();
    same.first_line = Some(same.root.clone());
    let mut paint = same.clone();
    paint.first_line.as_mut().unwrap().paint.color = [0, 0, 255, 255];
    let mut paint_simple = base_style();
    let mut paint_simple_first = paint_simple.root.clone();
    paint_simple_first.paint.color = [0, 0, 255, 255];
    paint_simple.first_line = Some(paint_simple_first);
    let mut size = base_style();
    let mut size_first = size.root.clone();
    size_first.font_size = 22.0;
    size.first_line = Some(size_first);

    let cases = [
        Case {
            name: "none",
            style: base_style(),
            text: prose.clone(),
        },
        Case {
            name: "same",
            style: same,
            text: prose.clone(),
        },
        Case {
            name: "paint",
            style: paint,
            text: prose.clone(),
        },
        Case {
            name: "none-simple",
            style: base_style(),
            text: simple.clone(),
        },
        Case {
            name: "paint-simple",
            style: paint_simple.clone(),
            text: simple.clone(),
        },
        Case {
            name: "none-geometry",
            style: base_style(),
            text: simple_with_trailing_space.clone(),
        },
        Case {
            name: "paint-geometry",
            style: paint_simple,
            text: simple_with_trailing_space,
        },
        Case {
            name: "size-simple",
            style: size.clone(),
            text: simple,
        },
        Case {
            name: "size-ligature",
            style: size,
            text: prose,
        },
    ];
    let paragraphs: Vec<_> = cases
        .iter()
        .map(|case| paragraph(&case.style, &case.text, &fonts.collection))
        .collect();
    let mut samples = vec![Vec::<u128>::new(); cases.len()];
    let mut output = vec![String::new(); cases.len()];

    for round in 0..(sample_count + 3) {
        for offset in 0..cases.len() {
            let index = (round + offset) % cases.len();
            let mut cx = LayoutContext::new();
            let started = Instant::now();
            let sizes = paragraphs[index].intrinsic_sizes(
                &mut cx,
                &Default::default(),
                &AtomicIntrinsics::EMPTY,
            );
            let elapsed = started.elapsed().as_nanos();
            let warnings = cx.take_warnings();
            let value = format!("{sizes:?}{warnings:?}");
            if round < 3 {
                output[index] = value;
            } else {
                if output[index] != value {
                    panic!("unstable intrinsic output for {}", cases[index].name);
                }
                samples[index].push(elapsed);
            }
        }
    }

    for (case, mut timings) in cases.iter().zip(samples) {
        timings.sort_unstable();
        let median = timings[timings.len() / 2];
        let raw = timings
            .iter()
            .map(u128::to_string)
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{}\tmedian_ns={}\tsamples_ns={}\toutput={}",
            case.name,
            median,
            raw,
            output[cases
                .iter()
                .position(|item| item.name == case.name)
                .unwrap()]
        );
    }
}
