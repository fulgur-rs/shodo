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

pub(crate) enum AnnotationAlign<'a> {
    Policy(RubyAlign),
    Gaps(&'a [(LayoutUnit, LayoutUnit)]),
}

#[cfg(test)]
thread_local! {
    static GROUP_VECTORS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn count(data: &ParagraphData, range: Range<usize>) -> usize {
    let mut count = 0;
    visit_groups(data, range, |_, _, _, _| count += 1);
    count
}

fn groups(data: &ParagraphData, range: Range<usize>) -> Vec<Group> {
    #[cfg(test)]
    GROUP_VECTORS.with(|vectors| vectors.set(vectors.get() + 1));
    let mut result = Vec::new();
    visit_groups(data, range, |head, tail, level, ruby| {
        result.push(Group {
            head,
            tail,
            level,
            ruby,
        });
    });
    result
}

fn visit_groups(
    data: &ParagraphData,
    range: Range<usize>,
    mut visit: impl FnMut(usize, usize, u8, bool),
) {
    let mut i = range.start;
    let mut pending = None;
    let mut through = range.end;
    data.ruby.intervals.intersecting(
        &data.ruby.containers,
        range.start,
        &mut through,
        |index, _| {
            let r = &data.ruby.containers[index];
            if range.start > r.units.start || r.units.end > range.end {
                return;
            }

            match pending {
                Some((start, _, level)) if start == r.units.start => {
                    // The old start-keyed map overwrote equal starts in index order.
                    pending = Some((start, r.units.end, level));
                }
                Some((start, end, level)) => {
                    i = visit_unit_groups(data, i, start, range.end, &mut visit);
                    if i == start {
                        visit(start, end - 1, level, true);
                        i = end;
                    }
                    pending = (r.units.start >= i).then_some((
                        r.units.start,
                        r.units.end,
                        data.units[r.units.start].level,
                    ));
                }
                None => {
                    pending = Some((r.units.start, r.units.end, data.units[r.units.start].level));
                }
            }
        },
    );

    if let Some((start, end, level)) = pending {
        i = visit_unit_groups(data, i, start, range.end, &mut visit);
        if i == start {
            visit(start, end - 1, level, true);
            i = end;
        }
    }
    visit_unit_groups(data, i, range.end, range.end, &mut visit);
}

fn visit_unit_groups(
    data: &ParagraphData,
    mut i: usize,
    stop: usize,
    range_end: usize,
    visit: &mut impl FnMut(usize, usize, u8, bool),
) -> usize {
    while i < stop {
        let unit = &data.units[i];
        if matches!(
            unit.kind,
            UnitKind::Cluster { .. } | UnitKind::Atomic { .. }
        ) || unit.combine.is_some()
        {
            let tail = if let Some(combine) = unit.combine {
                data.combine_spans[combine as usize]
                    .units
                    .end
                    .min(range_end)
                    - 1
            } else if let Some(shared) = &unit.shared_cluster {
                shared.units.end.min(range_end) - 1
            } else {
                data.unit_spacing[i].tail.min(range_end - 1)
            };
            visit(i, tail, unit.level, false);
            i = tail + 1;
        } else {
            i += 1;
        }
    }
    i
}

/// Fixed-point edge/internal gaps: no font advance or outline is rescaled.
pub(crate) fn gaps(
    align: RubyAlign,
    count: usize,
    extra: LayoutUnit,
) -> Vec<(LayoutUnit, LayoutUnit)> {
    let mut result = Vec::new();
    append_gaps(align, count, extra, &mut result);
    result
}

pub(crate) fn append_gaps(
    align: RubyAlign,
    count: usize,
    extra: LayoutUnit,
    result: &mut Vec<(LayoutUnit, LayoutUnit)>,
) {
    let start = result.len();
    result.resize(start + count, (LayoutUnit::ZERO, LayoutUnit::ZERO));
    if count == 0 {
        return;
    }
    if align == RubyAlign::Center || align == RubyAlign::SpaceBetween && count == 1 {
        result[start].0 = extra.div_i32(2);
        result[start + count - 1].1 = extra - result[start].0;
    } else if align == RubyAlign::Start {
        result[start + count - 1].1 = extra;
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
                    result[start + slot / 2].0 = value;
                } else {
                    result[start + slot / 2].1 = value;
                }
            } else {
                result[start + slot].1 = value;
            }
        }
    }
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
    align: AnnotationAlign<'_>,
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
    match align {
        AnnotationAlign::Policy(align) => {
            let gaps = gaps(align, groups.len(), extra);
            debug_assert_eq!(gaps.len(), groups.len());
            distribute(data, start, scan, &groups, &gaps, sat);
        }
        AnnotationAlign::Gaps(gaps) => {
            debug_assert_eq!(gaps.len(), groups.len());
            distribute(data, start, scan, &groups, gaps, sat);
        }
    }
    scan.content = scan.content.add(extra, sat);
}

#[cfg(test)]
#[path = "tests/align_count.rs"]
mod tests;
