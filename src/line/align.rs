use super::Scan;
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::output::BreakReason;
use crate::paragraph::ParagraphData;
use crate::style::{LineOptions, TextAlign, TextAlignLast, TextJustify};
use unicode_bidi::{BidiInfo, Level};

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
    let end = match align {
        TextAlign::End => !reversed_start,
        TextAlign::Left => data.base_level % 2 == 1,
        TextAlign::Right => data.base_level.is_multiple_of(2),
        _ => reversed_start,
    };
    let mut shift = if align == TextAlign::Center {
        spare.div_i32(2)
    } else if end {
        spare
    } else {
        LayoutUnit::ZERO
    };
    if reversed_start {
        shift = shift.sub(indent, sat);
    }
    let mut result = Alignment {
        shift,
        justified: false,
    };
    if !matches!(align, TextAlign::Justify | TextAlign::JustifyAll)
        || options.text_justify == TextJustify::None
        || spare == LayoutUnit::ZERO
    {
        return result;
    }
    // Owned windows may have a different number of clusters from their
    // shared source. Enumerate the glyph source that will actually render.
    let mut clusters = Vec::new();
    let mut seen = vec![false; scan.overlays.len()];
    let visible_hyphen = scan
        .overlays
        .iter()
        .find_map(|w| w.hyphen.as_ref().map(|text| text.start));
    for i in start..scan.hang_start {
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
                    clusters.push((unit, space, Some((w, end - 1))));
                }
                g = end;
            }
        } else if data.units[i].shared_cluster.as_ref().is_none_or(|c| {
            let end = c.slices.partition_point(|i| *i < scan.hang_start);
            end > 0 && c.slices[end - 1] == i
        }) {
            clusters.push((i, *space, None));
        }
    }
    let mut opportunities: Vec<_> = clusters
        .iter()
        .enumerate()
        .filter(|(j, (_, space, _))| {
            if options.text_justify == TextJustify::InterCharacter {
                *j + 1 < clusters.len()
                    && super::spacing::intercharacter_allowed(
                        data,
                        clusters[*j].0,
                        clusters[*j + 1].0,
                    )
            } else {
                *space
            }
        })
        .map(|(_, point)| *point)
        .collect();
    if opportunities.is_empty() {
        return result;
    }
    let levels: Vec<_> = opportunities
        .iter()
        .map(|(i, _, _)| Level::new(data.units[*i].level).unwrap())
        .collect();
    let mut order = BidiInfo::reorder_visual(&levels);
    if data.base_level % 2 == 1 {
        order.reverse();
    }
    opportunities = order.into_iter().map(|i| opportunities[i]).collect();
    let n = opportunities.len() as i32;
    for (j, (i, _, owned)) in opportunities.into_iter().enumerate() {
        let extra = LayoutUnit::from_raw(spare.raw() / n + i32::from((j as i32) < spare.raw() % n));
        scan.widths[i - start] = scan.widths[i - start].add(extra, sat);
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
