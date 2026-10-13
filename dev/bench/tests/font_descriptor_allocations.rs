#![cfg(feature = "allocation-counting")]

use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions, FontQuery};
use shodo::limits::Limits;
use shodo::style::FontFamily;
use shodo_bench::allocator::CountingAllocator;

#[global_allocator]
static ALLOC: CountingAllocator<std::alloc::System> = CountingAllocator::new(std::alloc::System);

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn prepared_static_matches_do_not_copy_the_descriptor_range_payload() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let fonts = FontCollection::with_options(
        &Limits::unlimited(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: 1,
            ..Default::default()
        },
    );
    let id = fonts
        .register_face(
            shodo_fixtures::font("latin").unwrap().bytes.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Descriptor".into(),
                unicode_ranges: vec![(0, 0x10ffff); 1024],
                ..Default::default()
            },
        )
        .unwrap();
    let query = FontQuery {
        families: vec![FontFamily::Named("Descriptor".into())],
        ..Default::default()
    };
    for cluster in ["a", "b", "a", "b"] {
        assert_eq!(fonts.match_cluster(&query, cluster).unwrap().id, id);
    }
    // Capacity one makes alternating clusters miss both match caches, while
    // the family/property candidate preparation stays warm. Registration and
    // candidate payload retention are outside the allocation scope.
    let scope = ALLOC.begin().unwrap();
    for cluster in ["a", "b", "a", "b", "a", "b", "a", "b"] {
        let found = fonts.match_cluster(&query, cluster).unwrap();
        assert_eq!(found.id, id);
        assert!(found.variations.is_empty());
    }
    let counts = scope.finish();
    // Ordinary query/cache result ownership fits well below one copy of the
    // 8 KiB descriptor. Cloning it per match exceeds this budget eightfold.
    assert!(
        counts.allocated_bytes < 8192,
        "eight warmed matches allocated {} bytes in {} calls",
        counts.allocated_bytes,
        counts.calls,
    );
}

#[test]
fn oversized_descriptors_are_shared_when_prepared_and_cluster_caches_cannot_help() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let long = format!("a{}", "\u{200d}".repeat(4096));
    for cache in [0, 8] {
        let fonts = FontCollection::with_options(
            &Limits::unlimited(),
            FontOptions {
                system_fonts: false,
                match_cache_entries: cache,
                ..Default::default()
            },
        );
        let id = fonts
            .register_face(
                shodo_fixtures::font("latin").unwrap().bytes.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Descriptor".into(),
                    unicode_ranges: vec![(0, 0x10ffff); 8193],
                    ..Default::default()
                },
            )
            .unwrap();
        let mut query = FontQuery {
            families: vec![FontFamily::Named("Descriptor".into())],
            ..Default::default()
        };
        for cluster in ["a", long.as_str()] {
            // Initialize the fixed per-thread result-cache table outside the
            // scope. Distinct weights below still force fresh matching work.
            assert_eq!(fonts.match_cluster(&query, cluster).unwrap().id, id);
            let scope = ALLOC.begin().unwrap();
            for weight in 401..409 {
                query.weight = weight as f32;
                assert_eq!(fonts.match_cluster(&query, cluster).unwrap().id, id);
            }
            let counts = scope.finish();
            // A single copy of the raw range list needs over 64 KiB. All
            // ordinary candidate/query/long-cluster scratch stays below this.
            assert!(
                counts.allocated_bytes < 16 * 1024,
                "cache={cache}, long={}: {} bytes in {} calls",
                cluster.len() > 4096,
                counts.allocated_bytes,
                counts.calls
            );
        }
    }
}
