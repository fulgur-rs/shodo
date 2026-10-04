//! Missing-font scalar probe (shodo-zb0.13).
//! Builds paragraphs whose scalars have no matching face, so every scalar
//! takes the `.notdef` path in `shape_inputs`, then prints an output digest
//! plus build timing or allocation counts.
//!
//! Modes: `digest`, `time`, `alloc` (needs `allocation-counting`), and
//! `loop <case> <n>` for valgrind runs. An optional trailing case name filters
//! `digest`/`time`/`alloc`.
use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::font::{FontCollection, FontOptions};
use shodo::geometry::WritingMode;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{InlineStyle, LineOptions, ParagraphStyle};
use shodo::{
    AtomicSizes, Fragment, LayoutContext, Line, Paragraph, ParagraphBuilder, Ruby, RubyAnnotation,
    RubyBase, RubyContent, RubyLevel,
};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

#[derive(Clone, Copy, PartialEq)]
enum Fonts {
    /// No faces at all: every scalar is missing.
    Empty,
    /// Latin, CJK and Arabic fixtures: only uncovered scripts are missing.
    Fixture,
}

#[derive(Clone, Copy, PartialEq)]
enum Content {
    Text(&'static str),
    /// Ruby containers whose base content carries its own limits.
    Ruby {
        base: &'static str,
        annotation: &'static str,
        base_budget: Option<u64>,
        base_glyphs: Option<u64>,
    },
}

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    content: Content,
    repeat: usize,
    fonts: Fonts,
    mode: WritingMode,
    font_size: f32,
    budget: Option<u64>,
}

const LATIN: &str = "The quick brown fox jumps over the lazy dog. ";
// Base letters followed by combining marks (zero advance, negative offset).
const COMBINING: &str = "e\u{301}a\u{308}o\u{302}\u{323}u\u{30a}";
// ZWJ, ZWSP and ZWNJ stay in the input as zero-advance `.notdef` glyphs.
const IGNORABLE: &str = "a\u{200d}b\u{200b}c\u{200c}d";
const CJK: &str = "日本語組版句読点空白調整文章条件";
// Cherokee is not covered by the fixture faces; Latin between words is.
const MIXED: &str = "ᏣᎳᎩ abc ᎦᏬᏂᎯᏍᏗ ";

fn cases() -> Vec<Case> {
    let text = |name, text, repeat| Case {
        name,
        content: Content::Text(text),
        repeat,
        fonts: Fonts::Empty,
        mode: WritingMode::HorizontalTb,
        font_size: 16.0,
        budget: None,
    };
    let ruby = |name, base_budget, base_glyphs| Case {
        name,
        content: Content::Ruby {
            base: "ᏣᎳᎩᎦᏬᏂ",
            annotation: "abc",
            base_budget,
            base_glyphs,
        },
        repeat: 64,
        fonts: Fonts::Fixture,
        mode: WritingMode::HorizontalTb,
        font_size: 16.0,
        budget: None,
    };
    vec![
        text("short", "abc", 1),
        text("latin-64", LATIN, 64),
        text("latin-1024", LATIN, 1024),
        text("combining-256", COMBINING, 256),
        text("ignorable-256", IGNORABLE, 256),
        Case {
            budget: Some(16),
            ..text("latin-split16", LATIN, 64)
        },
        Case {
            font_size: 1.0e6,
            ..text("latin-huge-size", LATIN, 4)
        },
        Case {
            mode: WritingMode::VerticalRl,
            ..text("cjk-vertical", CJK, 64)
        },
        Case {
            fonts: Fonts::Fixture,
            ..text("mixed-items", MIXED, 64)
        },
        ruby("ruby-base-split6", Some(6), None),
        ruby("ruby-base-glyph-limit", Some(6), Some(4)),
    ]
}

fn load(fonts: Fonts) -> FontCollection {
    let limits = Limits::default();
    match fonts {
        Fonts::Empty => FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        ),
        Fonts::Fixture => {
            shodo_fixtures::load_fonts(&limits)
                .expect("fixture fonts")
                .collection
        }
    }
}

fn builder(case: &Case) -> ParagraphBuilder {
    let limits = Limits {
        max_shaping_run_bytes: case.budget,
        ..Limits::default()
    };
    let root = InlineStyle {
        font_size: case.font_size,
        ..Default::default()
    };
    let style = ParagraphStyle {
        root: root.clone(),
        writing_mode: case.mode,
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    match case.content {
        Content::Text(text) => {
            builder.push_text(
                TextSource::Generated { node: NodeId(1) },
                &text.repeat(case.repeat),
            );
        }
        Content::Ruby {
            base,
            annotation,
            base_budget,
            base_glyphs,
        } => {
            let base_limits = Limits {
                max_shaping_run_bytes: base_budget,
                max_shaped_glyphs: base_glyphs,
                ..Limits::default()
            };
            for i in 0..case.repeat as u64 {
                let node = NodeId(1 + i * 4);
                let content = |node, text, limits| {
                    RubyContent::text(TextSource::Generated { node }, text, &root, limits)
                };
                let ruby = Ruby::new(
                    vec![RubyBase {
                        node: NodeId(node.0 + 1),
                        content: content(NodeId(node.0 + 1), base, &base_limits),
                        align: Default::default(),
                    }],
                    vec![RubyLevel {
                        annotations: vec![RubyAnnotation {
                            node: NodeId(node.0 + 2),
                            content: content(NodeId(node.0 + 2), annotation, &limits),
                            span: Default::default(),
                            visibility: Default::default(),
                        }],
                        style: Default::default(),
                    }],
                )
                .expect("valid ruby");
                builder.push_ruby(node, &root, ruby);
                builder.push_text(
                    TextSource::Generated {
                        node: NodeId(node.0 + 3),
                    },
                    " ",
                );
            }
        }
    }
    builder
}

fn add(hash: &mut Sha256, value: u64) {
    hash.update(value.to_le_bytes());
}

fn add_f32(hash: &mut Sha256, value: f32) {
    hash.update(value.to_bits().to_le_bytes());
}

fn hash_line(hash: &mut Sha256, line: &Line) {
    add(hash, line.text_range().start as u64);
    add(hash, line.text_range().end as u64);
    for value in [line.inline_size(), line.block_size(), line.block_offset()] {
        add_f32(hash, value);
    }
    for fragment in line.fragments() {
        match fragment {
            Fragment::GlyphRun(run) => {
                hash.update(b"run");
                add(hash, run.text_range().start as u64);
                add(hash, run.text_range().end as u64);
                // FontId values are collection-scoped; hash the retained face
                // instead so digests compare across processes. An empty
                // collection's primary font has no face.
                match run.font_data() {
                    Some(data) => {
                        hash.update(Sha256::digest(data.data.as_ref()));
                        add(hash, data.index as u64);
                    }
                    None => hash.update(b"no-face"),
                }
                hash.update(format!("{:?}", run.normalized_coords()).as_bytes());
                hash.update([run.bidi_level()]);
                hash.update(format!("{:?}", run.orientation()).as_bytes());
                add_f32(hash, run.font_size());
                for (index, glyph) in run.glyphs().enumerate() {
                    add(hash, glyph.id as u64);
                    add(hash, glyph.cluster as u64);
                    for value in [glyph.inline_position, glyph.block_offset, glyph.advance] {
                        add_f32(hash, value);
                    }
                    let origin = run.glyph_origin(index);
                    hash.update([u8::from(origin.is_some())]);
                    if let Some((x, y)) = origin {
                        add_f32(hash, x);
                        add_f32(hash, y);
                    }
                }
            }
            // Annotation lines are hashed below; their Debug form carries
            // per-build owner identity.
            Fragment::RubyAnnotation(annotation) => {
                hash.update(b"annotation");
                add(hash, annotation.level() as u64);
                hash.update(format!("{:?}", annotation.transform()).as_bytes());
            }
            other => hash.update(format!("{other:?}").as_bytes()),
        }
    }
    for annotation in line.ruby_annotations() {
        hash.update(b"ruby");
        add(hash, annotation.base_text_range().start as u64);
        add(hash, annotation.base_text_range().end as u64);
        hash_line(hash, annotation.line());
    }
}

struct Output {
    sha256: String,
    runs: usize,
    glyphs: usize,
    warnings: Vec<String>,
}

fn output(result: &Result<Paragraph, shodo::limits::LimitExceeded>) -> Output {
    let mut hash = Sha256::new();
    let paragraph = match result {
        Ok(paragraph) => paragraph,
        Err(error) => {
            return Output {
                sha256: format!("error:{error:?}"),
                runs: 0,
                glyphs: 0,
                warnings: Vec::new(),
            };
        }
    };
    let mut context = LayoutContext::new();
    let lines = paragraph.break_all(
        &mut context,
        &LineOptions::default(),
        480.0,
        &AtomicSizes::EMPTY,
    );
    let mut runs = 0;
    let mut glyphs = 0;
    for line in &lines {
        for fragment in line.fragments() {
            if let Fragment::GlyphRun(run) = fragment {
                runs += 1;
                glyphs += run.glyphs().len();
            }
        }
        hash_line(&mut hash, line);
    }
    let warnings: Vec<_> = paragraph
        .warnings()
        .iter()
        .chain(&context.take_warnings())
        .map(|w| format!("{:?}:{}", w.kind, w.message))
        .collect();
    for warning in &warnings {
        hash.update(warning.as_bytes());
    }
    Output {
        sha256: format!("{:x}", hash.finalize()),
        runs,
        glyphs,
        warnings,
    }
}

fn build(case: &Case, fonts: &FontCollection) -> Result<Paragraph, shodo::limits::LimitExceeded> {
    builder(case).build(&mut LayoutContext::new(), fonts)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map_or("digest", String::as_str);
    let all = cases();
    let select = |name: Option<&String>| -> Vec<Case> {
        all.iter()
            .filter(|c| name.is_none_or(|n| n == c.name))
            .copied()
            .collect()
    };
    match mode {
        "loop" => {
            let case = select(args.get(1))[0];
            let n: usize = args.get(2).map_or(10, |n| n.parse().unwrap());
            let fonts = load(case.fonts);
            for _ in 0..n {
                black_box(build(black_box(&case), &fonts)).ok();
            }
        }
        "digest" | "time" | "alloc" => {
            for case in select(args.get(1)) {
                let fonts = load(case.fonts);
                // Warm font caches outside every measured scope.
                let reference = output(&build(&case, &fonts));
                assert_eq!(
                    output(&build(&case, &fonts)).sha256,
                    reference.sha256,
                    "unstable output: {}",
                    case.name
                );
                let measurement = match mode {
                    "time" => {
                        let mut samples: Vec<u64> = (0..21)
                            .map(|_| {
                                let builder = builder(&case);
                                let start = Instant::now();
                                let result =
                                    black_box(builder).build(&mut LayoutContext::new(), &fonts);
                                let ns = start.elapsed().as_nanos() as u64;
                                black_box(result).ok();
                                ns
                            })
                            .collect();
                        samples.sort_unstable();
                        json!({"median_ns": samples[samples.len() / 2], "samples_ns": samples})
                    }
                    "alloc" => {
                        #[cfg(feature = "allocation-counting")]
                        {
                            let builder = builder(&case);
                            let scope = ALLOC.begin().unwrap();
                            let result =
                                black_box(builder).build(&mut LayoutContext::new(), &fonts);
                            let counts = scope.finish();
                            assert_eq!(output(&result).sha256, reference.sha256);
                            json!(counts)
                        }
                        #[cfg(not(feature = "allocation-counting"))]
                        panic!("alloc mode requires --features allocation-counting");
                    }
                    _ => json!(null),
                };
                println!(
                    "{}",
                    json!({
                        "case": case.name,
                        "sha256": reference.sha256,
                        "runs": reference.runs,
                        "glyphs": reference.glyphs,
                        "warnings": reference.warnings,
                        "measurement": measurement,
                    })
                );
            }
        }
        other => panic!("unknown mode {other}"),
    }
}
