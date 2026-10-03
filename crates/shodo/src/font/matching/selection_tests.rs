use super::*;

/// Independent copy of the candidate ordering that selected the head of a
/// stable sort. Permuting the survivors must never move the selection.
fn stable_sort_head(
    mut candidates: Vec<Candidate>,
    query: &FontQuery,
    color: bool,
) -> Option<Candidate> {
    let rank = |c: &Candidate| {
        (
            u8::from(c.color != color),
            range_rank(query.width, c.descriptor.width, 100.),
            style_rank(query.style, c.descriptor.style),
            weight_rank(query.weight, c.descriptor.weight),
        )
    };
    candidates.sort_by(|a, b| {
        rank(a)
            .partial_cmp(&rank(b))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.order_cmp(b))
    });
    candidates.into_iter().next()
}

/// Registration identity of a candidate, so two selections can be compared
/// without comparing fonts.
#[derive(Debug, PartialEq)]
struct Selected {
    order_key: u64,
    index: u32,
    color: bool,
    weight: (f32, f32),
    width: (f32, f32),
}

fn identity(candidate: Option<&Candidate>) -> Option<Selected> {
    candidate.map(|candidate| Selected {
        order_key: candidate.order_key.0,
        index: candidate.info.index(),
        color: candidate.color,
        weight: candidate.descriptor.weight,
        width: candidate.descriptor.width,
    })
}

/// One reusable face: its parsed metadata plus the rank inputs, so candidate
/// permutations reuse a single parse.
struct Face {
    order_key: u64,
    color: bool,
    weight: (f32, f32),
    width: (f32, f32),
    style: FontStyle,
    info: FontInfo,
    data: FontData,
}
impl Face {
    fn candidate(&self) -> Candidate {
        Candidate {
            order_key: (self.order_key, 0),
            id: FontId { layer: 0, index: 0 },
            descriptor: FontFaceDescriptor {
                family: "Web".into(),
                weight: self.weight,
                width: self.width,
                style: self.style,
                unicode_ranges: Vec::new(),
            },
            info: self.info.clone(),
            data: self.data.clone(),
            color: self.color,
        }
    }
    fn memory(
        order_key: u64,
        color: bool,
        weight: (f32, f32),
        width: (f32, f32),
        style: FontStyle,
    ) -> Self {
        let blob =
            super::super::Blob::from(super::super::browser_tests::test_font("Web", &['a'], 600));
        let info = FontInfo::from_source(
            SourceInfo::new(SourceId::new(), SourceKind::Memory(blob.clone())),
            0,
        )
        .unwrap();
        Self {
            order_key,
            color,
            weight,
            width,
            style,
            info,
            data: FontData::new(blob, 0),
        }
    }
    fn path(
        path: &std::path::Path,
        color: bool,
        weight: (f32, f32),
        width: (f32, f32),
        style: FontStyle,
    ) -> Self {
        let info = FontInfo::from_source(
            SourceInfo::new(SourceId::new(), SourceKind::Path(path.into())),
            0,
        )
        .unwrap();
        let data = FontData::new(info.load(None).unwrap(), 0);
        Self {
            order_key: path_order_key(path),
            color,
            weight,
            width,
            style,
            info,
            data,
        }
    }
}

/// Path-backed faces rank by their reverse-sorted file name; folding the file
/// name keeps their identities apart from memory registration keys.
fn path_order_key(path: &std::path::Path) -> u64 {
    path.file_name()
        .unwrap()
        .to_string_lossy()
        .bytes()
        .fold(0u64, |key, byte| {
            key.wrapping_mul(256).wrapping_add(u64::from(byte))
        })
}

fn web_fonts(dir: &std::path::Path, names: &[&str]) -> Vec<std::path::PathBuf> {
    std::fs::create_dir(dir).unwrap();
    names
        .iter()
        .map(|name| {
            let path = dir.join(name);
            std::fs::write(
                &path,
                super::super::browser_tests::test_font("Web", &['a'], 600),
            )
            .unwrap();
            path
        })
        .collect()
}

fn scratch_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "shodo-{label}-{}-{}",
        std::process::id(),
        SourceId::new().to_u64()
    ))
}

#[test]
fn best_candidate_ranks_color_width_style_then_weight() {
    let dir = scratch_dir("best-candidate");
    let paths = web_fonts(&dir, &["narrow.ttf"]);
    let pool = [
        // A weight far from the request loses to any weight that covers it.
        Face::memory(10, false, (400., 400.), (100., 100.), FontStyle::Normal),
        // Two faces cover the requested weight: the later registration wins.
        Face::memory(11, false, (700., 700.), (100., 100.), FontStyle::Normal),
        // The same rank in a color face loses to the monochrome face for text
        // presentation, and wins for emoji presentation.
        Face::memory(12, true, (700., 700.), (100., 100.), FontStyle::Normal),
        // Covering weight and width at a normal style.
        Face::memory(13, false, (700., 900.), (50., 50.), FontStyle::Normal),
        // Covering weight and width, with an oblique style an italic query
        // ranks ahead of the upright face.
        Face::memory(14, false, (700., 900.), (50., 50.), FontStyle::Oblique(14.)),
        // A weight inside 400-500 that the pivot search reaches first.
        Face::memory(15, false, (500., 500.), (100., 100.), FontStyle::Normal),
        // A file source ties with the memory face of equal rank and loses.
        Face::path(
            &paths[0],
            false,
            (700., 900.),
            (50., 50.),
            FontStyle::Normal,
        ),
    ];
    let italic = FontQuery {
        weight: 700.,
        width: 50.,
        style: FontStyle::Italic,
        ..Default::default()
    }
    .normalized();
    let normal = FontQuery {
        weight: 700.,
        width: 50.,
        ..Default::default()
    }
    .normalized();
    let pivot = FontQuery {
        weight: 450.,
        ..Default::default()
    }
    .normalized();
    for (name, query, color, expected) in [
        // An italic query ranks the oblique face (1, 3) ahead of the upright
        // face (3, 0), and text presentation ignores the color face.
        (
            "an italic query prefers an oblique face",
            &italic,
            false,
            14,
        ),
        ("emoji presentation prefers a color face", &italic, true, 12),
        ("a normal query avoids the oblique face", &normal, false, 13),
        // A 450 request searches upward through 500 first: the 500 face (1)
        // outranks the 400 face (2) and the 700 face (3).
        (
            "the 400-to-500 pivot searches upward first",
            &pivot,
            false,
            15,
        ),
    ] {
        let candidates: Vec<Candidate> = pool.iter().map(Face::candidate).collect();
        assert_eq!(
            identity(best_candidate(candidates, query, color).as_ref())
                .map(|selected| selected.order_key),
            Some(expected),
            "{name}"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn best_candidate_matches_a_stable_sort_for_every_candidate_order() {
    let dir = scratch_dir("best-candidate-order");
    let paths = web_fonts(&dir, &["a.ttf", "b.ttf"]);
    let pool = [
        Face::memory(10, false, (400., 400.), (100., 100.), FontStyle::Normal),
        Face::memory(11, false, (700., 700.), (100., 100.), FontStyle::Normal),
        Face::memory(12, true, (700., 700.), (100., 100.), FontStyle::Normal),
        Face::memory(13, false, (700., 900.), (50., 50.), FontStyle::Normal),
        Face::path(
            &paths[1],
            false,
            (700., 900.),
            (50., 50.),
            FontStyle::Normal,
        ),
        Face::path(
            &paths[0],
            false,
            (700., 900.),
            (50., 50.),
            FontStyle::Normal,
        ),
    ];
    let queries = [
        FontQuery::default().normalized(),
        FontQuery {
            weight: 450.,
            ..Default::default()
        }
        .normalized(),
        FontQuery {
            weight: 700.,
            width: 50.,
            style: FontStyle::Italic,
            ..Default::default()
        }
        .normalized(),
        FontQuery {
            weight: 1000.,
            width: 200.,
            style: FontStyle::Oblique(-20.),
            presentation: FontPresentation::Emoji,
            ..Default::default()
        }
        .normalized(),
    ];
    // Every permutation of the pool, so a head that depends on the survivor
    // order rather than on their ranks fails here.
    let mut orders: Vec<Vec<usize>> = vec![Vec::new()];
    for face in 0..pool.len() {
        let mut extended = Vec::new();
        for order in &orders {
            for at in 0..=order.len() {
                let mut order = order.clone();
                order.insert(at, face);
                extended.push(order);
            }
        }
        orders = extended;
    }
    assert_eq!(orders.len(), 720);
    for query in &queries {
        for color in [false, true] {
            for order in &orders {
                let build = || order.iter().map(|&face| pool[face].candidate()).collect();
                assert_eq!(
                    identity(best_candidate(build(), query, color).as_ref()),
                    identity(stable_sort_head(build(), query, color).as_ref()),
                    "{query:?} color={color} order={order:?}"
                );
            }
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
