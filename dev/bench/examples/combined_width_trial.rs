//! Paired build probe for vertical-combine width-feature trial shaping.
//! Run without features for timing, then with `allocation-counting` and `alloc`.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::geometry::WritingMode;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{
    FontFamily, FontFeature, InlineStyle, LineOptions, ParagraphStyle, TextCombineUpright,
};
use shodo::{AtomicSizes, Fragment, LayoutContext, Paragraph, ParagraphBuilder};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const GROUPS: usize = 64;
const WARMUP: usize = 3;
const FONT_FAMILY: &str = "Shodo Fixture CJK";

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    text: &'static str,
    groups: usize,
    writing_mode: WritingMode,
    derived_feature: Option<[u8; 4]>,
    disable_hwid: bool,
}

fn cases() -> [Case; 6] {
    [
        Case {
            name: "hwid-two-complete",
            text: "12",
            groups: GROUPS,
            writing_mode: WritingMode::VerticalRl,
            derived_feature: None,
            disable_hwid: false,
        },
        Case {
            name: "twid-three-complete",
            text: "123",
            groups: GROUPS,
            writing_mode: WritingMode::VerticalRl,
            derived_feature: Some(*b"twid"),
            disable_hwid: false,
        },
        Case {
            name: "qwid-four-complete",
            text: "1234",
            groups: GROUPS,
            writing_mode: WritingMode::VerticalRl,
            derived_feature: Some(*b"qwid"),
            disable_hwid: false,
        },
        Case {
            name: "hwid-two-partial",
            text: "1水",
            groups: GROUPS,
            writing_mode: WritingMode::VerticalRl,
            derived_feature: None,
            disable_hwid: false,
        },
        Case {
            name: "hwid-two-explicitly-disabled",
            text: "12",
            groups: GROUPS,
            writing_mode: WritingMode::VerticalRl,
            derived_feature: None,
            disable_hwid: true,
        },
        Case {
            name: "horizontal-control",
            text: "12",
            groups: GROUPS,
            writing_mode: WritingMode::HorizontalTb,
            derived_feature: None,
            disable_hwid: false,
        },
    ]
}

fn fixture_fonts(case: Case, limits: &Limits) -> FontCollection {
    let mut bytes = shodo_fixtures::font("cjk")
        .expect("pinned CJK font")
        .bytes
        .to_vec();
    if let Some(feature) = case.derived_feature {
        replace_hwid_feature_tag(&mut bytes, feature);
    }
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            bytes,
            0,
            FontFaceDescriptor {
                family: FONT_FAMILY.into(),
                ..Default::default()
            },
        )
        .expect("register pinned CJK fixture");
    fonts
}

fn replace_hwid_feature_tag(bytes: &mut [u8], replacement: [u8; 4]) {
    let table_count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    let gsub = (0..table_count)
        .find_map(|index| {
            let record = 12 + index * 16;
            (&bytes[record..record + 4] == b"GSUB").then(|| {
                u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize
            })
        })
        .expect("fixture GSUB table");
    let feature_list =
        gsub + u16::from_be_bytes(bytes[gsub + 6..gsub + 8].try_into().unwrap()) as usize;
    let feature_count =
        u16::from_be_bytes(bytes[feature_list..feature_list + 2].try_into().unwrap()) as usize;
    let record = (0..feature_count)
        .map(|index| feature_list + 2 + index * 6)
        .find(|record| &bytes[*record..*record + 4] == b"hwid")
        .expect("fixture `hwid` feature");
    bytes[record..record + 4].copy_from_slice(&replacement);
}

fn builder(case: Case, limits: &Limits) -> ParagraphBuilder {
    let root = InlineStyle {
        font_families: vec![FontFamily::Named(FONT_FAMILY.into())],
        ..Default::default()
    };
    let paragraph_style = ParagraphStyle {
        root: root.clone(),
        writing_mode: case.writing_mode,
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, limits);
    for index in 0..case.groups {
        let mut combined = root.clone();
        combined.text_combine_upright = TextCombineUpright::All;
        if case.disable_hwid {
            combined.font_features.push(FontFeature {
                tag: *b"hwid",
                value: 0,
            });
        }
        let node = NodeId(index as u64 + 1);
        builder
            .open_inline(node, &combined, InlineEdges::default())
            .push_text(TextSource::Generated { node }, case.text)
            .close_inline();
    }
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
    let lines = paragraph.break_all(
        context,
        &LineOptions::default(),
        1_000_000.0,
        &AtomicSizes::EMPTY,
    );
    let mut output = Sha256::new();
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
                    add_usize(&mut output, run.style_index() as usize);
                    output.update([run.bidi_level()]);
                    output.update(format!("{:?}", run.source()).as_bytes());
                    output.update(format!("{:?}", run.orientation()).as_bytes());
                    for (index, glyph) in run.glyphs().enumerate() {
                        add_usize(&mut output, glyph.id as usize);
                        add_usize(&mut output, glyph.cluster as usize);
                        add_usize(&mut output, index);
                        for value in [glyph.inline_position, glyph.block_offset, glyph.advance] {
                            add_f32(&mut output, value);
                        }
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

fn sample(case: Case, fonts: &FontCollection, mode: &str, index: usize) -> Value {
    let limits = Limits::default();
    let builder = builder(case, &limits);
    let mut context = LayoutContext::new();
    let (paragraph, measurement) = match mode {
        "time" => {
            let start = Instant::now();
            let paragraph = black_box(builder).build(&mut context, fonts).unwrap();
            let elapsed = start.elapsed().as_nanos() as u64;
            black_box(&paragraph);
            (paragraph, json!({"ns": elapsed}))
        }
        "alloc" => {
            #[cfg(feature = "allocation-counting")]
            {
                let scope = ALLOC.begin().unwrap();
                let paragraph = black_box(builder).build(&mut context, fonts).unwrap();
                let allocation = scope.finish();
                black_box(&paragraph);
                (paragraph, json!({"allocation": allocation}))
            }
            #[cfg(not(feature = "allocation-counting"))]
            panic!("alloc mode requires --features allocation-counting");
        }
        _ => panic!("mode must be time or alloc"),
    };
    let output = snapshot(&paragraph, &mut context);
    json!({
        "schema": 1,
        "mode": mode,
        "fixture": case.name,
        "font_sha256": shodo_fixtures::font("cjk").unwrap().sha256,
        "derived_feature": case.derived_feature.map(|tag| String::from_utf8_lossy(&tag).to_string()),
        "groups": case.groups,
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
        "use separate time and allocation builds"
    );
    let samples = if mode == "time" { 21 } else { 9 };
    let limits = Limits::default();
    let mut outputs = BTreeMap::new();
    for case in cases() {
        let fonts = fixture_fonts(case, &limits);
        for _ in 0..WARMUP {
            let row = sample(case, &fonts, &mode, usize::MAX);
            let output = row["output"].clone();
            if let Some(expected) = outputs.get(case.name) {
                assert_eq!(&output, expected, "unstable warmup output: {}", case.name);
            } else {
                outputs.insert(case.name, output);
            }
        }
        for index in 0..samples {
            let row = sample(case, &fonts, &mode, index);
            assert_eq!(
                row["output"], outputs[case.name],
                "unstable sample output: {}",
                case.name
            );
            println!("{}", serde_json::to_string(&row).unwrap());
        }
    }
}
