use super::Scan;
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::output::BreakReason;
use crate::paragraph::ParagraphData;
use crate::style::{LineOptions, TextAlign, TextAlignLast, TextJustify};
use unicode_bidi::{BidiInfo, Level};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Point {
    unit: usize,
    owned: Option<(usize, usize)>,
}

#[derive(Clone, Copy)]
struct Opportunity {
    point: Point,
    count: i64,
}

fn add_opportunity(opportunities: &mut Vec<Opportunity>, point: Point, count: usize) {
    if count == 0 {
        return;
    }
    if let Some(previous) = opportunities.last_mut()
        && previous.point == point
    {
        previous.count += count as i64;
    } else {
        opportunities.push(Opportunity {
            point,
            count: count as i64,
        });
    }
}

fn shift_for(
    align: TextAlign,
    spare: LayoutUnit,
    reversed_start: bool,
    rtl: bool,
    indent: LayoutUnit,
    sat: &mut Saturation,
) -> LayoutUnit {
    let end = match align {
        TextAlign::End => !reversed_start,
        TextAlign::Left => rtl,
        TextAlign::Right => !rtl,
        _ => reversed_start,
    };
    let shift = if align == TextAlign::Center {
        spare.div_i32(2)
    } else if end {
        spare
    } else {
        LayoutUnit::ZERO
    };
    if reversed_start {
        shift.sub(indent, sat)
    } else {
        shift
    }
}

fn unexpandable_alignment(options: &LineOptions) -> TextAlign {
    match options.text_align_last {
        TextAlignLast::Auto if options.text_align == TextAlign::JustifyAll => TextAlign::Center,
        TextAlignLast::Auto | TextAlignLast::Start => TextAlign::Start,
        TextAlignLast::End => TextAlign::End,
        TextAlignLast::Left => TextAlign::Left,
        TextAlignLast::Right => TextAlign::Right,
        TextAlignLast::Center | TextAlignLast::Justify => TextAlign::Center,
    }
}

pub(super) struct Alignment {
    pub(super) shift: LayoutUnit,
    pub(super) justified: bool,
}

pub(super) fn apply(
    data: &ParagraphData,
    start: usize,
    scan: &mut Scan,
    options: &LineOptions,
    available: LayoutUnit,
    indent: LayoutUnit,
    sat: &mut Saturation,
) -> Alignment {
    let last = !matches!(scan.reason, BreakReason::Regular | BreakReason::Emergency);
    let mut align = options.text_align;
    if last {
        align = match options.text_align_last {
            TextAlignLast::Auto if align == TextAlign::Justify => TextAlign::Start,
            TextAlignLast::Auto => align,
            TextAlignLast::Start => TextAlign::Start,
            TextAlignLast::End => TextAlign::End,
            TextAlignLast::Left => TextAlign::Left,
            TextAlignLast::Right => TextAlign::Right,
            TextAlignLast::Center => TextAlign::Center,
            TextAlignLast::Justify => TextAlign::Justify,
        };
    }
    let spare = available
        .sub(indent, sat)
        .sub(scan.content, sat)
        .max(LayoutUnit::ZERO);
    let inline_level = data
        .bidi_paragraph_at_unit(start)
        .map_or(data.base_level, |p| p.inline_level);
    let reversed_start = inline_level % 2 != data.base_level % 2;
    let mut result = Alignment {
        shift: shift_for(
            align,
            spare,
            reversed_start,
            data.base_level % 2 == 1,
            indent,
            sat,
        ),
        justified: false,
    };
    // Disabled justification keeps start alignment; the unexpandable-text
    // fallback below applies only when justification is enabled.
    if !matches!(align, TextAlign::Justify | TextAlign::JustifyAll)
        || spare == LayoutUnit::ZERO
        || options.text_justify == TextJustify::None
    {
        return result;
    }
    let fallback = |sat: &mut Saturation| {
        shift_for(
            unexpandable_alignment(options),
            spare,
            reversed_start,
            data.base_level % 2 == 1,
            indent,
            sat,
        )
    };
    // Owned windows may have a different number of clusters from their
    // shared source. Enumerate the glyph source that will actually render.
    let mut clusters = Vec::new();
    let mut seen = vec![false; scan.overlays.len()];
    let visible_hyphen = scan
        .overlays
        .iter()
        .find_map(|w| w.hyphen.as_ref().map(|text| text.start));
    for i in start..scan.hang_start {
        if let Some(index) = data.units[i].combine {
            let span = &data.combine_spans[index as usize];
            if i + 1 == span.units.end {
                clusters.push((i, false, None, span.text.clone()));
            }
            continue;
        }
        let UnitKind::Cluster { glyphs, space, .. } = &data.units[i].kind else {
            continue;
        };
        if data
            .text
            .get(data.units[i].text.start as usize..data.units[i].text.end as usize)
            == Some("\u{ad}")
            && visible_hyphen != Some(data.units[i].text.start)
        {
            continue;
        }
        if let Some((w, window)) = scan.overlays.iter().enumerate().find(|(_, w)| {
            w.glyphs.start <= glyphs.start
                && glyphs.end <= w.glyphs.end
                && w.text.start <= data.units[i].text.start
                && data.units[i].text.start < w.text.end
        }) {
            if std::mem::replace(&mut seen[w], true) {
                continue;
            }
            let original =
                &data.glyphs.cluster[window.glyphs.start as usize..window.glyphs.end as usize];
            let mut g = 0;
            while g < window.store.len() {
                let cluster = window.store.cluster[g];
                let mut end = g + 1;
                while end < window.store.len() && window.store.cluster[end] == cluster {
                    end += 1;
                }
                let old = window.glyphs.start as usize
                    + original
                        .partition_point(|c| *c <= cluster)
                        .saturating_sub(1);
                let mut unit = data.clusters[data.glyph_clusters[old] as usize] as usize;
                if let Some(shared) = &data.units[unit].shared_cluster {
                    unit = shared.slices[shared
                        .slices
                        .partition_point(|i| data.units[*i].text.start <= cluster)
                        .saturating_sub(1)];
                }
                if unit >= start
                    && unit < scan.hang_start
                    && (!data.text[cluster as usize..].starts_with('\u{ad}')
                        || visible_hyphen == Some(cluster))
                {
                    let space = data.text[cluster as usize..].starts_with(' ');
                    let text_end = window
                        .store
                        .cluster
                        .get(end)
                        .copied()
                        .unwrap_or(window.text.end);
                    clusters.push((unit, space, Some((w, end - 1)), cluster..text_end));
                }
                g = end;
            }
        } else if data.units[i].shared_cluster.as_ref().is_none_or(|c| {
            let end = c.slices.partition_point(|i| *i < scan.hang_start);
            end > 0 && c.slices[end - 1] == i
        }) {
            clusters.push((i, *space, None, data.units[i].shaping_text().clone()));
        }
    }
    let mut opportunities = Vec::new();
    if clusters.is_empty() {
        result.shift = fallback(sat);
        return result;
    }
    if options.text_justify == TextJustify::InterCharacter {
        let mut previous = None;
        let content_end = data.units[scan.hang_start - 1].text.end;
        for (unit, _, owned, text) in &clusters {
            let text = text.start.max(data.units[start].text.start)..text.end.min(content_end);
            let point = Point {
                unit: *unit,
                owned: *owned,
            };
            let (internal, first, last) =
                super::spacing::justification_metadata(data, text.clone(), visible_hyphen);
            if let Some((first, first_offset)) = first {
                if let Some((previous_point, previous_kind, previous_offset)) = previous
                    && !(previous_kind == super::spacing_summary::Kind::Cursive
                        && first == super::spacing_summary::Kind::Cursive)
                    && super::punctuation::justify_boundary(data, previous_offset, first_offset)
                {
                    add_opportunity(&mut opportunities, previous_point, 1);
                }
                add_opportunity(&mut opportunities, point, internal);
                let (last, last_offset) = last.unwrap();
                previous = Some((point, last, last_offset));
            } else if data.breaks.caret_cuts.binary_search(&text.start).is_err()
                && let Some((previous_point, _, _)) = &mut previous
            {
                // A mark or indivisible transformed continuation belongs to
                // the preceding typographic unit; place its following gap
                // after the final glyph piece, rather than inside the unit.
                *previous_point = point;
            }
        }
    } else {
        for (unit, space, owned, _) in &clusters {
            if *space {
                add_opportunity(
                    &mut opportunities,
                    Point {
                        unit: *unit,
                        owned: *owned,
                    },
                    1,
                );
            }
        }
    }
    if opportunities.is_empty() {
        result.shift = fallback(sat);
        return result;
    }
    let levels: Vec<_> = opportunities
        .iter()
        .map(|o| Level::new(data.units[o.point.unit].level).unwrap())
        .collect();
    let mut order = BidiInfo::reorder_visual(&levels);
    if data.base_level % 2 == 1 {
        order.reverse();
    }
    opportunities = order.into_iter().map(|i| opportunities[i]).collect();
    // Aggregate internal boundaries per final glyph cluster instead of
    // retaining one allocation record per character in a compressed ligature.
    let n: i64 = opportunities.iter().map(|o| o.count).sum();
    let mut remainder = i64::from(spare.raw()) % n;
    let per_boundary = i64::from(spare.raw()) / n;
    for opportunity in opportunities {
        let Point { unit: i, owned } = opportunity.point;
        let residual = remainder.min(opportunity.count);
        remainder -= residual;
        let extra = LayoutUnit::from_raw((per_boundary * opportunity.count + residual) as i32);
        scan.widths[i - start] = scan.widths[i - start].add(extra, sat);
        if data.combine_at_text(data.units[i].text.start).is_some()
            && data.units[i].level % 2 != data.base_level % 2
        {
            let leading = scan
                .leading
                .get_or_insert_with(|| vec![LayoutUnit::ZERO; scan.widths.len()]);
            leading[i - start] = leading[i - start].add(extra, sat);
        }
        if let Some((window, g)) = owned {
            let store = &mut scan.overlays[window].store;
            let spacing = store
                .spacing
                .get_or_insert_with(|| vec![LayoutUnit::ZERO; store.id.len()]);
            spacing[g] = spacing[g].add(extra, sat);
        }
    }
    scan.content = scan.content.add(spare, sat);
    // Successful justification consumes the spare width in either direction;
    // the start-alignment fallback shift is only appropriate without expansion.
    result.shift = if reversed_start {
        LayoutUnit::ZERO.sub(indent, sat)
    } else {
        LayoutUnit::ZERO
    };
    result.justified = true;
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_justification_allocates_positions() {
        use crate::limits::Limits;
        use crate::style::{LineOptions, ParagraphStyle, TextAlign};
        use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder};
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "a b",
        );
        let p = b
            .build(
                &mut LayoutContext::new(),
                &crate::font::FontCollection::new(&Limits::default()),
            )
            .unwrap();
        for a in [TextAlign::Start, TextAlign::JustifyAll] {
            let o = LineOptions {
                text_align: a,
                ..LineOptions::default()
            };
            let LineResult::Line(l) = p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &o,
                &LineConstraint::new(100.0),
                &AtomicSizes::EMPTY,
            ) else {
                panic!()
            };
            if a == TextAlign::Start {
                assert!(l.positions.is_none());
            } else {
                assert_eq!(
                    l.positions.unwrap().1.len()
                        * std::mem::size_of::<crate::geometry::LayoutUnit>(),
                    3 * 4
                );
            }
        }
    }
}
