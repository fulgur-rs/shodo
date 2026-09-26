use super::Scan;
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::output::BreakReason;
use crate::paragraph::ParagraphData;
use crate::style::{LineOptions, TextAlign, TextAlignLast, TextJustify};
use unicode_bidi::{BidiInfo, Level};

pub(super) struct Alignment {
    pub(super) shift: LayoutUnit,
    pub(super) positions: Option<(u32, Vec<LayoutUnit>)>,
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
    let last = scan.reason != BreakReason::Regular;
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
    let end = match align {
        TextAlign::End => true,
        TextAlign::Left => data.base_level % 2 == 1,
        TextAlign::Right => data.base_level.is_multiple_of(2),
        _ => false,
    };
    let shift = if end {
        spare
    } else if align == TextAlign::Center {
        spare.div_i32(2)
    } else {
        LayoutUnit::ZERO
    };
    let mut result = Alignment {
        shift,
        positions: None,
    };
    if !matches!(align, TextAlign::Justify | TextAlign::JustifyAll)
        || options.text_justify == TextJustify::None
        || spare == LayoutUnit::ZERO
    {
        return result;
    }
    let clusters: Vec<_> = (start..scan.hang_start)
        .filter(|i| matches!(data.units[*i].kind, UnitKind::Cluster { .. }))
        .collect();
    let mut opportunities: Vec<_> = clusters
        .iter()
        .copied()
        .filter(|i| match data.units[*i].kind {
            UnitKind::Cluster { space, .. } => {
                if options.text_justify == TextJustify::InterCharacter {
                    Some(i) != clusters.last()
                } else {
                    space
                }
            }
            _ => false,
        })
        .collect();
    if opportunities.is_empty() {
        return result;
    }
    let levels: Vec<_> = opportunities
        .iter()
        .map(|i| Level::new(data.units[*i].level).unwrap())
        .collect();
    let mut order = BidiInfo::reorder_visual(&levels);
    if data.base_level % 2 == 1 {
        order.reverse();
    }
    opportunities = order.into_iter().map(|i| opportunities[i]).collect();
    let n = opportunities.len() as i32;
    for (j, i) in opportunities.into_iter().enumerate() {
        let extra = LayoutUnit::from_raw(spare.raw() / n + i32::from((j as i32) < spare.raw() % n));
        scan.widths[i - start] = scan.widths[i - start].add(extra, sat);
    }
    scan.content = scan.content.add(spare, sat);
    let ranges: Vec<_> = data.units[start..scan.end]
        .iter()
        .filter_map(|u| match &u.kind {
            UnitKind::Cluster { glyphs, .. } => Some(glyphs.clone()),
            _ => None,
        })
        .collect();
    if let (Some(first), Some(last)) = (ranges.first(), ranges.last()) {
        let mut positions = vec![LayoutUnit::ZERO; (last.end - first.start) as usize];
        let mut pos = LayoutUnit::ZERO;
        for (u, width) in data.units[start..scan.end].iter().zip(&scan.widths) {
            if let UnitKind::Cluster { glyphs, .. } = &u.kind {
                for g in glyphs.clone() {
                    positions[(g - first.start) as usize] = if g == glyphs.start {
                        pos
                    } else {
                        pos.add(*width, sat)
                    };
                }
            }
            pos = pos.add(*width, sat);
        }
        result.positions = Some((first.start, positions));
    }
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
