use super::*;
use crate::style::FontFamily;
use std::sync::{Barrier, mpsc};
use std::time::Duration;

fn collections() -> (FontCollection, FontCollection, FontQuery, FontId) {
    let shared = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let id = shared
        .register_face(
            browser_tests::test_font("Concurrent", &['a', '0', '水'], 600),
            0,
            FontFaceDescriptor {
                family: "Concurrent".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let document = FontCollection::for_document(&shared, &Limits::default());
    let query = FontQuery {
        families: vec![FontFamily::Named("Concurrent".into())],
        ..Default::default()
    };
    (shared, document, query, id)
}

fn warm(fonts: &FontCollection, query: &FontQuery, id: FontId) -> Arc<harfrust::ShaperData> {
    assert_eq!(fonts.match_cluster(query, "a").unwrap().id, id);
    assert!(fonts.match_cluster(query, "z").is_none());
    for unit in [fonts.resolve_ch(query, 10.), fonts.resolve_ic(query, 10.)] {
        assert_eq!(
            unit,
            FontUnit {
                id: Some(id),
                advance: 6.
            }
        );
    }
    fonts.shaper_data(id).unwrap()
}

// Reintroducing an exclusive lock on any warm hit blocks the workers while
// the caller holds its guard. Release that guard before joining or asserting,
// so a regression fails with a timeout rather than hanging the test process.
fn assert_warm_hits_before_release(
    fonts: &FontCollection,
    query: &FontQuery,
    id: FontId,
    shaper: &Arc<harfrust::ShaperData>,
    release: impl FnOnce(),
) {
    let barrier = Arc::new(Barrier::new(5));
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..4 {
            let fonts = fonts.clone();
            let query = query.clone();
            let shaper = shaper.clone();
            let barrier = barrier.clone();
            let tx = tx.clone();
            workers.push(scope.spawn(move || {
                barrier.wait();
                let mut valid = true;
                for _ in 0..32 {
                    let found = fonts.match_cluster(&query, "a");
                    let missing = fonts.match_cluster(&query, "z");
                    let ch = fonts.resolve_ch(&query, 10.);
                    let ic = fonts.resolve_ic(&query, 10.);
                    let handle = fonts.shaper_data(id);
                    valid &= found.is_some_and(|found| found.id == id)
                        && missing.is_none()
                        && ch
                            == (FontUnit {
                                id: Some(id),
                                advance: 6.,
                            })
                        && ic
                            == (FontUnit {
                                id: Some(id),
                                advance: 6.,
                            })
                        && handle.is_some_and(|handle| Arc::ptr_eq(&shaper, &handle));
                }
                let _ = tx.send(());
                valid
            }));
        }
        barrier.wait();
        let mut completed = 0;
        while completed < workers.len() && rx.recv_timeout(Duration::from_secs(2)).is_ok() {
            completed += 1;
        }
        release();
        let mut valid = true;
        for worker in workers {
            valid &= worker.join().unwrap();
        }
        assert!(
            valid,
            "parallel hits changed the font result or retained shaper identity"
        );
        assert_eq!(completed, 4, "warm cache hits waited for an exclusive lock");
    });
}

#[test]
fn warm_cache_hits_do_not_wait_for_the_font_catalog() {
    let (shared, document, query, id) = collections();
    for fonts in [&shared, &document] {
        let shaper = warm(fonts, &query, id);
        let guard = fonts.state();
        assert_warm_hits_before_release(fonts, &query, id, &shaper, || drop(guard));
    }
}

#[test]
fn warm_cache_hits_share_read_guards_in_both_layers() {
    let (shared, document, query, id) = collections();
    for fonts in [&shared, &document] {
        let shaper = warm(fonts, &query, id);
        let guard = fonts.caches();
        let shared_guard = shared.caches();
        assert_warm_hits_before_release(fonts, &query, id, &shaper, || {
            drop(guard);
            drop(shared_guard);
        });
    }
}

#[test]
fn poisoned_cache_writer_preserves_hits_and_accepts_new_units() {
    let (shared, document, query, id) = collections();
    for fonts in [&shared, &document] {
        let shaper = warm(fonts, &query, id);
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = fonts.write_caches();
            panic!("poison cache writer");
        }));
        assert!(poisoned.is_err());
        let guard = fonts.state();
        assert_warm_hits_before_release(fonts, &query, id, &shaper, || drop(guard));
        assert_eq!(
            fonts.resolve_ch(&query, 20.),
            FontUnit {
                id: Some(id),
                advance: 12.
            }
        );
    }
}

#[test]
fn concurrent_first_shaper_misses_retain_one_shared_handle() {
    // Repeated cold collections exercise misses, rather than warmed lookups.
    // Shared and document callers must converge on the same retained handle.
    for _ in 0..8 {
        let (shared, document, _, id) = collections();
        let barrier = Arc::new(Barrier::new(9));
        let handles = std::thread::scope(|scope| {
            let mut workers = Vec::new();
            for index in 0..8 {
                let fonts = if index % 2 == 0 { &shared } else { &document }.clone();
                let barrier = barrier.clone();
                workers.push(scope.spawn(move || {
                    barrier.wait();
                    fonts.shaper_data(id).unwrap()
                }));
            }
            barrier.wait();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        for handle in &handles[1..] {
            assert!(Arc::ptr_eq(&handles[0], handle));
        }
        assert_eq!(shared.caches().shapers.len(), 1);
    }
}
