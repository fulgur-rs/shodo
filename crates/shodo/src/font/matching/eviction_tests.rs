use super::*;

#[test]
fn concurrent_thrash_keeps_font_choices_after_catalog_changes() {
    use crate::font::{FontCollection, FontOptions};
    use crate::limits::Limits;
    use crate::style::{FontFamily, FontStyle, GenericFamily};
    let chars = ['a', '日', '水', '中', '文', '未'];
    let clusters = ["a", "日", "水", "中", "文", "未", "🦀"];
    for cap in [0, 1, 8, 255, 256, 257] {
        let fonts = FontCollection::with_options(
            &Limits::unlimited(),
            FontOptions {
                system_fonts: false,
                match_cache_entries: cap,
                ..Default::default()
            },
        );
        let first = fonts
            .register(super::super::browser_tests::test_font("First", &chars, 600))
            .unwrap();
        let second = fonts
            .register(super::super::browser_tests::test_font(
                "Second", &chars, 600,
            ))
            .unwrap();
        let queries: Vec<_> = (0..512)
            .map(|i| FontQuery {
                families: vec![FontFamily::Generic(GenericFamily::Serif)],
                weight: 100. + (i % 128) as f32 * 6.,
                language: Some(format!("ja-{}", i / 128)),
                style: if i % 3 == 0 {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                },
                ..Default::default()
            })
            .collect();
        for (family, want) in [("First", first.index()), ("Second", second.index())] {
            fonts.set_generic_families(GenericFamily::Serif, vec![family.into()]);
            std::thread::scope(|scope| {
                for _ in 0..4 {
                    scope.spawn(|| {
                        for _ in 0..2 {
                            for (i, q) in queries.iter().enumerate() {
                                let c = clusters[i % clusters.len()];
                                let found = fonts.match_cluster(q, c);
                                if c == "🦀" {
                                    assert!(found.is_none());
                                } else {
                                    assert_eq!(
                                        found.unwrap().id.index(),
                                        want,
                                        "cap={cap} family={family}"
                                    );
                                }
                            }
                        }
                    });
                }
            });
            assert!(fonts.caches().matches.len() <= cap);
        }
    }
}
