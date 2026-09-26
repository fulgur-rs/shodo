//! Structural check of font blobs before registration.

use std::fmt;

use crate::limits::{LimitExceeded, LimitKind, Limits};

/// Why a font blob was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontError {
    Limit(LimitExceeded),
    Malformed(&'static str),
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(e) => e.fmt(f),
            Self::Malformed(what) => write!(f, "malformed font: {what}"),
        }
    }
}

impl std::error::Error for FontError {}

impl From<LimitExceeded> for FontError {
    fn from(e: LimitExceeded) -> Self {
        Self::Limit(e)
    }
}

const TRUNCATED: FontError = FontError::Malformed("truncated or out of bounds");

fn offset(base: usize, add: usize) -> Result<usize, FontError> {
    base.checked_add(add).ok_or(TRUNCATED)
}

fn read_u16(data: &[u8], at: usize) -> Result<u16, FontError> {
    let bytes = data.get(at..offset(at, 2)?).ok_or(TRUNCATED)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], at: usize) -> Result<u32, FontError> {
    let bytes = data.get(at..offset(at, 4)?).ok_or(TRUNCATED)?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Checks a font file or collection against `limits` and returns its number
/// of faces.
pub(crate) fn check_font(data: &[u8], limits: &Limits) -> Result<u32, FontError> {
    Limits::check(
        limits.max_font_blob_bytes,
        LimitKind::FontBlobBytes,
        data.len() as u64,
    )?;
    if data.get(0..4) == Some(b"ttcf".as_slice()) {
        let count = read_u32(data, 8)?;
        Limits::check(limits.max_ttc_faces, LimitKind::TtcFaces, u64::from(count))?;
        if count == 0 {
            return Err(FontError::Malformed("empty font collection"));
        }
        for i in 0..count as usize {
            let face = read_u32(data, offset(12, 4 * i)?)? as usize;
            check_face(data, face, limits)?;
        }
        Ok(count)
    } else {
        check_face(data, 0, limits)?;
        Ok(1)
    }
}

fn check_face(data: &[u8], face: usize, limits: &Limits) -> Result<(), FontError> {
    let num_tables = read_u16(data, offset(face, 4)?)? as usize;
    let mut lookups = 0u64;
    let mut subtables = 0u64;
    for i in 0..num_tables {
        let record = offset(offset(face, 12)?, 16 * i)?;
        let tag = data.get(record..offset(record, 4)?).ok_or(TRUNCATED)?;
        let start = read_u32(data, offset(record, 8)?)? as usize;
        let len = read_u32(data, offset(record, 12)?)? as usize;
        let table = data.get(start..offset(start, len)?).ok_or(TRUNCATED)?;
        match tag {
            b"GSUB" | b"GPOS" => count_layout(table, &mut lookups, &mut subtables, limits)?,
            b"fvar" => {
                let axes = read_u16(table, 8)?;
                Limits::check(limits.max_font_axes, LimitKind::FontAxes, u64::from(axes))?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Counts lookups and subtables per reference: a Lookup referenced by N
/// LookupList entries is counted N times, and so are its subtables. This is
/// the number of cache entries the shaper creates. Extension subtables
/// resolve to exactly one subtable each, so the declared count is exact.
/// Stops at the first exceeded limit, so the walk is bounded.
fn count_layout(
    table: &[u8],
    lookups: &mut u64,
    subtables: &mut u64,
    limits: &Limits,
) -> Result<(), FontError> {
    let list = read_u16(table, 8)? as usize;
    if list == 0 {
        return Ok(());
    }
    let count = read_u16(table, list)?;
    *lookups += u64::from(count);
    Limits::check(
        limits.max_layout_lookups,
        LimitKind::LayoutLookups,
        *lookups,
    )?;
    for i in 0..count as usize {
        let lookup = offset(list, read_u16(table, offset(list, 2 + 2 * i)?)? as usize)?;
        let declared = read_u16(table, offset(lookup, 4)?)?;
        *subtables += u64::from(declared);
        Limits::check(
            limits.max_layout_subtables,
            LimitKind::LayoutSubtables,
            *subtables,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::sfnt::{amplifying_layout_table, build_sfnt};
    use crate::limits::{LimitKind, Limits};

    fn limits() -> Limits {
        Limits::default()
    }

    #[test]
    fn accepts_plain_font_and_counts_one_face() {
        let font = build_sfnt(&[(*b"GSUB", amplifying_layout_table(2, 2))]);
        assert_eq!(check_font(&font, &limits()), Ok(1));
        assert_eq!(check_font(&build_sfnt(&[]), &limits()), Ok(1));
    }

    #[test]
    fn rejects_duplicate_subtable_references() {
        // About 48 KiB of GSUB that would expand to 8192 * 8192 cache entries.
        let font = build_sfnt(&[(*b"GSUB", amplifying_layout_table(8192, 8192))]);
        let mut l = limits();
        l.max_layout_lookups = None;
        match check_font(&font, &l) {
            Err(FontError::Limit(e)) => assert_eq!(e.kind, LimitKind::LayoutSubtables),
            other => panic!("expected LayoutSubtables limit, got {other:?}"),
        }
    }

    #[test]
    fn rejects_duplicate_lookup_references() {
        let font = build_sfnt(&[(*b"GPOS", amplifying_layout_table(8192, 1))]);
        match check_font(&font, &limits()) {
            Err(FontError::Limit(e)) => assert_eq!(e.kind, LimitKind::LayoutLookups),
            other => panic!("expected LayoutLookups limit, got {other:?}"),
        }
    }

    #[test]
    fn gsub_and_gpos_share_one_budget() {
        let font = build_sfnt(&[
            (*b"GSUB", amplifying_layout_table(1, 3)),
            (*b"GPOS", amplifying_layout_table(1, 3)),
        ]);
        let mut l = limits();
        l.max_layout_subtables = Some(5);
        assert!(matches!(check_font(&font, &l), Err(FontError::Limit(_))));
        l.max_layout_subtables = Some(6);
        assert_eq!(check_font(&font, &l), Ok(1));
    }

    #[test]
    fn rejects_truncated_and_out_of_bounds_data() {
        assert!(matches!(
            check_font(&[0, 1, 0], &limits()),
            Err(FontError::Malformed(_))
        ));
        let mut font = build_sfnt(&[(*b"GSUB", amplifying_layout_table(2, 2))]);
        font.truncate(30);
        assert!(matches!(
            check_font(&font, &limits()),
            Err(FontError::Malformed(_))
        ));
    }

    #[test]
    fn enforces_blob_size_axis_and_collection_limits() {
        let mut l = limits();
        l.max_font_blob_bytes = Some(4);
        assert!(matches!(
            check_font(&build_sfnt(&[]), &l),
            Err(FontError::Limit(_))
        ));

        let mut fvar = vec![0u8; 16];
        fvar[8..10].copy_from_slice(&2u16.to_be_bytes());
        let mut l = limits();
        l.max_font_axes = Some(1);
        let font = build_sfnt(&[(*b"fvar", fvar)]);
        assert!(matches!(check_font(&font, &l), Err(FontError::Limit(_))));

        // 'ttcf' header claiming 3 faces.
        let mut ttc = b"ttcf".to_vec();
        ttc.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        ttc.extend_from_slice(&3u32.to_be_bytes());
        let mut l = limits();
        l.max_ttc_faces = Some(2);
        match check_font(&ttc, &l) {
            Err(FontError::Limit(e)) => assert_eq!(e.kind, LimitKind::TtcFaces),
            other => panic!("expected TtcFaces limit, got {other:?}"),
        }
    }

    #[test]
    fn counts_faces_in_a_collection() {
        let face = build_sfnt(&[]);
        let mut ttc = b"ttcf".to_vec();
        ttc.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        ttc.extend_from_slice(&2u32.to_be_bytes());
        let first = 12 + 8;
        ttc.extend_from_slice(&(first as u32).to_be_bytes());
        ttc.extend_from_slice(&((first + face.len()) as u32).to_be_bytes());
        ttc.extend_from_slice(&face);
        ttc.extend_from_slice(&face);
        assert_eq!(check_font(&ttc, &limits()), Ok(2));
    }
}
