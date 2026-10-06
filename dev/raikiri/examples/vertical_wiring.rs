//! Vertical writing-mode wiring check: raikiri cascade → adapter → shodo.
//!
//! The four `text-autospace-vertical-*` WPT documents (vendored unmodified in
//! `inputs/vertical/`) are laid out through the raikiri cascade and shodo's
//! existing vertical API. The WPT reference is the oracle: a test document and
//! its reference must produce the same lines and glyph geometry. This is
//! separate from the unmerged raikiri integration spikes.
use raikiri_html::{ParseOptions, parse_html};
use raikiri_traits::{Dom, NodeId as DomId};
use shodo::font::FontCollection;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, GlyphOrientation, LayoutContext, ParagraphBuilder};

#[path = "../adapter/ch.rs"]
#[allow(dead_code)]
mod ch_adapter;
#[path = "support/source_fonts.rs"]
#[allow(dead_code)]
mod fonts;
#[path = "../adapter/vertical.rs"]
mod vertical_adapter;

/// One accepted line: axis extents and the glyph runs in logical order.
#[derive(Debug, PartialEq)]
struct LineSummary {
    inline_size: f32,
    block_size: f32,
    runs: Vec<RunSummary>,
}

#[derive(Debug, PartialEq)]
struct RunSummary {
    font: shodo::font::FontId,
    orientation: GlyphOrientation,
    font_size: f32,
    /// (glyph id, inline position, block offset, advance)
    glyphs: Vec<(u32, f32, f32, f32)>,
}

const FIXTURE_FAMILY: &str = "Shodo Fixture CJK";

fn inline_style(
    values: &raikiri_style::ComputedValues,
    families: &[FontFamily],
) -> Result<InlineStyle, String> {
    Ok(InlineStyle {
        // The documents name no font, so the caller supplies the family: the
        // fixture face for CI, or the pinned registry's generic serif.
        font_families: families.to_vec(),
        font_size: values.font_size.0,
        font_weight: values.font_weight,
        font_style: ch_adapter::font_style(values.font_style)?,
        lang: Some("ja".into()),
        text_orientation: vertical_adapter::text_orientation(values.text_orientation)?,
        text_combine_upright: vertical_adapter::text_combine_upright(values.text_combine_upright)?,
        text_autospace: vertical_adapter::text_autospace(values.text_autospace)?,
        ..Default::default()
    })
}

fn add_children(
    doc: &raikiri_html::HtmlDocument,
    builder: &mut ParagraphBuilder,
    parent: usize,
    families: &[FontFamily],
) -> Result<(), String> {
    for id in doc.dom().child_ids(DomId(parent as u64)) {
        let node = doc.dom().get_node(id.0 as usize).unwrap();
        if let Some(text) = node.text_content() {
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(id.0),
                    offset: 0,
                },
                text,
            );
        } else {
            let style = inline_style(&doc.cascade().computed[id.0 as usize], families)?;
            builder.open_inline(NodeId(id.0), &style, InlineEdges::default());
            add_children(doc, builder, id.0 as usize, families)?;
            builder.close_inline();
        }
    }
    Ok(())
}

fn lines(
    html: &str,
    fonts: &FontCollection,
    families: &[FontFamily],
) -> Result<Vec<LineSummary>, String> {
    let doc = parse_html(
        html.as_bytes(),
        &ParseOptions {
            extra_stylesheets: &[],
            network: None,
            base_url: None,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    let container = (0..doc.dom().node_count())
        .find(|&id| doc.dom().get_node(id).unwrap().attribute("id") == Some("container"))
        .ok_or("missing #container")?;
    // `cssom_writing_mode` keeps vertical values; `writing_mode` is
    // normalized to horizontal for the renderer and would drop them.
    let mode =
        vertical_adapter::writing_mode(doc.cascade().computed[container].cssom_writing_mode)?;
    let limits = Limits::default();
    let mut summaries = Vec::new();
    for block in doc.dom().child_ids(DomId(container as u64)) {
        let node = doc.dom().get_node(block.0 as usize).unwrap();
        if node.text_content().is_some() {
            continue; // inter-block whitespace
        }
        let root = inline_style(&doc.cascade().computed[block.0 as usize], families)?;
        let style = ParagraphStyle {
            writing_mode: mode,
            root,
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &limits);
        add_children(&doc, &mut builder, block.0 as usize, families)?;
        let mut context = LayoutContext::new();
        let paragraph = builder
            .build(&mut context, fonts)
            .map_err(|e| format!("{e:?}"))?;
        let accepted = paragraph.break_all(
            &mut context,
            &LineOptions::default(),
            10000.0,
            &AtomicSizes::EMPTY,
        );
        for line in accepted {
            let runs = line
                .fragments()
                .filter_map(|f| match f {
                    Fragment::GlyphRun(run) => Some(RunSummary {
                        font: run.font(),
                        orientation: run.orientation(),
                        font_size: run.font_size(),
                        glyphs: run
                            .glyphs()
                            .map(|g| (g.id, g.inline_position, g.block_offset, g.advance))
                            .collect(),
                    }),
                    _ => None,
                })
                .collect();
            summaries.push(LineSummary {
                inline_size: line.inline_size(),
                block_size: line.block_size(),
                runs,
            });
        }
    }
    Ok(summaries)
}

fn fixture_lines(html: &str, fonts: &FontCollection) -> Result<Vec<LineSummary>, String> {
    lines(html, fonts, &[FontFamily::Named(FIXTURE_FAMILY.into())])
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0 / 64.0
}

/// Compares two documents' lines with a 1/64px tolerance.
fn same(a: &[LineSummary], b: &[LineSummary]) -> Result<(), String> {
    if a.len() != b.len() {
        return Err(format!("line count {} != {}", a.len(), b.len()));
    }
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if !close(x.inline_size, y.inline_size) || !close(x.block_size, y.block_size) {
            return Err(format!("line {i} extents differ: {x:?} vs {y:?}"));
        }
        if x.runs.len() != y.runs.len() {
            return Err(format!("line {i} run count differs: {x:?} vs {y:?}"));
        }
        for (rx, ry) in x.runs.iter().zip(&y.runs) {
            let g_ok = rx.glyphs.len() == ry.glyphs.len()
                && rx.glyphs.iter().zip(&ry.glyphs).all(|(p, q)| {
                    p.0 == q.0 && close(p.1, q.1) && close(p.2, q.2) && close(p.3, q.3)
                });
            if rx.font != ry.font
                || rx.orientation != ry.orientation
                || !close(rx.font_size, ry.font_size)
                || !g_ok
            {
                return Err(format!("line {i} glyphs differ: {rx:?} vs {ry:?}"));
            }
        }
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).map_err(|e| format!("{e:?}"))?;
    for name in [
        "text-autospace-vertical-combine-001",
        "text-autospace-vertical-upright-001",
    ] {
        let read = |suffix: &str| {
            std::fs::read_to_string(format!(
                "{}/inputs/vertical/{name}{suffix}.html",
                env!("CARGO_MANIFEST_DIR")
            ))
            .map_err(|e| e.to_string())
        };
        let test = fixture_lines(&read("")?, &fonts.collection)?;
        let reference = fixture_lines(&read("-ref")?, &fonts.collection)?;
        let sizes = |v: &[LineSummary]| v.iter().map(|l| l.inline_size).collect::<Vec<_>>();
        println!(
            "{name}: test {:?} reference {:?}, matches reference: {:?}",
            sizes(&test),
            sizes(&reference),
            same(&test, &reference)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shodo_fixtures::load_fonts;

    fn doc(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/inputs/vertical/{name}.html",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn combine_test_matches_reference() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let test = fixture_lines(
            &doc("text-autospace-vertical-combine-001"),
            &fonts.collection,
        )
        .unwrap();
        let reference = fixture_lines(
            &doc("text-autospace-vertical-combine-001-ref"),
            &fonts.collection,
        )
        .unwrap();
        assert_eq!(test.len(), 2);
        same(&test, &reference).unwrap();
    }

    // shodo-39u: upright vertical letters and digits are excluded from
    // autospace, so the test document matches its no-autospace reference.
    #[test]
    fn upright_test_matches_reference() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let test = fixture_lines(
            &doc("text-autospace-vertical-upright-001"),
            &fonts.collection,
        )
        .unwrap();
        let reference = fixture_lines(
            &doc("text-autospace-vertical-upright-001-ref"),
            &fonts.collection,
        )
        .unwrap();
        assert_eq!(test.len(), 4);
        same(&test, &reference).unwrap();
    }

    #[test]
    fn vertical_is_not_dropped_and_upright_reaches_glyph_runs() {
        let fonts = load_fonts(&Default::default()).unwrap();
        // The no-autospace reference isolates orientation and axis from
        // spacing behavior.
        let reference = fixture_lines(
            &doc("text-autospace-vertical-upright-001-ref"),
            &fonts.collection,
        )
        .unwrap();
        // In upright vertical text every glyph is upright and stacks along the
        // vertical inline axis: 3 cells of 20px.
        for line in &reference {
            assert!(
                line.runs
                    .iter()
                    .all(|r| r.orientation == GlyphOrientation::Upright),
                "{line:?}"
            );
            assert!(close(line.inline_size, 60.0), "{line:?}");
        }
        // The same document forced horizontal must differ, proving the
        // vertical mode reached shodo.
        let horizontal =
            doc("text-autospace-vertical-upright-001-ref").replace("vertical-rl", "horizontal-tb");
        let horizontal = fixture_lines(&horizontal, &fonts.collection).unwrap();
        assert_eq!(horizontal.len(), 4);
        assert!(!horizontal[0].runs.is_empty());
        assert!(
            horizontal[0]
                .runs
                .iter()
                .all(|r| r.orientation == GlyphOrientation::Horizontal)
        );
    }

    #[test]
    fn tcy_span_occupies_one_em() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let combine = fixture_lines(
            &doc("text-autospace-vertical-combine-001"),
            &fonts.collection,
        )
        .unwrap();
        assert_eq!(combine.len(), 2);
        for line in &combine {
            assert!(
                line.runs
                    .iter()
                    .any(|r| r.orientation == GlyphOrientation::Combined),
                "TCY span reached shodo as ordinary text: {line:?}"
            );
            // 国 + one combined em + 国 at 20px.
            assert!(close(line.inline_size, 60.0), "{line:?}");
        }
    }

    /// The same oracle with the original 88-font registry and the documents'
    /// initial generic family. Needs a WPT checkout: set `SHODO_WPT_ROOT`
    /// (CI has none, so the test is skipped there).
    #[test]
    fn pinned_registry_matches_references_for_combine_and_upright() {
        let Some(root) = std::env::var_os("SHODO_WPT_ROOT") else {
            eprintln!("skipped: SHODO_WPT_ROOT is not set");
            return;
        };
        let registry = fonts::load(
            &std::path::Path::new(&root).join("fonts"),
            &Limits::default(),
        )
        .unwrap();
        let family = [FontFamily::Generic(shodo::style::GenericFamily::Serif)];
        let run = |name: &str| lines(&doc(name), &registry.collection, &family).unwrap();
        let combine = run("text-autospace-vertical-combine-001");
        same(&combine, &run("text-autospace-vertical-combine-001-ref")).unwrap();
        let upright = run("text-autospace-vertical-upright-001");
        let upright_ref = run("text-autospace-vertical-upright-001-ref");
        // Every character must come from a real glyph, not .notdef.
        for line in combine.iter().chain(&upright) {
            for r in &line.runs {
                assert!(
                    r.glyphs.iter().all(|g| g.0 != 0),
                    ".notdef glyph in {line:?}"
                );
            }
        }
        same(&upright, &upright_ref).unwrap();
    }

    #[test]
    fn autospace_auto_and_explicit_sets_reach_shodo() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let original = doc("text-autospace-vertical-upright-001");
        let normal = fixture_lines(&original, &fonts.collection).unwrap();
        for value in [
            "auto",
            "ideograph-alpha",
            "ideograph-numeric",
            "punctuation",
            "ideograph-alpha ideograph-numeric insert",
        ] {
            let html = original.replace(
                "text-autospace: normal",
                &format!("text-autospace: {value}"),
            );
            same(&fixture_lines(&html, &fonts.collection).unwrap(), &normal).unwrap();
        }
        let horizontal = original.replace("vertical-rl", "horizontal-tb");
        let plain = fixture_lines(
            &horizontal.replace("text-autospace: normal", "text-autospace: no-autospace"),
            &fonts.collection,
        )
        .unwrap();
        for (value, alpha, numeric) in [
            ("auto", true, true),
            ("ideograph-alpha", true, false),
            ("ideograph-numeric", false, true),
            ("punctuation", false, false),
        ] {
            let selected = fixture_lines(
                &horizontal.replace(
                    "text-autospace: normal",
                    &format!("text-autospace: {value}"),
                ),
                &fonts.collection,
            )
            .unwrap();
            assert_eq!(selected.len(), plain.len());
            for (index, (actual, natural)) in selected.iter().zip(&plain).enumerate() {
                let expected_gap = if (index % 2 == 0 && alpha) || (index % 2 == 1 && numeric) {
                    5.0
                } else {
                    0.0
                };
                assert!(
                    close(actual.inline_size, natural.inline_size + expected_gap),
                    "value {value}, line {index}: {actual:?} vs {natural:?}"
                );
            }
        }
        let replace = original.replace(
            "text-autospace: normal",
            "text-autospace: ideograph-alpha replace",
        );
        assert!(
            fixture_lines(&replace, &fonts.collection)
                .unwrap_err()
                .contains("replace")
        );
    }
}
