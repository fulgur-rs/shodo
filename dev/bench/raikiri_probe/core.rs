//! Actual engine line protocols, reused by contract validation and probes.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::{LayoutContext, Line, LineConstraint, LineResult};
use shodo_raikiri_integration::PreparedIfc;
use std::{collections::BTreeMap, error::Error};

pub fn candidate_retry(
    prepared: &PreparedIfc,
    context: &mut LayoutContext,
    width: f32,
) -> Result<(Vec<Line>, usize), Box<dyn Error>> {
    if !width.is_finite() || width < 0.0 {
        return Err("candidate retry requires a nonnegative finite inline size".into());
    }
    let options = prepared.line_options(width);
    let mut token = prepared.paragraph.start_token();
    let mut block_offset = 0.0;
    let mut lines = Vec::new();
    let mut rejections = 0;
    for _ in 0..100_000 {
        // Force an actual unsatisfied remaining height at each line boundary.
        let mut constraint = LineConstraint::new(width);
        constraint.block_offset = block_offset;
        constraint.max_block_size = Some(0.0);
        let rejected =
            prepared
                .paragraph
                .next_line(context, token, &options, &constraint, &prepared.atomics);
        let accepted = match rejected {
            LineResult::BlockSizeExceeded { .. } => {
                rejections += 1;
                // The original value token is reused. A rejected trial never
                // supplies a continuation token for the caller to accept.
                constraint.max_block_size = None;
                prepared.paragraph.next_line(
                    context,
                    token,
                    &options,
                    &constraint,
                    &prepared.atomics,
                )
            }
            LineResult::Done => return Ok((lines, rejections)),
            LineResult::Line(line) if line.block_size() == 0.0 => {
                token = line.break_token();
                lines.push(line);
                continue;
            }
            _ => return Err("candidate zero-height trial did not reject a positive line".into()),
        };
        match accepted {
            LineResult::Line(line) => {
                token = line.break_token();
                block_offset += line.block_size();
                lines.push(line);
            }
            _ => return Err("candidate restored checkpoint did not accept its line".into()),
        }
    }
    Err("candidate retry exceeded the explicit call budget".into())
}

pub fn native_retry(
    layout: &mut parley::Layout<()>,
    width: f32,
) -> Result<(usize, usize), Box<dyn Error>> {
    use parley::layout::YieldData;
    if !width.is_finite() || width < 0.0 {
        return Err("native retry requires a nonnegative finite inline size".into());
    }
    let mut breaker = layout.break_lines();
    breaker.state_mut().set_layout_max_advance(width);
    breaker.state_mut().set_line_max_advance(width);
    let mut engine_rejections = 0;
    let mut caller_rejections = 0;
    for _ in 0..100_000 {
        let checkpoint = breaker.state().clone();
        breaker.state_mut().set_line_max_height(0.0);
        match breaker.break_next() {
            None => {
                if !breaker.is_done() {
                    return Err("native breaker stopped before Done".into());
                }
                breaker.finish();
                return Ok((engine_rejections, caller_rejections));
            }
            Some(YieldData::MaxHeightExceeded(_)) => {
                breaker.revert_to(checkpoint);
                breaker.state_mut().set_line_max_height(f32::MAX);
                match breaker.break_next() {
                    Some(YieldData::LineBreak(_)) => engine_rejections += 1,
                    _ => return Err("native restored checkpoint did not accept its line".into()),
                }
            }
            Some(YieldData::LineBreak(line)) if line.line_height == 0.0 => {}
            Some(YieldData::LineBreak(_)) => {
                // Parley can commit a final one-cluster line before yielding
                // MaxHeightExceeded. The caller still checks the returned
                // height, rejects that placement, and restores the saved
                // breaker state. Count this separately from an engine event.
                breaker.revert_to(checkpoint);
                breaker.state_mut().set_line_max_height(f32::MAX);
                match breaker.break_next() {
                    Some(YieldData::LineBreak(_)) => caller_rejections += 1,
                    _ => {
                        return Err(
                            "native caller-rejected checkpoint did not replay its line".into()
                        );
                    }
                }
            }
            Some(other) => {
                return Err(
                    format!("native retry encountered an unrepresented event: {other:?}").into(),
                );
            }
        }
    }
    Err("native retry exceeded the explicit call budget".into())
}

pub fn native_output(layout: &parley::Layout<()>) -> Value {
    let mut fonts = BTreeMap::new();
    let lines: Vec<_> = layout
        .lines()
        .map(|line| {
            let runs: Vec<_> = line
                .items()
                .filter_map(|item| {
                    let parley::PositionedLayoutItem::GlyphRun(view) = item else {
                        return None;
                    };
                    let run = view.run();
                    let font = fonts
                        .entry(run.font().data.id())
                        .or_insert_with(|| format!("{:x}", Sha256::digest(run.font().data.data())));
                    let glyphs: Vec<_> = view
                        .positioned_glyphs()
                        .map(|g| json!([g.id, g.x, g.y, g.advance]))
                        .collect();
                    Some(json!({"font_sha256": font, "font_index": run.font().index,
                "font_size": run.font_size(), "source_range": run.text_range(),
                "coords": run.normalized_coords(), "glyphs": glyphs}))
                })
                .collect();
            json!({"range": line.text_range(), "advance": line.metrics().advance,
            "height": line.metrics().line_height, "baseline": line.metrics().baseline,
            "runs": runs})
        })
        .collect();
    json!(lines)
}
