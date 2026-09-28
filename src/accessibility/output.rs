use super::{AccessibleCharacter, AccessibleCharacterKind, AccessibleLine, AccessibleRun};
use crate::analysis::{ItemKind, units::UnitKind};
use crate::geometry::LogicalRect;
use crate::hit::{Caret, LineLayout, TextPosition};
use crate::mapping::Affinity;
use crate::{Fragment, GlyphOrientation, Line};

pub(super) fn union(a: LogicalRect, b: LogicalRect) -> LogicalRect {
    let inline_start = a.inline_start.min(b.inline_start);
    let block_start = a.block_start.min(b.block_start);
    LogicalRect {
        inline_start,
        block_start,
        inline_size: (a.inline_start + a.inline_size).max(b.inline_start + b.inline_size)
            - inline_start,
        block_size: (a.block_start + a.block_size).max(b.block_start + b.block_size) - block_start,
    }
}

fn point(c: Caret) -> (f32, f32) {
    (c.rect.inline_start, c.rect.block_start)
}

pub(super) fn build<'a>(lines: &'a [Line], hit: &LineLayout<'_>) -> Vec<AccessibleLine<'a>> {
    let mut result: Vec<_> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| build_line(index, line, hit))
        .collect();
    word_starts(&mut result);
    result
}

fn build_line<'a>(index: usize, line: &'a Line, hit: &LineLayout<'_>) -> AccessibleLine<'a> {
    let range = line.text_range();
    let text_range = range.start as u32..range.end as u32;
    let mut offsets: Vec<_> = hit
        .accepted_stops(index)
        .iter()
        .map(|c| c.position.offset)
        .collect();
    offsets.dedup();
    let mut segments: Vec<_> = hit.accepted_segments(index).collect();
    segments.sort_by_key(|(r, _)| r.start);
    let mut segment_index = 0;
    let mut glyph_runs: Vec<_> = line
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .collect();
    glyph_runs.sort_by_key(|r| r.text_range().start);
    let mut characters = Vec::new();
    let mut runs: Vec<AccessibleRun<'a>> = Vec::new();
    for pair in offsets.windows(2) {
        let span = pair[0]..pair[1];
        let leading = hit
            .caret(TextPosition {
                line: index,
                offset: span.start,
                affinity: Affinity::Downstream,
            })
            .expect("accepted caret");
        let trailing = hit
            .caret(TextPosition {
                line: index,
                offset: span.end,
                affinity: Affinity::Upstream,
            })
            .expect("accepted caret");
        while segment_index < segments.len() && segments[segment_index].0.end <= span.start {
            segment_index += 1;
        }
        let rect = segments[segment_index..]
            .iter()
            .take_while(|(r, _)| r.start < span.end)
            .filter(|(r, _)| r.end > span.start)
            .map(|(_, r)| *r)
            .reduce(union)
            .unwrap_or({
                // Nonpainting source still occupies a selectable text interval.
                // Its caret may be copied from its nearest painted neighbour.
                LogicalRect {
                    inline_size: 0.0,
                    ..leading.rect
                }
            });
        let unit_begin = line
            .data
            .units
            .partition_point(|u| u.text.end <= span.start);
        let units = &line.data.units[unit_begin..];
        let atomic = units
            .iter()
            .take_while(|u| u.text.start < span.end)
            .find_map(|u| match u.kind {
                UnitKind::Atomic { node } => Some(node),
                _ => None,
            });
        let kind = if let Some(node) = atomic {
            AccessibleCharacterKind::Atomic(node)
        } else if units
            .iter()
            .take_while(|u| u.text.start < span.end)
            .any(|u| u.kind == UnitKind::ForcedBreak)
        {
            AccessibleCharacterKind::HardBreak
        } else {
            AccessibleCharacterKind::Text
        };
        let character = characters.len();
        let item_begin = line
            .data
            .items
            .partition_point(|i| i.text.end <= span.start);
        let items = &line.data.items[item_begin..];
        let item = items
            .iter()
            .take_while(|i| i.text.start < span.end)
            .find(|i| matches!(i.kind, ItemKind::Text))
            .or_else(|| {
                items
                    .iter()
                    .take_while(|i| i.text.start < span.end)
                    .find(|i| !i.text.is_empty())
            });
        let style_index = item.map_or(0, |i| i.style) as usize;
        let style = &line.data.styles[style_index];
        let owner_offset = item.map_or(span.start, |i| i.text.start.max(span.start));
        let glyph_end =
            glyph_runs.partition_point(|r| r.text_range().start <= owner_offset as usize);
        let glyph = glyph_runs[..glyph_end]
            .iter()
            .rev()
            .find(|r| r.text_range().end > owner_offset as usize);
        let tab = items
            .iter()
            .take_while(|i| i.text.start < span.end)
            .any(|i| matches!(i.kind, ItemKind::Tab));
        let metrics = line.data.style_metrics[style_index];
        let orientation = glyph.map_or_else(
            || {
                if line.data.combine_at_text(owner_offset).is_some() {
                    GlyphOrientation::Combined
                } else {
                    crate::shape::orientation::resolve(
                        line.writing_mode(),
                        style.text_orientation,
                        line.text()[span.start as usize..span.end as usize]
                            .chars()
                            .next()
                            .unwrap(),
                    )
                }
            },
            |r| r.orientation(),
        );
        let current = AccessibleRun {
            character_range: character..character + 1,
            text_range: span.clone(),
            node: item.and_then(|i| i.node),
            style,
            font: glyph
                .map(|r| r.font())
                .or_else(|| tab.then_some(metrics.font)),
            font_size: glyph.map_or(metrics.size, |r| r.font_size()),
            bidi_level: glyph.map_or_else(
                || units.first().map_or(line.data.base_level, |u| u.level),
                |r| r.bidi_level(),
            ),
            orientation,
            bounds: rect,
        };
        let same_kind = characters
            .last()
            .is_some_and(|c: &AccessibleCharacter<'_>| c.kind == kind);
        if let Some(previous) = runs.last_mut()
            && same_kind
            && kind == AccessibleCharacterKind::Text
            && previous.node == current.node
            && previous.style == current.style
            && previous.font == current.font
            && previous.font_size == current.font_size
            && previous.bidi_level == current.bidi_level
            && previous.orientation == current.orientation
        {
            previous.character_range.end = current.character_range.end;
            previous.text_range.end = span.end;
            previous.bounds = union(previous.bounds, rect);
        } else {
            runs.push(current);
        }
        characters.push(AccessibleCharacter {
            text: &line.text()[span.start as usize..span.end as usize],
            text_range: span,
            kind,
            rect,
            leading: point(leading),
            trailing: point(trailing),
        });
    }
    let empty_caret = hit.accepted_stops(index)[0].rect;
    if runs.is_empty() {
        runs.push(AccessibleRun {
            character_range: 0..0,
            text_range: text_range.clone(),
            node: None,
            style: &line.data.styles[0],
            font: None,
            font_size: line.data.style_metrics[0].size,
            bidi_level: line.data.base_level,
            orientation: crate::shape::orientation::resolve(
                line.writing_mode(),
                line.data.styles[0].text_orientation,
                ' ',
            ),
            bounds: empty_caret,
        });
    }
    let bounds = characters
        .iter()
        .map(|c| c.rect)
        .reduce(union)
        .unwrap_or(empty_caret);
    AccessibleLine {
        index,
        text: &line.text()[range],
        text_range,
        characters,
        runs,
        word_starts: Vec::new(),
        writing_mode: line.writing_mode(),
        direction: line.used_direction(),
        break_reason: line.break_reason(),
        bounds,
    }
}

fn word_starts(lines: &mut [AccessibleLine<'_>]) {
    let text: String = lines.iter().map(|l| l.text).collect();
    #[cfg(feature = "complex-scripts")]
    let segmenter = icu_segmenter::WordSegmenter::new_auto(Default::default());
    #[cfg(not(feature = "complex-scripts"))]
    let segmenter = icu_segmenter::WordSegmenter::new_for_non_complex_scripts(Default::default());
    let mut starts = Vec::new();
    let mut begin = 0;
    for (end, kind) in segmenter.segment_str(&text).iter_with_word_type() {
        if kind.is_word_like() {
            starts.push(begin);
        }
        begin = end;
    }
    let mut prefix = 0;
    let mut start_index = 0;
    for line in lines {
        while start_index < starts.len() && starts[start_index] < prefix + line.text.len() {
            let at = starts[start_index] - prefix + line.text_range.start as usize;
            let character = line
                .characters
                .partition_point(|c| c.text_range.end as usize <= at);
            if character < line.characters.len() {
                line.word_starts.push(character);
            }
            start_index += 1;
        }
        for (i, c) in line.characters.iter().enumerate() {
            if matches!(c.kind, AccessibleCharacterKind::Atomic(_)) {
                line.word_starts.push(i);
                if i + 1 < line.characters.len() {
                    line.word_starts.push(i + 1);
                }
            }
        }
        line.word_starts.sort_unstable();
        line.word_starts.dedup();
        prefix += line.text.len();
    }
}
