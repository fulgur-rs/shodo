//! Registered candidate preparation: `digest|time|alloc|loop faces distinct|repeat|native [repetitions]`.
//! Use identical source on both revisions. Font registration is outside run_matches.
use sha2::{Digest, Sha256};
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions, FontQuery};
use shodo::limits::Limits;
use shodo::style::FontFamily;
use std::{hint::black_box, time::Instant};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);
const TEXT: &str = "日本語組版句読点空白調整文章条件読みやすい同じで比べます。縦書き横幅文字表示";
fn fonts(faces: usize, native: bool, cap: usize) -> FontCollection {
    let fonts = FontCollection::with_options(
        &Limits::unlimited(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: cap,
            ..Default::default()
        },
    );
    for i in 0..faces {
        let bytes = shodo_fixtures::font("cjk").unwrap().bytes.to_vec();
        if native {
            fonts.register(bytes).unwrap();
        } else {
            fonts
                .register_face(
                    bytes,
                    0,
                    FontFaceDescriptor {
                        family: "Prep".into(),
                        weight: if i % 2 == 0 {
                            (400., 400.)
                        } else {
                            (700., 700.)
                        },
                        ..Default::default()
                    },
                )
                .unwrap();
        }
    }
    fonts
}
#[inline(never)]
fn run_matches(
    fonts: &FontCollection,
    query: &FontQuery,
    clusters: &[String],
    n: usize,
) -> Vec<Option<shodo::font::FontMatch>> {
    let mut out = Vec::with_capacity(clusters.len());
    for _ in 0..n {
        out.clear();
        for cluster in clusters {
            out.push(black_box(
                fonts.match_cluster(black_box(query), black_box(cluster)),
            ));
        }
        black_box(&out);
    }
    out
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(
        (3..=4).contains(&args.len()),
        "digest|time|alloc|loop faces distinct|repeat|native [n]"
    );
    let mode = &args[0];
    let faces: usize = args[1].parse().unwrap();
    assert!([1, 16, 128, 512].contains(&faces));
    let case = &args[2];
    assert!(["distinct", "repeat", "native"].contains(&case.as_str()));
    let n = args.get(3).map_or(20, |s| s.parse().unwrap());
    let clusters: Vec<_> = if case == "repeat" {
        vec!["日".to_owned(); TEXT.chars().count()]
    } else {
        TEXT.chars().map(|c| c.to_string()).collect()
    };
    let fonts = fonts(
        faces,
        case == "native",
        if mode.starts_with("build-") { 256 } else { 8 },
    );
    let query = FontQuery {
        families: vec![FontFamily::Named(
            if case == "native" {
                shodo_fixtures::font("cjk").unwrap().family
            } else {
                "Prep"
            }
            .into(),
        )],
        script: *b"Hani",
        ..Default::default()
    };
    if mode.starts_with("build-") {
        build_report(mode, faces, case, n, fonts, &query, &clusters);
        return;
    }
    #[cfg(feature = "allocation-counting")]
    let scope = if mode == "alloc" {
        Some(ALLOC.begin().unwrap())
    } else {
        None
    };
    let start = Instant::now();
    let out = run_matches(
        &fonts,
        &query,
        &clusters,
        if mode == "digest" { 1 } else { n },
    );
    let ns = start.elapsed().as_nanos();
    #[cfg(feature = "allocation-counting")]
    let counts = scope.map(|s| s.finish());
    let raw: Vec<_> = out
        .iter()
        .map(|m| {
            m.as_ref()
                .map(|m| (m.id.index(), &m.variations, m.embolden, m.skew))
        })
        .collect();
    let raw = format!("{raw:?}");
    let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
    let report = serde_json::json!({"faces":faces,"case":case,"clusters":clusters,"repetitions":if mode == "digest" {1} else {n},"ns":ns,"sha256":digest,"raw":raw});
    #[cfg(feature = "allocation-counting")]
    let mut report = report;
    #[cfg(feature = "allocation-counting")]
    if let Some(counts) = counts {
        report["allocations"] = serde_json::to_value(counts).unwrap();
    }
    // Drop all cache owners under a separate scope so retained payload releases
    // remain observable instead of being misreported as leaked memory.
    drop(out);
    #[cfg(feature = "allocation-counting")]
    let drop_scope = if mode == "alloc" {
        Some(ALLOC.begin().unwrap())
    } else {
        None
    };
    drop(fonts);
    #[cfg(feature = "allocation-counting")]
    if let Some(scope) = drop_scope {
        report["owner_drop"] = serde_json::to_value(scope.finish()).unwrap();
    }
    println!("{report}");
}

#[inline(never)]
fn run_builds(
    fonts: &FontCollection,
    style: &shodo::style::ParagraphStyle,
    text: &str,
    n: usize,
) -> (shodo::Paragraph, shodo::LayoutContext) {
    let limits = Limits::unlimited();
    let mut out = None;
    for _ in 0..n {
        let doc = FontCollection::for_document(fonts, &limits);
        let mut context = shodo::LayoutContext::new();
        let mut builder = shodo::ParagraphBuilder::new(style, &limits);
        builder.push_text(
            shodo::node::TextSource::Generated {
                node: shodo::node::NodeId(1),
            },
            black_box(text),
        );
        out = Some((
            black_box(builder.build(&mut context, &doc).unwrap()),
            context,
        ));
        black_box(&out);
    }
    out.unwrap()
}
fn build_report(
    mode: &str,
    faces: usize,
    case: &str,
    n: usize,
    fonts: FontCollection,
    query: &FontQuery,
    clusters: &[String],
) {
    let text = clusters.concat().repeat(32);
    let style = shodo::style::ParagraphStyle {
        root: shodo::style::InlineStyle {
            font_families: query.families.clone(),
            lang: Some("ja".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    #[cfg(feature = "allocation-counting")]
    let scope = if mode == "build-alloc" {
        Some(ALLOC.begin().unwrap())
    } else {
        None
    };
    let start = Instant::now();
    let (paragraph, mut context) = run_builds(
        &fonts,
        &style,
        &text,
        if mode == "build-digest" { 1 } else { n },
    );
    let ns = start.elapsed().as_nanos();
    #[cfg(feature = "allocation-counting")]
    let counts = scope.map(|scope| scope.finish());
    // Layout and output serialization are outside the measured build region.
    let lines = paragraph.break_all(
        &mut context,
        &shodo::style::LineOptions::default(),
        400.,
        &shodo::AtomicSizes::EMPTY,
    );
    let mut raw = String::new();
    for line in &lines {
        raw.push_str(&format!(
            "{:?}/{:?}/{:?};",
            line.text_range(),
            line.inline_size(),
            line.block_size()
        ));
        for fragment in line.fragments() {
            if let shodo::Fragment::GlyphRun(run) = fragment {
                raw.push_str(&format!(
                    "{:?}/{:?}/{:?}/{:?};",
                    run.font().index(),
                    run.variations(),
                    run.normalized_coords(),
                    run.metrics()
                ));
                for glyph in run.glyphs() {
                    raw.push_str(&format!("{glyph:?};"));
                }
            }
        }
    }
    let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
    drop(lines);
    drop(paragraph);
    drop(context);
    let report = serde_json::json!({"faces":faces,"case":case,"operation":"build","scalars":text.chars().count(),
        "repetitions":if mode=="build-digest" {1} else {n},"ns":ns,"sha256":digest,"bundled_only":fonts.is_bundled_only()});
    #[cfg(feature = "allocation-counting")]
    let mut report = report;
    #[cfg(feature = "allocation-counting")]
    if let Some(counts) = counts {
        report["allocations"] = serde_json::to_value(counts).unwrap();
    }
    drop(fonts);
    println!("{report}");
}
