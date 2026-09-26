//! Minimal sfnt (OpenType) writer. Produces the built-in stub face and fonts
//! for tests. Checksums and search hints are left zero: nothing in shodo
//! validates them.

pub(crate) fn build_sfnt(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0u8; 6]);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        let pad = (4 - data.len() % 4) % 4;
        body.extend(std::iter::repeat_n(0u8, pad));
        offset += data.len() + pad;
    }
    out.extend_from_slice(&body);
    out
}

/// A GSUB/GPOS table whose LookupList has `lookup_count` entries that all
/// point at one Lookup, whose `subtable_count` subtable offsets all point at
/// one empty SingleSubst subtable. Tiny on disk, huge once expanded per
/// reference. Counts must be at most 16 000 so offsets fit in 16 bits.
#[cfg(test)]
pub(crate) fn amplifying_layout_table(lookup_count: u16, subtable_count: u16) -> Vec<u8> {
    assert!(lookup_count <= 16_000 && subtable_count <= 16_000);
    let mut t = Vec::new();
    let put = |t: &mut Vec<u8>, v: u16| t.extend_from_slice(&v.to_be_bytes());
    // Header: version 1.0, no ScriptList/FeatureList, LookupList at offset 10.
    for v in [1, 0, 0, 0, 10] {
        put(&mut t, v);
    }
    // LookupList: every entry points at the Lookup placed right after it.
    put(&mut t, lookup_count);
    let lookup_offset = 2 + 2 * lookup_count;
    for _ in 0..lookup_count {
        put(&mut t, lookup_offset);
    }
    // Lookup: type 1 (single substitution), flags 0.
    put(&mut t, 1);
    put(&mut t, 0);
    put(&mut t, subtable_count);
    let subtable_offset = 6 + 2 * subtable_count;
    for _ in 0..subtable_count {
        put(&mut t, subtable_offset);
    }
    // SingleSubst format 1 with an empty format-1 Coverage at offset 6.
    for v in [1, 6, 0, 1, 0] {
        put(&mut t, v);
    }
    t
}
