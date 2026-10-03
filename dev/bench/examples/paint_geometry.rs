//! Measure paint_spans and LineLayout construction separately.
use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::geometry::{Direction, WritingMode};
use shodo::hit::{LineLayout, TextPosition};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineOptions, ParagraphStyle, TextAlign, TextCombineUpright,
};
use shodo::{
    AtomicSizes, Fragment, LayoutContext, Line, ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation,
    RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
use shodo_bench::{Operation, Run, Workload, digest, layout};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

struct Case {
    id: String,
    run: Run,
    warnings: Vec<String>,
}

fn measure<T>(f: impl FnOnce() -> T) -> (T, serde_json::Value) {
    #[cfg(feature = "allocation-counting")]
    {
        let scope = ALLOC.begin().unwrap();
        let value = std::hint::black_box(f());
        let counts = scope.finish();
        (value, json!({"allocation":counts}))
    }
    #[cfg(not(feature = "allocation-counting"))]
    {
        let start = std::time::Instant::now();
        let value = std::hint::black_box(f());
        (value, json!({"ns":start.elapsed().as_nanos()}))
    }
}

fn fixture_style(id: &str) -> ParagraphStyle {
    let mut root = InlineStyle {
        font_families: vec![FontFamily::Named(
            shodo_fixtures::font(id).unwrap().family.into(),
        )],
        font_size: 16.0,
        ..Default::default()
    };
    let mut style = ParagraphStyle::default();
    if id == "arabic" {
        root.direction = Direction::Rtl;
        root.lang = Some("ar".into());
        style.direction = Direction::Rtl;
    }
    style.root = root;
    style
}

fn source(node: u64) -> TextSource {
    TextSource::Generated { node: NodeId(node) }
}

fn finish_case(
    id: impl Into<String>,
    builder: ParagraphBuilder,
    width: f32,
    options: &LineOptions,
    fonts: &shodo_fixtures::FixtureFonts,
) -> Case {
    let mut context = LayoutContext::new();
    let paragraph = builder.build(&mut context, &fonts.collection).unwrap();
    let mut warnings = vec![format!("build:{:?}", paragraph.warnings())];
    let lines = paragraph.break_all(&mut context, options, width, &AtomicSizes::EMPTY);
    warnings.push(format!("layout:{:?}", context.take_warnings()));
    Case {
        id: id.into(),
        run: Run {
            lines,
            ..Default::default()
        },
        warnings,
    }
}

fn text_case(
    id: &str,
    style: ParagraphStyle,
    text: &str,
    width: f32,
    options: &LineOptions,
    fonts: &shodo_fixtures::FixtureFonts,
    limits: &Limits,
) -> Case {
    let mut builder = ParagraphBuilder::new(&style, limits);
    builder.push_text(source(1), text);
    finish_case(id, builder, width, options, fonts)
}

fn manual_cases(fonts: &shodo_fixtures::FixtureFonts, limits: &Limits) -> Vec<Case> {
    let latin = fixture_style("latin");
    let mut cases = vec![
        text_case(
            "single-glyph",
            latin.clone(),
            "A",
            10_000.0,
            &LineOptions::default(),
            fonts,
            limits,
        ),
        text_case(
            "combining-marks",
            latin.clone(),
            "Cafe\u{301}, a\u{308}, n\u{303}, A\u{30a}",
            10_000.0,
            &LineOptions::default(),
            fonts,
            limits,
        ),
        text_case(
            "gdef-ligature",
            latin.clone(),
            "ffi",
            10_000.0,
            &LineOptions::default(),
            fonts,
            limits,
        ),
        text_case(
            "rtl-arabic",
            fixture_style("arabic"),
            "مرحبا بالعالم، الكتابة العربية متصلة في الكلمات.",
            10_000.0,
            &LineOptions::default(),
            fonts,
            limits,
        ),
    ];

    let mut spacing = latin.clone();
    spacing.root.letter_spacing = 1.0;
    spacing.root.word_spacing = 2.0;
    cases.push(text_case(
        "spacing",
        spacing,
        "A quiet library keeps its doors open. Readers compare words.",
        10_000.0,
        &LineOptions::default(),
        fonts,
        limits,
    ));

    let mut justify = ParagraphBuilder::new(&latin, limits);
    justify.push_text(
        source(2),
        &"A quiet library keeps its doors open. Readers compare words, spaces, and lines. "
            .repeat(6),
    );
    cases.push(finish_case(
        "justification",
        justify,
        180.0,
        &LineOptions {
            text_align: TextAlign::Justify,
            ..Default::default()
        },
        fonts,
    ));

    cases.push(text_case(
        "soft-hyphen-overlay",
        latin.clone(),
        "of\u{ad}fice",
        24.0,
        &LineOptions::default(),
        fonts,
        limits,
    ));

    let reading = InlineStyle {
        font_size: 24.0,
        ..latin.root.clone()
    };
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: RubyContent::text(source(10), "A", &latin.root, limits),
            align: RubyAlign::SpaceAround,
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(11),
                content: RubyContent::text(source(11), "MMMM", &reading, limits),
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle {
                overhang: shodo::RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let mut ruby_builder = ParagraphBuilder::new(&latin, limits);
    ruby_builder.push_ruby(NodeId(12), &latin.root, ruby);
    cases.push(finish_case(
        "ruby-padding",
        ruby_builder,
        10_000.0,
        &LineOptions::default(),
        fonts,
    ));

    let mut vertical = fixture_style("cjk");
    vertical.writing_mode = WritingMode::VerticalRl;
    vertical.root.text_combine_upright = TextCombineUpright::All;
    cases.push(text_case(
        "vertical-combine",
        vertical,
        "2024",
        10_000.0,
        &LineOptions::default(),
        fonts,
        limits,
    ));

    cases.push(text_case(
        "long-run",
        latin.clone(),
        &"a".repeat(1024),
        100_000.0,
        &LineOptions::default(),
        fonts,
        limits,
    ));

    let mut short_runs = ParagraphBuilder::new(&latin, limits);
    for i in 0..256 {
        let inline = InlineStyle {
            font_size: if i % 2 == 0 { 16.0 } else { 17.0 },
            ..latin.root.clone()
        };
        short_runs
            .open_inline(NodeId(1000 + i), &inline, Default::default())
            .push_text(source(2000 + i), "a")
            .close_inline();
    }
    cases.push(finish_case(
        "many-short-runs",
        short_runs,
        100_000.0,
        &LineOptions::default(),
        fonts,
    ));
    cases
}

fn hash_integer(hash: &mut Sha256, value: usize) {
    hash.update((value as u64).to_le_bytes());
}

fn hash_rect(hash: &mut Sha256, rect: shodo::geometry::LogicalRect) {
    for value in [
        rect.inline_start,
        rect.inline_size,
        rect.block_start,
        rect.block_size,
    ] {
        hash.update(value.to_bits().to_le_bytes());
    }
}

fn line_layout_hash(lines: &[Line], layout: &LineLayout<'_>) -> String {
    let mut hash = Sha256::new();
    for (line_index, line) in lines.iter().enumerate() {
        let range = line.text_range();
        for offset in range.start..=range.end {
            for affinity in [
                shodo::mapping::Affinity::Upstream,
                shodo::mapping::Affinity::Downstream,
            ] {
                hash_integer(&mut hash, line_index);
                hash_integer(&mut hash, offset);
                hash.update([match affinity {
                    shodo::mapping::Affinity::Upstream => 0,
                    shodo::mapping::Affinity::Downstream => 1,
                }]);
                if let Some(caret) = layout.caret(TextPosition {
                    line: line_index,
                    offset: offset as u32,
                    affinity,
                }) {
                    hash.update([1]);
                    hash_integer(&mut hash, caret.position.line);
                    hash_integer(&mut hash, caret.position.offset as usize);
                    hash.update([match caret.position.affinity {
                        shodo::mapping::Affinity::Upstream => 0,
                        shodo::mapping::Affinity::Downstream => 1,
                    }]);
                    hash_rect(&mut hash, caret.rect);
                } else {
                    hash.update([0]);
                }
            }
        }
        let selection = layout.selection_rects(
            TextPosition {
                line: line_index,
                offset: range.start as u32,
                affinity: shodo::mapping::Affinity::Downstream,
            },
            TextPosition {
                line: line_index,
                offset: range.end as u32,
                affinity: shodo::mapping::Affinity::Upstream,
            },
        );
        hash_integer(&mut hash, selection.len());
        for r in selection {
            hash_rect(&mut hash, r);
        }
    }
    format!("{:x}", hash.finalize())
}

fn paint_hash(lines: &[Line]) -> String {
    let spans = lines.iter().map(Line::paint_spans).collect::<Vec<_>>();
    format!("{:x}", Sha256::digest(format!("{spans:?}").as_bytes()))
}

fn emit(case: &Case, fonts: &shodo_fixtures::FixtureFonts, samples: usize) {
    let geometry = digest(&case.run, fonts).unwrap();
    let expected_paint = paint_hash(&case.run.lines);
    let expected_layout = {
        let layout = LineLayout::new(&case.run.lines);
        line_layout_hash(&case.run.lines, &layout)
    };
    for sample in 0..samples {
        let (spans, paint_measure) = measure(|| {
            case.run
                .lines
                .iter()
                .map(Line::paint_spans)
                .collect::<Vec<_>>()
        });
        let actual_paint = format!("{:x}", Sha256::digest(format!("{spans:?}").as_bytes()));
        assert_eq!(actual_paint, expected_paint, "{} paint output", case.id);
        println!(
            "{}",
            json!({"id":case.id,"operation":"paint_spans","sample":sample,"measure":paint_measure,"paint":expected_paint,"geometry":geometry,"warnings":case.warnings})
        );

        let (layout, layout_measure) = measure(|| LineLayout::new(&case.run.lines));
        let actual_layout = line_layout_hash(&case.run.lines, &layout);
        assert_eq!(actual_layout, expected_layout, "{} hit output", case.id);
        println!(
            "{}",
            json!({"id":case.id,"operation":"LineLayout::new","sample":sample,"measure":layout_measure,"layout":expected_layout,"geometry":geometry,"warnings":case.warnings})
        );
    }
}

fn main() {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let samples = std::env::var("SHODO_GEOMETRY_SAMPLES")
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(9);
    assert!(samples > 0);
    let reverse = std::env::var_os("SHODO_GEOMETRY_REVERSE").is_some();
    let mut cases = Vec::new();
    for id in [
        "latin-long",
        "arabic-long",
        "combining-latin",
        "nested-atomic",
        "preserved-tabs",
    ] {
        let work = Workload::named(id, 8).unwrap();
        let mut context = LayoutContext::new();
        let paragraphs = work.build(&mut context, &fonts, &limits).unwrap();
        let run = layout(
            &work,
            &paragraphs,
            &mut context,
            &fonts,
            &limits,
            Operation::AllLines,
        )
        .unwrap();
        let mut warnings: Vec<_> = paragraphs
            .iter()
            .map(|p| format!("build:{:?}", p.warnings()))
            .collect();
        warnings.push(format!("layout:{:?}", context.take_warnings()));
        cases.push(Case {
            id: id.into(),
            run,
            warnings,
        });
    }
    cases.extend(manual_cases(&fonts, &limits));
    for case in &cases {
        match case.id.as_str() {
            "single-glyph" => assert_eq!(
                case.run.lines[0]
                    .fragments()
                    .filter_map(|f| match f {
                        Fragment::GlyphRun(r) => Some(r.glyphs().count()),
                        _ => None,
                    })
                    .sum::<usize>(),
                1
            ),
            "gdef-ligature" => assert_eq!(
                case.run.lines[0]
                    .fragments()
                    .filter_map(|f| match f {
                        Fragment::GlyphRun(r) => Some(r.glyphs().count()),
                        _ => None,
                    })
                    .sum::<usize>(),
                1,
                "fixed Latin face must shape ffi as one glyph"
            ),
            "soft-hyphen-overlay" => {
                assert!(case.run.lines.iter().flat_map(Line::fragments).any(|f| {
                matches!(f, Fragment::GlyphRun(r) if r.clusters().any(|c| c.flags.synthetic_hyphen))
            }), "fixture must select and paint the soft hyphen")
            }
            "ruby-padding" => assert!(
                case.run
                    .lines
                    .iter()
                    .any(|line| line.ruby_annotations().next().is_some())
            ),
            "vertical-combine" => assert!(
                case.run
                    .lines
                    .iter()
                    .any(|line| line.text_combinations().next().is_some())
            ),
            "many-short-runs" => assert!(
                case.run.lines[0]
                    .fragments()
                    .filter(|f| matches!(f, Fragment::GlyphRun(_)))
                    .count()
                    >= 128
            ),
            "long-run" => assert_eq!(case.run.lines[0].text().len(), 1024),
            _ => {}
        }
    }
    if reverse {
        cases.reverse();
    }
    for case in &cases {
        emit(case, &fonts, samples);
    }
}
