//! Vertical writing-mode wiring check: raikiri cascade → adapter → shodo.
//!
//! The four `text-autospace-vertical-*` WPT documents (vendored unmodified in
//! `data/vertical/`) are laid out through the raikiri cascade and shodo's
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
    orientation: GlyphOrientation,
    font_size: f32,
    /// (glyph id, inline position, block offset, advance)
    glyphs: Vec<(u32, f32, f32, f32)>,
}

const FIXTURE_FAMILY: &str = "Shodo Fixture CJK";

fn inline_style(values: &raikiri_style::ComputedValues) -> Result<InlineStyle, String> {
    Ok(InlineStyle {
        // The documents name no font; the fixture face is supplied by the
        // caller, exactly as `ch_units` supplies named fixture families.
        font_families: vec![FontFamily::Named(FIXTURE_FAMILY.into())],
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
            let style = inline_style(&doc.cascade().computed[id.0 as usize])?;
            builder.open_inline(NodeId(id.0), &style, InlineEdges::default());
            add_children(doc, builder, id.0 as usize)?;
            builder.close_inline();
        }
    }
    Ok(())
}

fn lines(html: &str, fonts: &FontCollection) -> Result<Vec<LineSummary>, String> {
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
        let root = inline_style(&doc.cascade().computed[block.0 as usize])?;
        let style = ParagraphStyle {
            writing_mode: mode,
            root,
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &limits);
        add_children(&doc, &mut builder, block.0 as usize)?;
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
            if rx.orientation != ry.orientation || !close(rx.font_size, ry.font_size) || !g_ok {
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
                "{}/data/vertical/{name}{suffix}.html",
                env!("CARGO_MANIFEST_DIR")
            ))
            .map_err(|e| e.to_string())
        };
        let test = lines(&read("")?, &fonts.collection)?;
        let reference = lines(&read("-ref")?, &fonts.collection)?;
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
            "{}/data/vertical/{name}.html",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn combine_test_matches_reference() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let test = lines(
            &doc("text-autospace-vertical-combine-001"),
            &fonts.collection,
        )
        .unwrap();
        let reference = lines(
            &doc("text-autospace-vertical-combine-001-ref"),
            &fonts.collection,
        )
        .unwrap();
        assert_eq!(test.len(), 2);
        same(&test, &reference).unwrap();
    }

    // Known shodo core difference (shodo-39u): autospace is applied to upright
    // vertical text. Kept as the oracle; remove `ignore` when it is fixed.
    #[test]
    #[ignore = "shodo-39u: text-autospace applied to upright vertical text"]
    fn upright_test_matches_reference() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let test = lines(
            &doc("text-autospace-vertical-upright-001"),
            &fonts.collection,
        )
        .unwrap();
        let reference = lines(
            &doc("text-autospace-vertical-upright-001-ref"),
            &fonts.collection,
        )
        .unwrap();
        assert_eq!(test.len(), 4);
        same(&test, &reference).unwrap();
    }

    #[test]
    fn upright_autospace_divergence_is_pinned() {
        // Pins the current shodo-39u behavior: the test document is 5px longer
        // per line than its no-autospace reference (2.5px on each side of the
        // upright X/1). This test must be deleted when that issue is fixed.
        let fonts = load_fonts(&Default::default()).unwrap();
        let test = lines(
            &doc("text-autospace-vertical-upright-001"),
            &fonts.collection,
        )
        .unwrap();
        let reference = lines(
            &doc("text-autospace-vertical-upright-001-ref"),
            &fonts.collection,
        )
        .unwrap();
        assert_eq!(test.len(), 4);
        for (t, r) in test.iter().zip(&reference) {
            assert!(close(t.inline_size - r.inline_size, 5.0), "{t:?} {r:?}");
        }
    }

    #[test]
    fn vertical_is_not_dropped_and_upright_reaches_glyph_runs() {
        let fonts = load_fonts(&Default::default()).unwrap();
        // The no-autospace reference isolates orientation and axis from the
        // autospace difference above.
        let reference = lines(
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
        let horizontal = lines(&horizontal, &fonts.collection).unwrap();
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
        let combine = lines(
            &doc("text-autospace-vertical-combine-001"),
            &fonts.collection,
        )
        .unwrap();
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

    #[test]
    fn autospace_auto_is_rejected() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let html = doc("text-autospace-vertical-upright-001")
            .replace("text-autospace: normal", "text-autospace: auto");
        assert!(lines(&html, &fonts.collection).is_err());
    }
}
