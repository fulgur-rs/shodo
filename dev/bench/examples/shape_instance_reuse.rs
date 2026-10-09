//! Per-call font instance reuse probe (shodo-5nl).
//! Builds interleaved Latin/CJK/emoji runs and a single-run control; prints
//! build timing or allocation counts.
//!
//! Modes: `digest`, `time`/`alloc` (build), `shape-time`/`shape-alloc`
//! (analysis prepared before the measured scope), and `loop <case> <n>`
//! (full builds for Valgrind). `alloc` modes need `allocation-counting`.
//! A trailing case name filters measurement modes. Timing excludes result
//! destruction; allocation output separates retained result and result drop.
//! The consumed builder/analysis is already live at the allocation boundary,
//! so net bytes are signed deltas, not the total paragraph size or RSS.
use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::font::FontCollection;
use shodo::geometry::WritingMode;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, LayoutContext, Line, Paragraph, ParagraphBuilder};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const REPEAT: usize = 256;

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    text: &'static str,
    vertical: bool,
    adjust: Option<shodo::style::FontSizeAdjust>,
    max_warnings: Option<u64>,
    styles: usize,
    variations: usize,
}

fn cases() -> Vec<Case> {
    let case = |name, text| Case {
        name,
        text,
        vertical: false,
        adjust: None,
        max_warnings: None,
        styles: 0,
        variations: 0,
    };
    vec![
        case("latin-single", "abc"),
        case("latin-cjk", "a水b"),
        case("latin-emoji", "a😀b"),
        case("cjk-emoji", "水😀日"),
        Case {
            vertical: true,
            ..case("vertical-mixed", "水😀12")
        },
        Case {
            adjust: Some(shodo::style::FontSizeAdjust {
                metric: shodo::style::FontMetricKind::ExHeight,
                value: 0.5,
            }),
            ..case("size-adjust", "a😀b")
        },
        Case {
            adjust: Some(shodo::style::FontSizeAdjust {
                metric: shodo::style::FontMetricKind::IcHeight,
                value: 0.5,
            }),
            max_warnings: Some(3),
            ..case("warning-limit3", "a😀b")
        },
        Case {
            adjust: Some(shodo::style::FontSizeAdjust {
                metric: shodo::style::FontMetricKind::IcHeight,
                value: 0.5,
            }),
            max_warnings: Some(0),
            ..case("warning-limit0", "a😀b")
        },
        Case {
            styles: 9,
            ..case("nine-sizes", "a😀b")
        },
        Case {
            variations: 4096,
            ..case("oversized-variations", "a😀b")
        },
    ]
}

fn load() -> FontCollection {
    shodo_fixtures::load_emoji_fonts(&Limits::default())
        .expect("pinned fonts")
        .base
        .collection
}

fn builder(case: &Case) -> ParagraphBuilder {
    let limits = Limits {
        max_warnings: case.max_warnings,
        ..Limits::default()
    };
    let root = InlineStyle {
        font_size_adjust: case.adjust,
        font_variations: (0..case.variations)
            .map(|index| shodo::style::FontVariation {
                tag: (0x70000000 + index as u32).to_be_bytes(),
                value: index as f32,
            })
            .collect(),
        ..Default::default()
    };
    let style = ParagraphStyle {
        root: root.clone(),
        writing_mode: if case.vertical {
            WritingMode::VerticalRl
        } else {
            WritingMode::HorizontalTb
        },
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    if case.styles == 0 {
        builder.push_text(
            TextSource::Generated { node: NodeId(1) },
            &case.text.repeat(REPEAT),
        );
    } else {
        for index in 0..REPEAT {
            let node = NodeId(index as u64 + 1);
            let style = InlineStyle {
                font_size: 16.0 + (index % case.styles) as f32,
                ..root.clone()
            };
            builder
                .open_inline(node, &style, shodo::node::InlineEdges::default())
                .push_text(TextSource::Generated { node }, case.text)
                .close_inline();
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
                // instead so digests compare across processes.
                let data = run.font_data().expect("retained face");
                hash.update(Sha256::digest(data.data.as_ref()));
                add(hash, data.index as u64);
                hash.update(format!("{:?}", run.normalized_coords()).as_bytes());
                hash.update(format!("{:?}", run.variations()).as_bytes());
                hash.update(format!("{:?}", run.metrics()).as_bytes());
                hash.update(format!("{:?}", run.vertical_metrics()).as_bytes());
                hash.update(format!("{:?}", run.script()).as_bytes());
                hash.update(format!("{:?}", run.language()).as_bytes());
                hash.update(format!("{:?}", (run.embolden(), run.skew())).as_bytes());
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
    let mut glyphs = 0;
    for line in &lines {
        glyphs += line
            .fragments()
            .map(|f| match f {
                Fragment::GlyphRun(run) => run.glyphs().len(),
                _ => 0,
            })
            .sum::<usize>();
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
            let fonts = load();
            for _ in 0..n {
                black_box(build(black_box(&case), &fonts)).ok();
            }
        }
        "digest" | "time" | "shape-time" | "alloc" | "shape-alloc" => {
            for case in select(args.get(1)) {
                let fonts = load();
                // Warm font caches outside every measured scope.
                let reference = output(&build(&case, &fonts));
                assert_eq!(
                    output(&build(&case, &fonts)).sha256,
                    reference.sha256,
                    "unstable output: {}",
                    case.name
                );
                let measurement = match mode {
                    "time" | "shape-time" => {
                        let mut samples: Vec<u64> = (0..21)
                            .map(|_| {
                                let (builder, analysis) = if mode == "shape-time" {
                                    (None, Some(builder(&case).analyze().unwrap()))
                                } else {
                                    (Some(builder(&case)), None)
                                };
                                let start = Instant::now();
                                let result = if let Some(analysis) = analysis {
                                    black_box(analysis).shape(&mut LayoutContext::new(), &fonts)
                                } else {
                                    black_box(builder.unwrap())
                                        .build(&mut LayoutContext::new(), &fonts)
                                };
                                let ns = start.elapsed().as_nanos() as u64;
                                black_box(result).ok();
                                ns
                            })
                            .collect();
                        samples.sort_unstable();
                        json!({"median_ns": samples[samples.len() / 2], "samples_ns": samples})
                    }
                    "alloc" | "shape-alloc" => {
                        #[cfg(feature = "allocation-counting")]
                        {
                            let (builder, analysis) = if mode == "shape-alloc" {
                                (None, Some(builder(&case).analyze().unwrap()))
                            } else {
                                (Some(builder(&case)), None)
                            };
                            let scope = ALLOC.begin().unwrap();
                            let result = if let Some(analysis) = analysis {
                                black_box(analysis).shape(&mut LayoutContext::new(), &fonts)
                            } else {
                                black_box(builder.unwrap()).build(&mut LayoutContext::new(), &fonts)
                            };
                            let counts = scope.finish();
                            assert_eq!(output(&result).sha256, reference.sha256);
                            let scope = ALLOC.begin().unwrap();
                            drop(result);
                            let dropped = scope.finish();
                            json!({"retained": counts, "drop": dropped})
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
