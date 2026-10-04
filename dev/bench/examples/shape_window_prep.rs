//! Shaping-window preparation probe (shodo-zb0.7).
//! Builds paragraphs whose items split into many `max_shaping_run_bytes`
//! windows (and single-window controls), then prints an output digest plus
//! build timing or allocation counts.
//!
//! Modes: `digest`, `time`, `alloc` (needs `allocation-counting`), and
//! `loop <case> <n>` for valgrind runs. An optional trailing case name filters
//! `digest`/`time`/`alloc`.
use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::geometry::WritingMode;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
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

const REPEAT: usize = 64;
const SPLIT: Option<u64> = Some(16);
const CJK_FAMILY: &str = "Shodo Fixture CJK";

#[derive(Clone, Copy, PartialEq)]
enum Fonts {
    Fixture,
    CjkWithoutVorg,
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
    fonts: Fonts,
    mode: WritingMode,
    lang: Option<&'static str>,
    budget: Option<u64>,
}

const LATIN: &str = "The quick brown fox jumps over the lazy dog. ";
// No spaces: a space matches the Latin face and would end the item.
const ARABIC: &str = "مرحبابالعالمالجميل";
// Han only: kana or punctuation would split items by script.
const CJK: &str = "日本語組版句読点空白調整文章条件";

fn cases() -> Vec<Case> {
    let text = |name, text, mode, lang, budget| Case {
        name,
        content: Content::Text(text),
        fonts: Fonts::Fixture,
        mode,
        lang,
        budget,
    };
    let ruby = |name, base_budget, base_glyphs| Case {
        name,
        content: Content::Ruby {
            base: "日本語組版",
            annotation: "にほんごのくみはん",
            base_budget,
            base_glyphs,
        },
        fonts: Fonts::Fixture,
        mode: WritingMode::HorizontalTb,
        lang: Some("ja"),
        budget: None,
    };
    let mut cases = vec![
        text(
            "latin-single",
            LATIN,
            WritingMode::HorizontalTb,
            Some("en"),
            None,
        ),
        text(
            "latin-split16",
            LATIN,
            WritingMode::HorizontalTb,
            Some("en"),
            SPLIT,
        ),
        text(
            "arabic-single",
            ARABIC,
            WritingMode::HorizontalTb,
            Some("ar"),
            None,
        ),
        text(
            "arabic-split16",
            ARABIC,
            WritingMode::HorizontalTb,
            Some("ar"),
            SPLIT,
        ),
        text(
            "cjk-upright-vorg-single",
            CJK,
            WritingMode::VerticalRl,
            Some("ja"),
            None,
        ),
        text(
            "cjk-upright-vorg-split16",
            CJK,
            WritingMode::VerticalRl,
            Some("ja"),
            SPLIT,
        ),
        ruby("ruby-base-split6", Some(6), None),
        ruby("ruby-base-glyph-limit", Some(6), Some(4)),
    ];
    for (name, budget) in [
        ("cjk-upright-novorg-single", None),
        ("cjk-upright-novorg-split16", SPLIT),
    ] {
        cases.push(Case {
            fonts: Fonts::CjkWithoutVorg,
            ..text(name, CJK, WritingMode::VerticalRl, Some("ja"), budget)
        });
    }
    cases
}

/// The pinned CJK CFF face with VORG removed, so upright glyphs take the
/// vmtx/CFF-bounds vertical-origin path and its per-window delta cache.
fn cjk_without_vorg() -> Vec<u8> {
    let original = shodo_fixtures::font("cjk").expect("pinned CJK font").bytes;
    let count = u16::from_be_bytes(original[4..6].try_into().unwrap()) as usize;
    let tables: Vec<([u8; 4], &[u8])> = (0..count)
        .filter_map(|n| {
            let at = 12 + n * 16;
            let tag: [u8; 4] = original[at..at + 4].try_into().unwrap();
            let start = u32::from_be_bytes(original[at + 8..at + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(original[at + 12..at + 16].try_into().unwrap()) as usize;
            (&tag != b"VORG").then_some((tag, &original[start..start + len]))
        })
        .collect();
    assert_eq!(
        tables.len() + 1,
        count,
        "fixture has exactly one VORG table"
    );
    let mut out = Vec::new();
    out.extend_from_slice(&original[0..4]);
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        let pad = (4 - data.len() % 4) % 4;
        body.extend(std::iter::repeat_n(0, pad));
        offset += data.len() + pad;
    }
    out.extend_from_slice(&body);
    out
}

fn load(fonts: Fonts) -> FontCollection {
    let limits = Limits::default();
    match fonts {
        Fonts::Fixture => {
            shodo_fixtures::load_fonts(&limits)
                .expect("fixture fonts")
                .collection
        }
        Fonts::CjkWithoutVorg => {
            let collection = FontCollection::with_options(
                &limits,
                FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            );
            collection
                .register_face(
                    cjk_without_vorg(),
                    0,
                    FontFaceDescriptor {
                        family: CJK_FAMILY.into(),
                        ..Default::default()
                    },
                )
                .expect("derived CJK face");
            collection
        }
    }
}

fn builder(case: &Case) -> ParagraphBuilder {
    let limits = Limits {
        max_shaping_run_bytes: case.budget,
        ..Limits::default()
    };
    let mut root = InlineStyle {
        lang: case.lang.map(Into::into),
        ..Default::default()
    };
    if case.fonts == Fonts::CjkWithoutVorg {
        root.font_families = vec![FontFamily::Named(CJK_FAMILY.into())];
    }
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
                &text.repeat(REPEAT),
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
            for i in 0..REPEAT as u64 {
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
                    "を",
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
                // instead so digests compare across processes.
                let data = run.font_data().expect("retained face");
                hash.update(Sha256::digest(data.data.as_ref()));
                add(hash, data.index as u64);
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
