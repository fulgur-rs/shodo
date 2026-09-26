use super::*;

/// Repository-owned synthetic font, deliberately independent of installed fonts.
fn test_font(family: &str, chars: &[char], width: u16) -> Vec<u8> {
    let mut head = vec![0; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    let mut hhea = vec![0; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea[4..6].copy_from_slice(&750i16.to_be_bytes());
    hhea[6..8].copy_from_slice(&(-250i16).to_be_bytes());
    hhea[8..10].copy_from_slice(&100i16.to_be_bytes());
    let count = chars.len() as u16 + 1;
    hhea[34..36].copy_from_slice(&count.to_be_bytes());
    let mut maxp = 0x0000_5000u32.to_be_bytes().to_vec();
    maxp.extend_from_slice(&count.to_be_bytes());
    let mut hmtx = Vec::new();
    for _ in 0..count {
        hmtx.extend_from_slice(&width.to_be_bytes());
        hmtx.extend_from_slice(&0u16.to_be_bytes());
    }
    // Format 12: one single-character group for each sorted code point.
    let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12, 0, 12, 0, 0];
    cmap.extend_from_slice(&(16u32 + 12 * chars.len() as u32).to_be_bytes());
    cmap.extend_from_slice(&0u32.to_be_bytes());
    cmap.extend_from_slice(&(chars.len() as u32).to_be_bytes());
    let mut chars = chars.to_vec();
    chars.sort();
    chars.dedup();
    for (index, ch) in chars.iter().enumerate() {
        cmap.extend_from_slice(&(*ch as u32).to_be_bytes());
        cmap.extend_from_slice(&(*ch as u32).to_be_bytes());
        cmap.extend_from_slice(&(index as u32 + 1).to_be_bytes());
    }
    let names = [
        (1, family.to_owned()),
        (2, "Regular".into()),
        (4, format!("{family} Regular")),
        (6, format!("{family}-Regular")),
    ];
    let mut name = vec![0, 0, 0, 4, 0, 54];
    let mut strings = Vec::new();
    for (id, value) in names {
        let encoded: Vec<_> = value.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for field in [
            3u16,
            1,
            0x0409,
            id,
            encoded.len() as u16,
            strings.len() as u16,
        ] {
            name.extend_from_slice(&field.to_be_bytes());
        }
        strings.extend(encoded);
    }
    name.extend(strings);
    sfnt::build_sfnt(&[
        (*b"cmap", cmap),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"maxp", maxp),
        (*b"name", name),
    ])
}

fn descriptor(family: &str) -> FontFaceDescriptor {
    FontFaceDescriptor {
        family: family.into(),
        ..Default::default()
    }
}

#[test]
fn css_face_registration_preserves_ranges_and_selected_face() {
    let fonts = FontCollection::new(&Limits::default());
    let mut desc = descriptor("Web");
    desc.weight = (300., 700.);
    desc.unicode_ranges = vec![(0x30, 0x39)];
    let id = fonts
        .register_face(test_font("Internal", &['0', 'a'], 600), 0, desc.clone())
        .unwrap();
    assert_eq!(fonts.face_descriptor(id), Some(desc));
    assert_eq!(fonts.font_data(id).unwrap().index, 0);
    assert_eq!(fonts.generation(), 1);
}

#[test]
fn invalid_css_descriptors_and_face_indices_are_transactional() {
    let fonts = FontCollection::new(&Limits::default());
    let bytes = test_font("Internal", &['a'], 600);
    assert!(
        fonts
            .register_face(bytes.clone(), 1, descriptor("Web"))
            .is_err()
    );
    for weight in [(700., 300.), (f32::NAN, 400.), (0., 400.), (400., 1001.)] {
        let mut desc = descriptor("Web");
        desc.weight = weight;
        assert!(fonts.register_face(bytes.clone(), 0, desc).is_err());
    }
    for range in [(0x110000, 0x110001), (90, 65)] {
        let mut desc = descriptor("Web");
        desc.unicode_ranges = vec![range];
        assert!(fonts.register_face(bytes.clone(), 0, desc).is_err());
    }
    assert!(
        fonts
            .register_face(bytes.clone(), 0, descriptor(" "))
            .is_err()
    );
    assert_eq!(fonts.generation(), 0);
    assert_eq!(
        fonts
            .register_face(bytes, 0, descriptor("Web"))
            .unwrap()
            .index(),
        1
    );
}

#[test]
fn css_registration_obeys_layer_budgets_and_isolation() {
    let shared = FontCollection::new(&Limits::default());
    let limits = Limits {
        max_faces_per_layer: Some(1),
        ..Default::default()
    };
    let doc = FontCollection::for_document(&shared, &limits);
    let other = FontCollection::for_document(&shared, &Limits::default());
    let bytes = test_font("Internal", &['a'], 600);
    let id = doc
        .register_face(bytes.clone(), 0, descriptor("Web"))
        .unwrap();
    assert!(other.font_data(id).is_none());
    assert!(shared.face_descriptor(id).is_none());
    assert!(matches!(
        doc.register_face(bytes, 0, descriptor("Web")),
        Err(FontError::Limit(_))
    ));
    assert_eq!(doc.generation(), 1);
}

#[test]
fn css_registration_selects_ttc_face_without_retaining_other_faces() {
    let first = test_font("First", &['a'], 400);
    let second = test_font("Second", &['b'], 700);
    let offsets = [20usize, 20 + first.len()];
    let mut ttc = b"ttcf\0\x01\0\0\0\0\0\x02".to_vec();
    for offset in offsets {
        ttc.extend_from_slice(&(offset as u32).to_be_bytes());
    }
    for (mut face, offset) in [(first, offsets[0]), (second, offsets[1])] {
        let count = u16::from_be_bytes([face[4], face[5]]) as usize;
        for table in 0..count {
            let at = 12 + table * 16 + 8;
            let value = u32::from_be_bytes(face[at..at + 4].try_into().unwrap());
            face[at..at + 4].copy_from_slice(&(value + offset as u32).to_be_bytes());
        }
        ttc.extend(face);
    }
    let shared = FontCollection::new(&Limits::default());
    let limits = Limits {
        max_faces_per_layer: Some(1),
        ..Default::default()
    };
    let doc = FontCollection::for_document(&shared, &limits);
    let id = doc.register_face(ttc, 1, descriptor("Web")).unwrap();
    assert_eq!(fonts_data_index(&doc, id), 1);
    assert_eq!(id.index(), 0);
}

fn fonts_data_index(fonts: &FontCollection, id: FontId) -> u32 {
    fonts.font_data(id).unwrap().index
}
