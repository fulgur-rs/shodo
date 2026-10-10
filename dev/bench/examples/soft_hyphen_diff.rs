//! Differential output probe for soft-hyphen line breaking. Usage:
//! `<cases>`. Lays out deterministic pseudo-random paragraphs with soft
//! hyphens at several widths and prints one hash over the lossless line
//! output and intrinsic sizes, and one over warnings, to compare two builds.
#[allow(dead_code)]
#[path = "support/fallback_snapshot.rs"]
mod snapshot;

use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, HangingPunctuation, Hyphens, InlineStyle, LineHeight, LineOptions, OverflowWrap,
    ParagraphStyle, TextAlign, TextWrapStyle, WordBreak,
};
use shodo::{
    AtomicIntrinsics, AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const LATIN: &[&str] = &[
    "office", "AVATAR", "Tower", "affine", "waffle", "To", "hyphen", "Wave", "fjord", "LTA", "yo,",
    "end.", "x-ray", "long", "kerning", "VAT",
];
const ARABIC: &[&str] = &[
    "\u{0645}\u{0631}\u{062d}\u{0628}\u{0627}",
    "\u{0628}\u{0627}\u{0644}\u{0639}\u{0627}\u{0644}\u{0645}",
];
const CJK: &[&str] = &["日本語", "文字列", "改行"];

fn word(rng: &mut Rng, shy: u64) -> String {
    let pick = rng.below(20);
    let base: String = if pick < 2 {
        ARABIC[rng.below(ARABIC.len() as u64) as usize].into()
    } else if pick < 3 {
        CJK[rng.below(CJK.len() as u64) as usize].into()
    } else if pick < 4 {
        (0..rng.below(4) + 2)
            .map(|_| LATIN[rng.below(LATIN.len() as u64) as usize])
            .collect()
    } else {
        LATIN[rng.below(LATIN.len() as u64) as usize].into()
    };
    let mut out = String::new();
    for (j, c) in base.chars().enumerate() {
        if j > 0 && rng.below(10) < shy {
            out.push('\u{ad}');
            if rng.below(30) == 0 {
                out.push('\u{ad}');
            }
        }
        out.push(c);
    }
    if rng.below(40) == 0 {
        out.push('\u{ad}');
    }
    out
}

fn style(rng: &mut Rng) -> InlineStyle {
    let fonts = &shodo_fixtures::FONTS;
    InlineStyle {
        font_size: [10.0, 12.5, 16.0][rng.below(3) as usize],
        line_height: LineHeight::Px(18.0),
        letter_spacing: if rng.below(5) == 0 { 1.5 } else { 0.0 },
        font_families: vec![
            FontFamily::Named(fonts[0].family.into()),
            FontFamily::Named(fonts[1].family.into()),
            FontFamily::Named(fonts[2].family.into()),
        ],
        hyphens: if rng.below(8) == 0 {
            Hyphens::None
        } else {
            Hyphens::Manual
        },
        overflow_wrap: [
            OverflowWrap::Normal,
            OverflowWrap::Normal,
            OverflowWrap::Anywhere,
            OverflowWrap::BreakWord,
        ][rng.below(4) as usize],
        word_break: if rng.below(10) == 0 {
            WordBreak::BreakAll
        } else {
            WordBreak::Normal
        },
        ..Default::default()
    }
}

fn main() {
    let cases: u64 = std::env::args()
        .nth(1)
        .expect("use: <cases>")
        .parse()
        .unwrap();
    let mut limits = Limits::default();
    if let Ok(bytes) = std::env::var("SOFT_HYPHEN_DIFF_WINDOW_BYTES") {
        limits.max_reshape_window_bytes = Some(bytes.parse().unwrap());
    }
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let mut total = Sha256::new();
    let mut warning_total = Sha256::new();
    let mut lines_total = 0usize;
    for case in 0..cases {
        let mut rng = Rng(case.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x5eed);
        let shy = rng.below(8) + 1;
        let root = style(&mut rng);
        let ps = ParagraphStyle {
            root: root.clone(),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&ps, &limits);
        let mut node = 1;
        for _ in 0..rng.below(4) + 1 {
            let span = rng.below(3) == 0;
            if span {
                node += 1;
                b.open_inline(NodeId(node), &style(&mut rng), Default::default());
            }
            let text: Vec<_> = (0..rng.below(30) + 1)
                .map(|_| word(&mut rng, shy))
                .collect();
            node += 1;
            b.push_text(
                TextSource::Generated { node: NodeId(node) },
                &(text.join(" ") + " "),
            );
            if span {
                b.close_inline();
            }
        }
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let options = LineOptions {
            text_align: [TextAlign::Start, TextAlign::Justify, TextAlign::Center]
                [rng.below(3) as usize],
            text_wrap_style: [
                TextWrapStyle::Auto,
                TextWrapStyle::Auto,
                TextWrapStyle::Balance,
                TextWrapStyle::Pretty,
            ][rng.below(4) as usize],
            hanging_punctuation: HangingPunctuation {
                allow_end: rng.below(3) == 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cx = LayoutContext::new();
        let mut out = Vec::new();
        for _ in 0..3 {
            let width = 15.0 + rng.below(500) as f32 + 0.25 * rng.below(4) as f32;
            let constraint = LineConstraint::new(width);
            let mut token = p.start_token();
            let mut lines = Vec::new();
            loop {
                // A wider trial first leaves a retained scan that the real
                // width may reuse, as a caller retrying narrower would.
                if rng.below(2) == 0 {
                    let wider = LineConstraint::new(width + 40.0 + rng.below(200) as f32);
                    let _ = p.next_line(&mut cx, token, &options, &wider, &AtomicSizes::EMPTY);
                }
                match p.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY) {
                    LineResult::Line(line) => {
                        token = line.break_token();
                        lines.push(snapshot::line(&line));
                    }
                    LineResult::Done => break,
                    other => panic!("unexpected layout result: {other:?}"),
                }
                assert!(lines.len() < 10_000);
            }
            lines_total += lines.len();
            out.push(json!(lines));
        }
        let sizes = p.intrinsic_sizes(&mut cx, &options, &AtomicIntrinsics::default());
        out.push(json!(format!("{sizes:?}")));
        let warnings: Vec<_> = cx
            .take_warnings()
            .iter()
            .map(|w| format!("{:?}:{}", w.kind, w.message))
            .collect();
        warning_total.update(serde_json::to_vec(&warnings).unwrap());
        let bytes = serde_json::to_vec(&out).unwrap();
        if std::env::var("SOFT_HYPHEN_DIFF_DUMP").is_ok_and(|c| c == case.to_string()) {
            println!("{}", serde_json::to_string_pretty(&out).unwrap());
        }
        if std::env::var_os("SOFT_HYPHEN_DIFF_VERBOSE").is_some() {
            let mut distinct = warnings.clone();
            distinct.sort();
            distinct.dedup();
            println!("{case} {:x} {distinct:?}", Sha256::digest(&bytes));
        }
        total.update(&bytes);
    }
    println!(
        "{}",
        json!({
            "cases":cases, "lines":lines_total,
            "output_sha256":format!("{:x}", total.finalize()),
            "warnings_sha256":format!("{:x}", warning_total.finalize()),
        })
    );
}
