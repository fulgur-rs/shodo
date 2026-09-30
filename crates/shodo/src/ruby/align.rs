//! Distribute accepted column space around real cluster/composition owners.
use crate::RubyAlign;
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::line::Scan;
use crate::paragraph::ParagraphData;
use std::ops::Range;

/// Source-local padding introduced solely by ruby alignment. Ordinary tracking
/// and justification retain their existing caret allocation rules.
#[derive(Clone, Debug)]
pub(crate) struct CaretGap {
    pub(crate) text: Range<u32>,
    pub(crate) before: LayoutUnit,
    pub(crate) after: LayoutUnit,
}

struct Group {
    head: usize,
    tail: usize,
    level: u8,
    ruby: bool,
}

pub(crate) enum AnnotationAlign {
    Policy(RubyAlign),
    Gaps(Vec<(LayoutUnit, LayoutUnit)>),
}

pub(crate) fn count(data: &ParagraphData, range: Range<usize>) -> usize {
    groups(data, range).len()
}

fn groups(data: &ParagraphData, range: Range<usize>) -> Vec<Group> {
    let mut result = Vec::new();
    let mut nested = std::collections::HashMap::new();
    let mut through = range.end;
    data.ruby.intervals.intersecting(
        &data.ruby.containers,
        range.start,
        &mut through,
        |index, _| {
            let r = &data.ruby.containers[index];
            if range.start <= r.units.start && r.units.end <= range.end {
                nested.insert(r.units.start, r.units.end);
            }
        },
    );
    let mut i = range.start;
    while i < range.end {
        let unit = &data.units[i];
        if let Some(end) = nested.get(&i) {
            result.push(Group {
                head: i,
                tail: *end - 1,
                level: unit.level,
                ruby: true,
            });
            i = *end;
            continue;
        }
        if matches!(
            unit.kind,
            UnitKind::Cluster { .. } | UnitKind::Atomic { .. }
        ) || unit.combine.is_some()
        {
            let tail = if let Some(combine) = unit.combine {
                data.combine_spans[combine as usize]
                    .units
                    .end
                    .min(range.end)
                    - 1
            } else if let Some(shared) = &unit.shared_cluster {
                shared.units.end.min(range.end) - 1
            } else {
                data.unit_spacing[i].tail.min(range.end - 1)
            };
            result.push(Group {
                head: i,
                tail,
                level: unit.level,
                ruby: false,
            });
            i = tail + 1;
        } else {
            i += 1;
        }
    }
    result
}

/// Fixed-point edge/internal gaps: no font advance or outline is rescaled.
pub(crate) fn gaps(
    align: RubyAlign,
    count: usize,
    extra: LayoutUnit,
) -> Vec<(LayoutUnit, LayoutUnit)> {
    let mut result = vec![(LayoutUnit::ZERO, LayoutUnit::ZERO); count];
    if count == 0 {
        return result;
    }
    if align == RubyAlign::Center || align == RubyAlign::SpaceBetween && count == 1 {
        result[0].0 = extra.div_i32(2);
        result[count - 1].1 = extra - result[0].0;
    } else if align == RubyAlign::Start {
        result[count - 1].1 = extra;
    } else {
        let slots = if align == RubyAlign::SpaceAround {
            count * 2
        } else {
            count - 1
        };
        let mut previous = 0_i64;
        for slot in 0..slots {
            let next = i64::from(extra.raw()) * (slot + 1) as i64 / slots as i64;
            let value = LayoutUnit::from_raw((next - previous) as i32);
            previous = next;
            if align == RubyAlign::SpaceAround {
                if slot % 2 == 0 {
                    result[slot / 2].0 = value;
                } else {
                    result[slot / 2].1 = value;
                }
            } else {
                result[slot].1 = value;
            }
        }
    }
    result
}

fn distribute(
    data: &ParagraphData,
    start: usize,
    scan: &mut Scan,
    groups: &[Group],
    gaps: &[(LayoutUnit, LayoutUnit)],
    sat: &mut Saturation,
) {
    let levels: Vec<_> = groups
        .iter()
        .map(|g| unicode_bidi::Level::new(g.level).unwrap())
        .collect();
    let mut order = unicode_bidi::BidiInfo::reorder_visual(&levels);
    if data.base_level % 2 == 1 {
        order.reverse();
    }
    let leading = scan
        .leading
        .get_or_insert_with(|| vec![LayoutUnit::ZERO; scan.widths.len()]);
    for (visual, logical) in order.into_iter().enumerate() {
        let group = &groups[logical];
        let (before, after) = gaps[visual];
        if group.ruby {
            let (head, tail) = if group.level % 2 == data.base_level % 2 {
                (before, after)
            } else {
                (after, before)
            };
            scan.widths[group.head - start] = scan.widths[group.head - start].add(head, sat);
            scan.widths[group.tail - start] = scan.widths[group.tail - start].add(tail, sat);
            leading[group.head - start] = leading[group.head - start].add(head, sat);
            leading[group.tail - start] = leading[group.tail - start].add(tail, sat);
            continue;
        }
        let forward = group.level % 2 == data.base_level % 2;
        scan.ruby_caret_gaps.push(CaretGap {
            text: data.units[group.head].text.start..data.units[group.tail].text.end,
            before: if forward { before } else { after },
            after: if forward { after } else { before },
        });
        scan.widths[group.tail - start] =
            scan.widths[group.tail - start].add(before.add(after, sat), sat);
        let amount = if group.level % 2 == data.base_level % 2 {
            before
        } else {
            after
        };
        leading[group.head - start] = leading[group.head - start].add(amount, sat);
    }
}

pub(crate) fn bases(data: &ParagraphData, start: usize, scan: &mut Scan, sat: &mut Saturation) {
    let Some(measure) = scan.ruby.take() else {
        return;
    };
    for fragment in &measure.fragments {
        let ruby = &data.ruby.containers[fragment.container];
        for (column, ((base, natural), width)) in fragment
            .bases
            .iter()
            .zip(&fragment.base_widths)
            .zip(&fragment.base_columns)
            .enumerate()
        {
            let extra = width.sub(*natural, sat);
            if extra <= LayoutUnit::ZERO {
                continue;
            }
            let groups = groups(data, base.clone());
            if groups.is_empty() {
                continue;
            }
            let reserved = base
                .clone()
                .rev()
                .find(|i| {
                    matches!(
                        data.units[*i].kind,
                        UnitKind::Close { .. } | UnitKind::Cluster { .. } | UnitKind::Atomic { .. }
                    )
                })
                .unwrap();
            scan.widths[reserved - start] = scan.widths[reserved - start].sub(extra, sat);
            distribute(
                data,
                start,
                scan,
                &groups,
                &gaps(
                    ruby.columns[fragment.column_start + column].align,
                    groups.len(),
                    extra,
                ),
                sat,
            );
        }
    }
    scan.ruby = Some(measure);
}

pub(crate) fn annotation(
    data: &ParagraphData,
    start: usize,
    scan: &mut Scan,
    width: LayoutUnit,
    align: AnnotationAlign,
    sat: &mut Saturation,
) {
    let extra = width.sub(scan.content, sat).max(LayoutUnit::ZERO);
    if extra == LayoutUnit::ZERO {
        return;
    }
    let groups = groups(data, start..scan.end);
    if groups.is_empty() {
        return;
    }
    let gaps = match align {
        AnnotationAlign::Policy(align) => gaps(align, groups.len(), extra),
        AnnotationAlign::Gaps(gaps) => gaps,
    };
    debug_assert_eq!(gaps.len(), groups.len());
    distribute(data, start, scan, &groups, &gaps, sat);
    scan.content = scan.content.add(extra, sat);
}
