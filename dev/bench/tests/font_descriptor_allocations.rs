#![cfg(feature = "allocation-counting")]

use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions, FontQuery};
use shodo::limits::Limits;
use shodo::style::FontFamily;
use shodo_bench::allocator::CountingAllocator;

#[global_allocator]
static ALLOC: CountingAllocator<std::alloc::System> = CountingAllocator::new(std::alloc::System);

#[test]
fn prepared_static_matches_do_not_copy_the_descriptor_range_payload() {
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
