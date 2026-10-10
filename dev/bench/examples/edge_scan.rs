//! Line-edge reshaping probe for kerning fonts.
//!
//! Usage: `edge_scan MODE REGULAR_FONT BOLD_FONT [ITERATIONS]`, where MODE is
//! `digest` or `time`. The fonts are sfnt files with kerning and contextual
//! features (for example Inter Regular and Bold). The workload builds
//! report-like Latin paragraphs, breaks each at three widths and measures its
//! intrinsic sizes, so most break opportunities end in a word whose last
//! glyph is unsafe to break. `digest` prints a hash of every line, glyph and
//! intrinsic size; `time` prints the best wall time of the whole workload.
use sha2::{Digest, Sha256};
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::limits::Limits;
use shodo::style::{FontFamily, LineOptions, ParagraphStyle};
use shodo::{AtomicIntrinsics, AtomicSizes, Fragment, LayoutContext, Line, RichText};
use std::hint::black_box;
use std::time::Instant;

const WORDS: &[&str] = &[
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "et",
    "dolore",
    "magna",
    "aliqua",
    "enim",
    "ad",
    "minim",
    "veniam",
    "quis",
    "nostrud",
    "exercitation",
    "ullamco",
    "laboris",
    "nisi",
    "aliquip",
    "ex",
    "ea",
    "commodo",
    "consequat",
    "Typography",
    "AVAWAY",
    "office",
    "fluffy",
    "Yesterday,",
    "\u{201c}quoted\u{201d}",
];
const PARAGRAPHS: usize = 40;
const WIDTHS: [f32; 3] = [240.0, 420.0, 610.0];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize
    }
    fn words(&mut self, n: usize) -> String {
        (0..n)
            .map(|_| WORDS[self.next() % WORDS.len()])
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn fonts(regular: &str, bold: &str, limits: &Limits) -> FontCollection {
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for (path, weight) in [(regular, 400.0), (bold, 700.0)] {
        fonts
            .register_face(
                std::fs::read(path).expect("font file"),
                0,
                FontFaceDescriptor {
                    family: "Probe".into(),
                    weight: (weight, weight),
                    ..Default::default()
                },
            )
            .expect("registered face");
    }
    fonts
}

struct Output {
    lines: Vec<Line>,
    intrinsics: Vec<(f32, f32)>,
}

fn workload(fonts: &FontCollection, limits: &Limits) -> Output {
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Probe".into())];
    style.root.font_size = 13.333;
    let mut bold = style.root.clone();
    bold.font_weight = 700.0;
    let mut rng = Rng(1);
    let mut cx = LayoutContext::new();
    let options = LineOptions::default();
    let mut output = Output {
        lines: Vec::new(),
        intrinsics: Vec::new(),
    };
    for _ in 0..PARAGRAPHS {
        let paragraph = RichText::with_limits(&style, limits)
            .push(&(rng.words(80) + " "), &style.root)
            .push(&rng.words(3), &bold)
            .push(&(" ".to_string() + &rng.words(30) + "."), &style.root)
            .build(&mut cx, fonts)
            .expect("paragraph");
        let sizes = paragraph.intrinsic_sizes(&mut cx, &options, &AtomicIntrinsics::EMPTY);
        output
            .intrinsics
            .push((sizes.min_content, sizes.max_content));
        for width in WIDTHS {
            output
                .lines
                .extend(paragraph.break_all(&mut cx, &options, width, &AtomicSizes::EMPTY));
        }
    }
    output
}

fn digest(output: &Output) -> String {
    let mut hash = Sha256::new();
    let mut glyphs = 0usize;
    for line in &output.lines {
        hash.update((line.text_range().start as u64).to_le_bytes());
        hash.update((line.text_range().end as u64).to_le_bytes());
        hash.update(line.inline_size().to_bits().to_le_bytes());
        hash.update(line.block_size().to_bits().to_le_bytes());
        for fragment in line.fragments() {
            if let Fragment::GlyphRun(run) = fragment {
                hash.update(run.font_size().to_bits().to_le_bytes());
                for g in run.glyphs() {
                    glyphs += 1;
                    hash.update(u64::from(g.id).to_le_bytes());
                    hash.update((g.cluster as u64).to_le_bytes());
                    hash.update(g.inline_position.to_bits().to_le_bytes());
                    hash.update(g.block_offset.to_bits().to_le_bytes());
                    hash.update(g.advance.to_bits().to_le_bytes());
                }
            }
        }
    }
    for (min, max) in &output.intrinsics {
        hash.update(min.to_bits().to_le_bytes());
        hash.update(max.to_bits().to_le_bytes());
    }
    format!(
        "{:x} lines={} glyphs={}",
        hash.finalize(),
        output.lines.len(),
        glyphs
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(mode), Some(regular), Some(bold)) = (args.get(1), args.get(2), args.get(3)) else {
        eprintln!("usage: edge_scan digest|time REGULAR_FONT BOLD_FONT [ITERATIONS]");
        std::process::exit(2);
    };
    let limits = Limits::default();
    let fonts = fonts(regular, bold, &limits);
    match mode.as_str() {
        "digest" => println!("{}", digest(&workload(&fonts, &limits))),
        "time" => {
            let iterations = args.get(4).map_or(5, |n| n.parse().expect("iterations"));
            let mut best = f64::INFINITY;
            for _ in 0..iterations {
                let start = Instant::now();
                black_box(workload(&fonts, &limits));
                best = best.min(start.elapsed().as_secs_f64());
            }
            println!("best {:.2} ms", best * 1e3);
        }
        _ => {
            eprintln!("unknown mode {mode}");
            std::process::exit(2);
        }
    }
}
