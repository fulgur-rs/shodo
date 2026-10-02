//! Measure paint_spans only; shaping, layout and output hashing are outside scope.
use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::LayoutContext;
use shodo_bench::{Operation, Workload, digest, layout};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);
fn main() {
    let limits = Default::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    for id in [
        "latin-long",
        "arabic-long",
        "combining-latin",
        "nested-atomic",
        "preserved-tabs",
    ] {
        let work = Workload::named(id, 8).unwrap();
        let mut cx = LayoutContext::new();
        let paragraphs = work.build(&mut cx, &fonts, &limits).unwrap();
        let lines = layout(
            &work,
            &paragraphs,
            &mut cx,
            &fonts,
            &limits,
            Operation::AllLines,
        )
        .unwrap();
        let geometry = digest(&lines, &fonts).unwrap();
        let warnings: Vec<_> = paragraphs
            .iter()
            .map(|p| format!("{:?}", p.warnings()))
            .collect();
        let paint = || {
            lines
                .lines
                .iter()
                .map(|line| line.paint_spans())
                .collect::<Vec<_>>()
        };
        let hash = |spans: &Vec<Vec<shodo::PaintSpan<'_>>>| {
            format!("{:x}", Sha256::digest(format!("{spans:?}").as_bytes()))
        };
        let expected = hash(&paint());
        for sample in 0..9 {
            #[cfg(feature = "allocation-counting")]
            let scope = ALLOC.begin().unwrap();
            #[cfg(not(feature = "allocation-counting"))]
            let start = std::time::Instant::now();
            let spans = std::hint::black_box(paint());
            #[cfg(feature = "allocation-counting")]
            let counts = scope.finish();
            #[cfg(not(feature = "allocation-counting"))]
            let elapsed = start.elapsed().as_nanos();
            #[cfg(feature = "allocation-counting")]
            let measure = json!({"allocation": counts});
            #[cfg(not(feature = "allocation-counting"))]
            let measure = json!({"ns": elapsed});
            assert_eq!(hash(&spans), expected);
            println!(
                "{}",
                json!({"id":id,"sample":sample,"measure":measure,"paint":expected,"geometry":geometry,"warnings":warnings})
            );
        }
    }
}
