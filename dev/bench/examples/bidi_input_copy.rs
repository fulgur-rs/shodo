//! Fixed Arabic and bidi paragraph build probe for line-separator normalization.
//! Run without features for timing, then with `allocation-counting` and `alloc`.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::geometry::{Direction, WritingMode};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineOptions, ParagraphStyle, TextOrientation, WhiteSpaceCollapse,
};
use shodo::{AtomicSizes, Fragment, LayoutContext, Paragraph, ParagraphBuilder};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const WARMUP: usize = 3;
const FONT_FAMILY: &str = "Shodo Fixture Arabic";

struct Case {
    name: &'static str,
    text: String,
    direction: Direction,
    plaintext: bool,
    writing_mode: WritingMode,
    upright: bool,
    white_space_collapse: WhiteSpaceCollapse,
}

fn cases() -> Vec<Case> {
    let arabic = "سلام لا ".repeat(1024);
    let with_separators = (0..1024)
        .map(|index| {
            if index > 0 && index % 64 == 0 {
                "\u{2028}"
            } else {
                ""
            }
            .to_owned()
                + "سلام لا "
        })
        .collect();
    vec![
        Case {
            name: "rtl-arabic-long-without-u2028",
            text: arabic,
            direction: Direction::Rtl,
            plaintext: false,
            writing_mode: WritingMode::HorizontalTb,
            upright: false,
            white_space_collapse: WhiteSpaceCollapse::Collapse,
        },
        Case {
            name: "rtl-arabic-long-with-u2028",
            text: with_separators,
            direction: Direction::Rtl,
            plaintext: false,
            writing_mode: WritingMode::HorizontalTb,
            upright: false,
            white_space_collapse: WhiteSpaceCollapse::Collapse,
        },
        Case {
            name: "plaintext-neutral-inheritance-controls",
            text: ["אב\u{2066}abc\u{2069}", "123", "abc"].repeat(8).join("\n"),
            direction: Direction::Ltr,
            plaintext: true,
            writing_mode: WritingMode::HorizontalTb,
            upright: false,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
        },
        Case {
            name: "vertical-upright-arabic",
            text: "سلام 123\n".repeat(64),
            direction: Direction::Ltr,
            plaintext: false,
            writing_mode: WritingMode::VerticalRl,
            upright: true,
            white_space_collapse: WhiteSpaceCollapse::Collapse,
        },
    ]
}

fn builder(case: &Case, limits: &Limits) -> ParagraphBuilder {
    let root = InlineStyle {
        font_families: vec![FontFamily::Named(FONT_FAMILY.into())],
        direction: case.direction,
        white_space_collapse: case.white_space_collapse,
        text_orientation: if case.upright {
            TextOrientation::Upright
        } else {
            TextOrientation::Mixed
        },
        ..Default::default()
    };
    let paragraph_style = ParagraphStyle {
        root: root.clone(),
        direction: case.direction,
        unicode_bidi_plaintext: case.plaintext,
        writing_mode: case.writing_mode,
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        &case.text,
    );
    builder
}

fn add_usize(hash: &mut Sha256, value: usize) {
    hash.update((value as u64).to_le_bytes());
}

fn add_f32(hash: &mut Sha256, value: f32) {
    hash.update(value.to_bits().to_le_bytes());
}

struct Snapshot {
    output_sha256: String,
    warning_sha256: String,
    warnings: Vec<String>,
}

fn snapshot(paragraph: &Paragraph, context: &mut LayoutContext) -> Snapshot {
    let mut output = Sha256::new();
    output.update(paragraph.text().as_bytes());
    output.update(format!("{:?}", paragraph.offset_mapping()).as_bytes());
    add_usize(&mut output, paragraph.text().len());

    let lines = paragraph.break_all(
        context,
        &LineOptions::default(),
        1_000_000.0,
        &AtomicSizes::EMPTY,
    );
    add_usize(&mut output, lines.len());
    for line in &lines {
        output.update(line.text().as_bytes());
        add_usize(&mut output, line.text_range().start);
        add_usize(&mut output, line.text_range().end);
        for value in [
            line.inline_size(),
            line.block_size(),
            line.block_offset(),
            line.hang_start(),
            line.hang_end(),
        ] {
            add_f32(&mut output, value);
        }
        for fragment in line.fragments() {
            match fragment {
                Fragment::GlyphRun(run) => {
                    output.update(b"glyph-run");
                    add_usize(&mut output, run.text_range().start);
                    add_usize(&mut output, run.text_range().end);
                    output.update([run.bidi_level()]);
                    output.update(format!("{:?}", run.source()).as_bytes());
                    output.update(format!("{:?}", run.orientation()).as_bytes());
                    for glyph in run.glyphs() {
                        add_usize(&mut output, glyph.id as usize);
                        add_usize(&mut output, glyph.cluster as usize);
                        add_f32(&mut output, glyph.inline_position);
                        add_f32(&mut output, glyph.block_offset);
                        add_f32(&mut output, glyph.advance);
                    }
                }
                other => output.update(format!("{other:?}").as_bytes()),
            }
        }
    }

    let mut warnings: Vec<_> = paragraph
        .warnings()
        .iter()
        .map(|warning| format!("{:?}:{}", warning.kind, warning.message))
        .collect();
    warnings.extend(
        context
            .take_warnings()
            .into_iter()
            .map(|warning| format!("{:?}:{}", warning.kind, warning.message)),
    );
    let mut warning_hash = Sha256::new();
    for warning in &warnings {
        warning_hash.update((warning.len() as u64).to_le_bytes());
        warning_hash.update(warning.as_bytes());
    }
    Snapshot {
        output_sha256: format!("{:x}", output.finalize()),
        warning_sha256: format!("{:x}", warning_hash.finalize()),
        warnings,
    }
}

fn sample(case: &Case, fonts: &shodo_fixtures::FixtureFonts, mode: &str, index: usize) -> Value {
    let limits = Limits::default();
    let input = builder(case, &limits);
    let mut context = LayoutContext::new();
    let (paragraph, measurement) = match mode {
        "time" => {
            let start = Instant::now();
            let paragraph = black_box(input)
                .build(&mut context, &fonts.collection)
                .unwrap();
            let elapsed = start.elapsed().as_nanos() as u64;
            black_box(&paragraph);
            (paragraph, json!({"ns": elapsed}))
        }
        "alloc" => {
            #[cfg(feature = "allocation-counting")]
            {
                let scope = ALLOC.begin().unwrap();
                let paragraph = black_box(input)
                    .build(&mut context, &fonts.collection)
                    .unwrap();
                let counts = scope.finish();
                black_box(&paragraph);
                (paragraph, json!({"allocation": counts}))
            }
            #[cfg(not(feature = "allocation-counting"))]
            panic!("alloc mode requires allocation-counting");
        }
        _ => panic!("mode must be time or alloc"),
    };
    let output = snapshot(&paragraph, &mut context);
    json!({
        "schema": 1,
        "mode": mode,
        "fixture": case.name,
        "text_bytes": case.text.len(),
        "sample": index,
        "measurement": measurement,
        "output": {
            "sha256": output.output_sha256,
            "warning_sha256": output.warning_sha256,
            "warnings": output.warnings,
        }
    })
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "time".into());
    let expected_mode = if cfg!(feature = "allocation-counting") {
        "alloc"
    } else {
        "time"
    };
    assert_eq!(
        mode, expected_mode,
        "use separate timing and allocation builds"
    );
    let samples = if mode == "time" { 21 } else { 9 };
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    for case in cases() {
        let mut expected_output: Option<Value> = None;
        for _ in 0..WARMUP {
            let row = sample(&case, &fonts, &mode, usize::MAX);
            if let Some(expected) = &expected_output {
                assert_eq!(&row["output"], expected, "unstable warmup: {}", case.name);
            } else {
                expected_output = Some(row["output"].clone());
            }
        }
        for index in 0..samples {
            let row = sample(&case, &fonts, &mode, index);
            assert_eq!(
                row["output"],
                expected_output.as_ref().unwrap().clone(),
                "unstable sample: {}",
                case.name
            );
            println!("{}", serde_json::to_string(&row).unwrap());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_neutral_inheritance_fixture_keeps_line_breaks() {
        let case = cases()
            .into_iter()
            .find(|case| case.name == "plaintext-neutral-inheritance-controls")
            .unwrap();
        assert!(!case.text.contains('\u{2028}'));
        assert!(case.text.contains('\u{2066}') && case.text.contains('\u{2069}'));

        let limits = Limits::default();
        let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
        let mut context = LayoutContext::new();
        let paragraph = builder(&case, &limits)
            .build(&mut context, &fonts.collection)
            .unwrap();
        let lines = paragraph.break_all(
            &mut context,
            &LineOptions::default(),
            1_000_000.0,
            &AtomicSizes::EMPTY,
        );

        assert_eq!(lines.len(), 24);
        for line_index in (1..lines.len()).step_by(3) {
            let neutral_range = lines[line_index].text_range();
            assert_eq!(&paragraph.text()[neutral_range], "123\n");
        }
    }
}
