//! Bound the work performed when harfrust creates font caches. Repeated
//! references are intentionally charged repeatedly, just like the shaper.

use super::{
    FontError,
    check::{TRUNCATED, offset, read_u16, read_u32},
};
use crate::limits::{LimitKind, Limits};

fn charge(work: &mut u64, amount: u64, limits: &Limits) -> Result<(), FontError> {
    *work = work.checked_add(amount).ok_or(TRUNCATED)?;
    Limits::check(
        limits.max_font_cache_items,
        LimitKind::FontCacheItems,
        *work,
    )?;
    Ok(())
}
fn coverage(data: &[u8], at: usize, work: &mut u64, limits: &Limits) -> Result<(), FontError> {
    let format = read_u16(data, at)?;
    let count = read_u16(data, offset(at, 2)?)? as usize;
    match format {
        1 => {
            charge(work, count as u64, limits)?;
            data.get(at..offset(at, 4 + 2 * count)?).ok_or(TRUNCATED)?;
        }
        2 => {
            // Charge declared records before walking them, then their expanded
            // glyph ranges. The record walk cannot exceed the same work budget.
            Limits::check(
                limits.max_font_cache_items,
                LimitKind::FontCacheItems,
                *work + count as u64,
            )?;
            let mut previous = None;
            for index in 0..count {
                let record = offset(at, 4 + 6 * index)?;
                let first = read_u16(data, record)?;
                let last = read_u16(data, offset(record, 2)?)?;
                read_u16(data, offset(record, 4)?)?;
                if first > last || previous.is_some_and(|p| first <= p) {
                    return Err(FontError::Malformed("unordered coverage range"));
                }
                charge(work, u64::from(last - first) + 1, limits)?;
                previous = Some(last);
            }
        }
        _ => return Err(FontError::Malformed("unknown coverage format")),
    }
    Ok(())
}
fn class_def(data: &[u8], at: usize, work: &mut u64, limits: &Limits) -> Result<(), FontError> {
    match read_u16(data, at)? {
        1 => {
            let count = read_u16(data, offset(at, 4)?)? as usize;
            charge(work, count as u64, limits)?;
            data.get(at..offset(at, 6 + 2 * count)?).ok_or(TRUNCATED)?;
        }
        2 => coverage(data, at, work, limits)?, // Same range layout; third word is class.
        _ => return Err(FontError::Malformed("unknown class definition format")),
    }
    Ok(())
}
fn coverage_ref(
    data: &[u8],
    base: usize,
    field: usize,
    work: &mut u64,
    limits: &Limits,
) -> Result<(), FontError> {
    let relative = read_u16(data, offset(base, field)?)? as usize;
    if relative == 0 {
        return Err(FontError::Malformed("null coverage reference"));
    }
    coverage(data, offset(base, relative)?, work, limits)
}

pub(super) fn check_subtable(
    data: &[u8],
    at: usize,
    kind: u16,
    is_subst: bool,
    work: &mut u64,
    limits: &Limits,
) -> Result<(), FontError> {
    let format = read_u16(data, at)?;
    let extension = if is_subst { 7 } else { 9 };
    if kind == extension {
        if format != 1 {
            return Err(FontError::Malformed("unknown extension format"));
        }
        let target_kind = read_u16(data, offset(at, 2)?)?;
        if target_kind == extension {
            return Err(FontError::Malformed("recursive layout extension"));
        }
        let target = offset(at, read_u32(data, offset(at, 4)?)? as usize)?;
        return check_subtable(data, target, target_kind, is_subst, work, limits);
    }
    let context = if is_subst { 5 } else { 7 };
    let chained = if is_subst { 6 } else { 8 };
    if kind == context || kind == chained {
        match format {
            1 => coverage_ref(data, at, 2, work, limits)?,
            2 => {
                coverage_ref(data, at, 2, work, limits)?;
                for field in if kind == context {
                    &[4][..]
                } else {
                    &[4, 6, 8][..]
                } {
                    let relative = read_u16(data, offset(at, *field)?)? as usize;
                    if relative != 0 {
                        class_def(data, offset(at, relative)?, work, limits)?;
                    }
                }
            }
            3 => {
                let mut field = 2;
                for _ in 0..if kind == context { 1 } else { 3 } {
                    let count = read_u16(data, offset(at, field)?)? as usize;
                    charge(work, count as u64, limits)?;
                    field += if kind == context { 4 } else { 2 };
                    for i in 0..count {
                        coverage_ref(data, at, field + 2 * i, work, limits)?;
                    }
                    field += 2 * count;
                }
            }
            _ => return Err(FontError::Malformed("unknown context format")),
        }
    } else if if is_subst {
        (1..=4).contains(&kind) || kind == 8
    } else {
        (1..=6).contains(&kind)
    } {
        coverage_ref(data, at, 2, work, limits)?;
        if !is_subst && kind == 2 && format == 2 {
            for field in [8, 10] {
                let relative = read_u16(data, offset(at, field)?)? as usize;
                if relative != 0 {
                    class_def(data, offset(at, relative)?, work, limits)?;
                }
            }
        }
        if !is_subst && (4..=6).contains(&kind) {
            coverage_ref(data, at, 4, work, limits)?;
        }
        if is_subst && kind == 8 {
            let mut field = 4;
            for _ in 0..2 {
                let count = read_u16(data, offset(at, field)?)? as usize;
                charge(work, count as u64, limits)?;
                field += 2;
                for i in 0..count {
                    coverage_ref(data, at, field + 2 * i, work, limits)?;
                }
                field += 2 * count;
            }
        }
    }
    Ok(())
}

pub(super) fn check_gdef(data: &[u8], work: &mut u64, limits: &Limits) -> Result<(), FontError> {
    for field in [4, 10] {
        let relative = read_u16(data, field)? as usize;
        if relative != 0 {
            class_def(data, relative, work, limits)?;
        }
    }
    let version = read_u32(data, 0)?;
    if version >= 0x0001_0002 {
        let base = read_u16(data, 12)? as usize;
        if base != 0 {
            let count = read_u16(data, offset(base, 2)?)? as usize;
            // Harfrust reserves one digest per declared mark set, even when
            // the coverage itself is malformed or empty. Charge the references
            // cumulatively so repeated GDEF tables cannot restart this work.
            Limits::check(
                limits.max_layout_subtables,
                LimitKind::LayoutSubtables,
                count as u64,
            )?;
            charge(work, count as u64, limits)?;
            for i in 0..count {
                let at = offset(base, read_u32(data, offset(base, 4 + 4 * i)?)? as usize)?;
                coverage(data, at, work, limits)?;
            }
        }
    }
    Ok(())
}

pub(super) fn check_aat(
    data: &[u8],
    tag: &[u8],
    subtables: &mut u64,
    work: &mut u64,
    limits: &Limits,
) -> Result<(), FontError> {
    let old_kern = tag == b"kern" && read_u16(data, 0)? == 0;
    let count = if old_kern {
        u32::from(read_u16(data, 2)?)
    } else {
        read_u32(data, 4)?
    };
    *subtables += u64::from(count);
    Limits::check(
        limits.max_layout_subtables,
        LimitKind::LayoutSubtables,
        *subtables,
    )?;
    let mut at = if old_kern { 4 } else { 8 };
    for _ in 0..count {
        if tag == b"morx" {
            let length = read_u32(data, offset(at, 4)?)? as usize;
            let features = read_u32(data, offset(at, 8)?)?;
            let declared = read_u32(data, offset(at, 12)?)?;
            *subtables += u64::from(declared);
            Limits::check(
                limits.max_layout_subtables,
                LimitKind::LayoutSubtables,
                *subtables,
            )?;
            charge(
                work,
                u64::from(features) + u64::from(declared) * 65536,
                limits,
            )?;
            if length < 16 || 16u64 + u64::from(features) * 12 > length as u64 {
                return Err(TRUNCATED);
            }
            data.get(at..offset(at, length)?).ok_or(TRUNCATED)?;
            at = offset(at, length)?;
        } else {
            // Each AAT glyph set is bounded by the 16-bit glyph space. Two
            // such sets per kern/kerx subtable bound its cache footprint.
            charge(work, 2 * 65536, limits)?;
            let length = if old_kern {
                read_u16(data, offset(at, 2)?)? as usize
            } else {
                read_u32(data, at)? as usize
            };
            if length < if old_kern { 6 } else { 8 } {
                return Err(TRUNCATED);
            }
            data.get(at..offset(at, length)?).ok_or(TRUNCATED)?;
            at = offset(at, length)?;
        }
    }
    Ok(())
}
