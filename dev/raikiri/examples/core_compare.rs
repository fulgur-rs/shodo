//! Fixed-font core comparison. Output snapshots are outside both timed phases.
use fontique::{Blob, Collection, CollectionOptions, FontInfoOverride};
use parley::style::{FontFamily, StyleProperty};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::{AtomicSizes, Fragment, LayoutContext, Line};
use shodo_fixtures::{FONTS, FixtureCase, cases, font, load_fonts};
use std::borrow::Cow;
use std::hint::black_box;
use std::time::Instant;

const WARMUPS: usize = 3;
const SAMPLES: usize = 21;

fn parley_fonts() -> parley::FontContext {
    let mut fonts = parley::FontContext {
        collection: Collection::new(CollectionOptions {
            system_fonts: false,
            shared: false,
        }),
        source_cache: Default::default(),
    };
    for face in FONTS {
        let registered = fonts.collection.register_fonts(
            Blob::new(std::sync::Arc::new(face.bytes.to_vec())),
            Some(FontInfoOverride {
                family_name: Some(face.family),
                ..Default::default()
            }),
        );
        assert_eq!(registered.len(), 1, "fixture must register one family");
    }
    fonts
}

fn parley_build(
    case: &FixtureCase,
    cx: &mut parley::LayoutContext<()>,
    fonts: &mut parley::FontContext,
) -> parley::Layout<()> {
    // List all three fixed families after the explicit chain. System discovery
    // is disabled; snapshot font hashes expose any selection difference.
    let families = case
        .font_ids
        .iter()
        .map(|id| font(id).expect("fixture font").family)
        .chain(FONTS.iter().map(|face| face.family))
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let mut builder = cx.ranged_builder(fonts, &case.text, 1.0, false);
    builder.push_default(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(
        families,
    ))));
    builder.push_default(StyleProperty::FontSize(case.font_size));
    builder.push_default(StyleProperty::Locale(
        case.lang
            .as_deref()
            .map(|lang| lang.parse().expect("fixture locale")),
    ));
    builder.build(&case.text)
}

fn shodo_output(lines: &[Line], text: &str, width: f32) -> Value {
    let output = json!(
        lines
            .iter()
            .map(|line| {
                let runs: Vec<_> = line
                    .fragments()
                    .filter_map(|fragment| {
                        let Fragment::GlyphRun(run) = fragment else {
                            return None;
                        };
                        let data = run.font_data().expect("fixed font data");
                        let glyphs: Vec<_> = run
                            .glyphs()
                            .enumerate()
                            .map(|(index, g)| {
                                let (x, y) = run
                                    .physical_origin(
                                        index,
                                        shodo::geometry::PhysicalSize { width, height: 0.0 },
                                    )
                                    .expect("glyph origin");
                                json!([
                                    g.id,
                                    x,
                                    y - line.block_offset() - line.metrics().baseline,
                                    g.advance
                                ])
                            })
                            .collect();
                        Some(
                            json!({"font_sha256": format!("{:x}", Sha256::digest(data.data.data())),
                "font_index": data.index, "font_size": run.font_size(), "glyphs": glyphs}),
                        )
                    })
                    .collect();
                json!({"range": line.text_range(), "advance": line.inline_size(),
            "height": line.block_size(), "baseline": line.metrics().baseline, "runs": runs})
            })
            .collect::<Vec<_>>()
    );
    json!({"text": text, "lines": output})
}

fn parley_output(layout: &parley::Layout<()>, text: &str) -> Value {
    let mut line_top = 0.0f64;
    let output = json!(
        layout
            .lines()
            .map(|line| {
                let baseline = line.metrics().baseline - line_top as f32;
                line_top += f64::from(line.metrics().line_height);
                let runs: Vec<_> = line.items().filter_map(|item| {
            let parley::PositionedLayoutItem::GlyphRun(view) = item else { return None };
            let run = view.run();
            let glyphs: Vec<_> = view.positioned_glyphs().map(|g|
                json!([g.id, g.x, g.y - view.baseline(), g.advance])
            ).collect();
            Some(json!({"font_sha256": format!("{:x}", Sha256::digest(run.font().data.data())),
                "font_index": run.font().index, "font_size": run.font_size(), "glyphs": glyphs}))
        }).collect();
                json!({"range": line.text_range(), "advance": line.metrics().advance,
            "height": line.metrics().line_height, "baseline": baseline, "runs": runs})
            })
            .collect::<Vec<_>>()
    );
    json!({"text": text, "lines": output})
}

fn oracle(shodo: &Value, parley: &Value) -> Value {
    let ranges = |output: &Value| {
        output["lines"]
            .as_array()
            .expect("lines")
            .iter()
            .map(|line| line["range"].clone())
            .collect::<Vec<_>>()
    };
    json!({
        "text_coordinate_spaces_equal": shodo["text"] == parley["text"],
        "numeric_line_ranges_equal": ranges(shodo) == ranges(parley),
        "line_ranges_equal": if shodo["text"] == parley["text"] {
            json!(ranges(shodo) == ranges(parley))
        } else { Value::Null },
        "exact_snapshot_equal": shodo == parley,
        // Even equal snapshots omit paint, hit-testing and cluster contracts.
        "equal_output_speedup_claim_eligible": false,
        "shodo": shodo,
        "parley": parley,
    })
}

fn statistics(mut samples: Vec<u64>) -> Value {
    let raw = samples.clone();
    samples.sort_unstable();
    json!({"samples_ns": raw, "median_ns": samples[samples.len() / 2],
        "p90_ns": samples[(samples.len() - 1) * 9 / 10]})
}

fn measure(case: &FixtureCase) -> Result<Value, Box<dyn std::error::Error>> {
    let limits = Limits::default();
    let fonts = load_fonts(&limits)?;
    let mut shodo_cx = LayoutContext::new();
    let mut parley_cx = parley::LayoutContext::new();
    let mut parley_fonts = parley_fonts();
    let options = shodo::style::LineOptions::default();
    let atomic = AtomicSizes::new();
    let mut builds = [Vec::new(), Vec::new()];
    let mut breaks = [Vec::new(), Vec::new()];
    let mut expected: [Option<Value>; 2] = [None, None];
    let mut parley_rtl = false;
    for iteration in 0..WARMUPS + SAMPLES {
        // Alternate engine order to avoid always giving one engine first use.
        for engine in [iteration % 2, 1 - iteration % 2] {
            let (build_ns, break_ns, output) = if engine == 0 {
                let start = Instant::now();
                let paragraph = case.build(&mut shodo_cx, &fonts, &limits)?;
                let build_ns = start.elapsed().as_nanos() as u64;
                black_box(&paragraph);
                let start = Instant::now();
                let lines = paragraph.break_all(&mut shodo_cx, &options, case.width, &atomic);
                let break_ns = start.elapsed().as_nanos() as u64;
                black_box(&lines);
                (
                    build_ns,
                    break_ns,
                    shodo_output(&lines, paragraph.text(), case.width),
                )
            } else {
                let start = Instant::now();
                let mut layout = parley_build(case, &mut parley_cx, &mut parley_fonts);
                let build_ns = start.elapsed().as_nanos() as u64;
                black_box(&layout);
                parley_rtl = layout.is_rtl();
                let start = Instant::now();
                layout.break_all_lines(Some(case.width));
                layout.align(
                    parley::Alignment::Start,
                    parley::AlignmentOptions::default(),
                );
                let break_ns = start.elapsed().as_nanos() as u64;
                black_box(&layout);
                (build_ns, break_ns, parley_output(&layout, &case.text))
            };
            if let Some(previous) = &expected[engine] {
                assert_eq!(previous, &output, "output changed across repeated calls");
            } else {
                expected[engine] = Some(output);
            }
            if iteration >= WARMUPS {
                builds[engine].push(build_ns);
                breaks[engine].push(break_ns);
            }
        }
    }
    let direction_matches = parley_rtl == (case.direction == shodo::geometry::Direction::Rtl);
    Ok(json!({"id": case.id, "input": {
        "text": case.text, "font_ids": case.font_ids, "lang": case.lang,
        "direction": if case.direction == shodo::geometry::Direction::Rtl { "rtl" } else { "ltr" },
        "font_size": case.font_size, "width": case.width},
        "parley_base_direction": if parley_rtl { "rtl" } else { "ltr" },
        "base_direction_matches": direction_matches,
        "shodo": {"build": statistics(builds[0].clone()), "all_lines": statistics(breaks[0].clone())},
        "parley": {"build": statistics(builds[1].clone()), "all_lines": statistics(breaks[1].clone())},
        "oracle": oracle(expected[0].as_ref().unwrap(), expected[1].as_ref().unwrap())}))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).ok_or("expected output JSON path")?;
    let results = cases().iter().map(measure).collect::<Result<Vec<_>, _>>()?;
    let report = json!({"schema": 1, "warmups": WARMUPS, "samples": SAMPLES,
        "scope": "Warm reused contexts; fixed fonts registered outside timing. Build includes style construction, analysis, shaping and paragraph/layout creation. all_lines includes breaking and output construction from a fresh built paragraph/layout. Snapshot, validation, JSON and destruction excluded. No DOM/CSS/box layout/paint; no cold or RSS claims.",
        "language": "Fixture lang set on both engines; Parley Locale parsed as fontique::Language.",
        "direction": "shodo explicit fixture direction; Parley public builder auto-detects; actual base direction checked per case.",
        "quantization": "shodo caller values round to 1/64px, derived line height rounds upward; Parley quantize=false uses floating coordinates. Exact geometry is not assumed equal.",
        "fallback": "System discovery disabled. shodo fixture explicit family chain plus Latn/Hani/Arab script fallbacks; Parley explicit fixture chain followed by all three fixture families. Actual output font hashes retained; fallback algorithms may differ.",
        "oracle_scope": "Each engine's reference text and byte line ranges. Semantic range equality only when reference texts match. Baselines are line-local; glyph origins use physical x and baseline-relative y (shodo physical_origin converts RTL). Parley Start alignment is inside all_lines timing. Line advance uses native semantics: shodo excludes hanging trailing spaces, Parley includes them. Visual runs retain font SHA/index/size and glyph ID/x/y/advance. Exact comparison; no tolerance hides differences. Omits cluster mapping, paint and hit-testing: no equal-output speedup claim.",
        "fonts": FONTS.iter().map(|face| json!({"id": face.id, "sha256": format!("{:x}", Sha256::digest(face.bytes))})).collect::<Vec<_>>(),
        "cases": results});
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_ranges_do_not_imply_matching_geometry() {
        let a = json!({"text": "abc", "lines": [{"range": {"start": 0, "end": 3}, "advance": 12}]});
        let b = json!({"text": "abc", "lines": [{"range": {"start": 0, "end": 3}, "advance": 13}]});
        let result = oracle(&a, &b);
        assert_eq!(result["line_ranges_equal"], true);
        assert_eq!(result["exact_snapshot_equal"], false);
        assert_eq!(result["equal_output_speedup_claim_eligible"], false);
    }

    #[test]
    fn changed_ranges_are_recorded() {
        let a = json!({"text": "abc", "lines": [{"range": {"start": 0, "end": 3}}]});
        let b = json!({"text": "abc", "lines": [{"range": {"start": 0, "end": 2}}]});
        assert_eq!(oracle(&a, &b)["line_ranges_equal"], false);
    }

    #[test]
    fn different_text_coordinates_do_not_claim_range_equivalence() {
        let a = json!({"text": "a b", "lines": [{"range": {"start": 0, "end": 3}}]});
        let b = json!({"text": "a  b", "lines": [{"range": {"start": 0, "end": 3}}]});
        let result = oracle(&a, &b);
        assert_eq!(result["numeric_line_ranges_equal"], true);
        assert_eq!(result["text_coordinate_spaces_equal"], false);
        assert!(result["line_ranges_equal"].is_null());
    }
}
