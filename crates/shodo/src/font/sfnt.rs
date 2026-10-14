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

/// The tables of a single-face SFNT in directory order: the inverse of
/// `build_sfnt`, so tests can replace or add tables of a fixture font.
#[cfg(test)]
pub(crate) fn read_sfnt_tables(font: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let be32 = |at: usize| u32::from_be_bytes(font[at..at + 4].try_into().unwrap()) as usize;
    let count = u16::from_be_bytes(font[4..6].try_into().unwrap()) as usize;
    (0..count)
        .map(|n| {
            let record = 12 + 16 * n;
            let (offset, len) = (be32(record + 8), be32(record + 12));
            (
                font[record..record + 4].try_into().unwrap(),
                font[offset..offset + len].to_vec(),
            )
        })
        .collect()
}

/// An `fvar` table with the given axes as `(tag, [min, default, max])` in
/// whole units, and no named instances.
#[cfg(test)]
pub(crate) fn fvar_table(axes: &[([u8; 4], [i32; 3])]) -> Vec<u8> {
    let mut fvar = Vec::new();
    for field in [1u16, 0, 16, 2, axes.len() as u16, 20, 0, 8] {
        fvar.extend(field.to_be_bytes());
    }
    for (tag, values) in axes {
        fvar.extend(tag);
        for value in values {
            fvar.extend((value << 16).to_be_bytes());
        }
        // Axis flags 0, axis name ID 256.
        fvar.extend([0, 0, 1, 0]);
    }
    fvar
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
