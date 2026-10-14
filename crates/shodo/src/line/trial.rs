//! Private Balance trial results; unsupported lines retain full finalization.
use super::Scan;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::ParagraphData;
use crate::{BreakToken, FloatCursor, LineResult};

#[derive(Clone, Copy, Debug)]
pub(super) struct TrialSummary {
    pub(super) token: BreakToken,
    pub(super) block_size: f32,
}

// The formatter result as the fixed-width driver consumes it: every event
// kind is kept, but only the fields that drive its progress.
pub(super) enum TrialResult {
    Line(TrialSummary),
    FloatEncountered { float_cursor: FloatCursor },
    BlockInInline { token_after: BreakToken },
    BlockSizeExceeded,
    Done,
    InvalidToken,
}

pub(super) trait Finalizer {
    type Output;
    const SUMMARY: bool = false;
    fn from_full(result: LineResult) -> Self::Output;
    fn summarize(_input: SummaryInput<'_>) -> Option<Self::Output> {
        None
    }
}

pub(super) struct SummaryInput<'a> {
    pub(super) data: &'a ParagraphData,
    pub(super) token: BreakToken,
    pub(super) scan: &'a Scan,
    pub(super) origin: LayoutUnit,
}

pub(super) struct Full;
impl Finalizer for Full {
    type Output = LineResult;
    fn from_full(result: LineResult) -> Self::Output {
        result
    }
}

pub(super) struct Trial;
impl Finalizer for Trial {
    type Output = TrialResult;
    const SUMMARY: bool = true;
    fn summarize(input: SummaryInput<'_>) -> Option<Self::Output> {
        summarize(input).map(TrialResult::Line)
    }
    fn from_full(result: LineResult) -> Self::Output {
        match result {
            LineResult::Line(line) => TrialResult::Line(TrialSummary {
                token: line.break_token(),
                block_size: line.block_size(),
            }),
            LineResult::FloatEncountered { float_cursor, .. } => {
                TrialResult::FloatEncountered { float_cursor }
            }
            LineResult::BlockInInline { token_after, .. } => {
                TrialResult::BlockInInline { token_after }
            }
            LineResult::BlockSizeExceeded { .. } => TrialResult::BlockSizeExceeded,
            LineResult::Done => TrialResult::Done,
            LineResult::InvalidToken => TrialResult::InvalidToken,
        }
    }
}

/// A deliberately narrow proof boundary. The probe writes no context state,
/// invokes no shaper and retains no geometry, even when it rejects a scan.
fn summarize(input: SummaryInput<'_>) -> Option<TrialSummary> {
    use super::fragments::{FragmentRecord, GlyphSource, RecordKind};
    use crate::analysis::units::UnitKind;
    let SummaryInput {
        data,
        token,
        scan,
        origin,
    } = input;
    let start = token.unit as usize;
    let root = &data.styles[0];
    let root_metrics = data.style_metrics[0];
    let bounded = |v: f32| v.is_finite() && v.abs() <= 4096.;
    let bounded_metrics = |m: crate::font::FontMetrics| {
        bounded(m.ascent) && bounded(m.descent) && bounded(m.line_gap)
    };
    if data.style.writing_mode != crate::geometry::WritingMode::HorizontalTb
        || data.first_line.is_some()
        || data.style.line_height_quirk
        || !data.ruby.containers.is_empty()
        || !data.floats.is_empty()
        || !data.boxes.is_empty()
        || !data.combine_spans.is_empty()
        || root.text_emphasis.is_some()
        || root.vertical_align != crate::style::VerticalAlign::Baseline
        || scan.end <= start
        || !scan.overlays.is_empty()
        || scan.leading.is_some()
        || scan.ruby.is_some()
        || !scan.ruby_caret_gaps.is_empty()
        || !bounded(root.font_size)
        || !bounded(root_metrics.size)
        || !bounded(root_metrics.em_ascent)
        || !bounded_metrics(root_metrics.metrics)
        || origin.to_f32().abs() > 1_048_576.
        || scan.content.to_f32().abs() > 1_048_576.
        || scan.hanging_end.to_f32().abs() > 1_048_576.
        || !bounded(scan.punctuation_edges.hang_start.to_f32())
    {
        return None;
    }
    let (mut above, mut below) = super::metrics::extents(
        root,
        root_metrics.metrics,
        root_metrics.vertical_metrics,
        false,
    );
    if !bounded(above) || !bounded(below) {
        return None;
    }
    // Every eligible parent is None. The default resolver therefore has the
    // exact parent lookup of ProfileResolver::new, without its redundant map.
    let mut resolver = super::metrics::ProfileResolver::default();
    let mut previous = None;
    let mut total = 0_f64;
    for (unit, width) in data.units[start..scan.end].iter().zip(&scan.widths) {
        let UnitKind::Cluster { run, glyphs, .. } = &unit.kind else {
            return None;
        };
        if unit.parent_box.is_some()
            || unit.combine.is_some()
            || unit.shared_cluster.is_some()
            || glyphs.is_empty()
            || unit.level != data.base_level
            || data.items[unit.item as usize].style != 0
            || data
                .text
                .get(unit.text.start as usize..unit.text.end as usize)?
                .contains('\u{ad}')
            || *width < LayoutUnit::ZERO
        {
            return None;
        }
        total += f64::from(width.to_f32());
        if total > 1_048_576. {
            return None;
        }
        let key = (*run, unit.item);
        if previous == Some(key) {
            continue;
        }
        let shaped = &data.runs[*run as usize];
        let metrics = shaped.instance.metrics?;
        if shaped.orientation != crate::shape::orientation::RunOrientation::Horizontal
            || !bounded(shaped.font_size)
            || !bounded_metrics(metrics)
        {
            return None;
        }
        // Glyph profiles in this class depend only on run and item, not text
        // or glyph ranges; adjacent equal keys can reuse their extent union.
        let record = FragmentRecord {
            kind: RecordKind::Glyphs {
                source: GlyphSource::Shared,
                run: *run,
                glyphs: glyphs.clone(),
                item: unit.item,
                text: unit.text.clone(),
            },
            inline_start: LayoutUnit::ZERO,
            inline_size: *width,
            level: unit.level,
        };
        let profile = resolver.record(data, &record, &[])?;
        if profile.group.is_some()
            || profile.own_group.is_some()
            || !bounded(profile.top)
            || !bounded(profile.bottom)
            || profile.shift != 0.
        {
            return None;
        }
        let content = super::metrics::em_content_extents(
            data,
            0,
            shaped.font,
            shaped.font_size,
            false,
            (metrics.ascent, metrics.descent),
        );
        if !bounded(content.0) || !bounded(content.1) {
            return None;
        }
        above = above.max(-profile.top);
        below = below.max(profile.bottom);
        previous = Some(key);
    }
    // Within this guard, fragment coordinates are bounded by origin + total
    // (< 2^22 px, including f32 conversion error). Font/annotation arithmetic uses a fixed number of bounded
    // terms (< 2^16 px). Neither approaches Q26.6's ~2^25 px boundary; no
    // omitted conversion/addition can saturate. Still validate the retained
    // metric conversions locally before accepting the probe.
    let mut sat = Saturation::default();
    let block_size = LayoutUnit::from_f32_ceil((above + below).max(0.), &mut sat).to_f32();
    let _ = LayoutUnit::from_f32_round(above, &mut sat);
    sat.is_clean().then_some(TrialSummary {
        token: BreakToken {
            para: data.id,
            unit: scan.end as u32,
            flags: if scan.reason == crate::output::BreakReason::Forced {
                BreakToken::AFTER_FORCED
            } else {
                0
            },
        },
        block_size,
    })
}

#[cfg(test)]
mod tests;
