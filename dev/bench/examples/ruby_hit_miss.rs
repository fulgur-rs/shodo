//! Targeted retained nested-ruby misses. Build/index and snapshots stay outside query scopes.
#[path = "support/ruby_hit_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/completed_height_snapshot.rs"]
mod snapshot;
use serde_json::{Value, json};
use shodo::hit::LineLayout;
use shodo::limits::Limits;
use std::hint::black_box;
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn cost(f: impl FnOnce()) -> Value {
    #[cfg(feature = "allocation-counting")]
    {
        let scope = ALLOC.begin().unwrap();
        f();
        let allocation = scope.finish();
        json!({"allocation": allocation})
    }
    #[cfg(not(feature = "allocation-counting"))]
    {
        let start = std::time::Instant::now();
        f();
        let ns = start.elapsed().as_nanos() as u64;
        json!({"ns": ns})
    }
}
fn main() {
    let mut rows = Vec::new();
    for (budget, limits) in [
        ("default", Limits::default()),
        (
            "shape-zero",
            Limits {
                max_shaping_run_bytes: Some(0),
                ..Default::default()
            },
        ),
        (
            "warning-zero",
            Limits {
                max_shaping_run_bytes: Some(0),
                max_warnings: Some(0),
                ..Default::default()
            },
        ),
        (
            "cache-zero",
            Limits {
                max_shaper_cache_entries: Some(0),
                ..Default::default()
            },
        ),
        (
            "glyph-zero",
            Limits {
                max_shaped_glyphs: Some(0),
                ..Default::default()
            },
        ),
    ] {
        for depth in if budget == "default" {
            &[4, 8, 12, 16][..]
        } else {
            &[4][..]
        } {
            let (lines, warnings) = match fixture::fixture(*depth, &limits) {
                Ok(value) => value,
                Err(error) => {
                    rows.push(json!({"budget":budget, "depth":depth, "error":error}));
                    continue;
                }
            };
            assert_eq!(fixture::actual_depth(&lines), *depth);
            fixture::assert_real_glyphs(&lines);
            let layout = LineLayout::new(&lines);
            let mut current = &lines[0];
            let mut path = Vec::new();
            while let Some(a) = current.ruby_annotations().next() {
                path.push(a);
                current = a.line();
            }
            let run = current
                .fragments()
                .find_map(|f| match f {
                    shodo::Fragment::GlyphRun(r) => Some(r),
                    _ => None,
                })
                .unwrap();
            let mut point = (run.inline_size() / 2.0, run.baseline());
            for a in path.iter().rev() {
                let t = a.transform();
                point = (
                    t.inline_inline * point.0 + t.inline_block * point.1 + t.inline_offset,
                    t.block_inline * point.0 + t.block_block * point.1 + t.block_offset,
                );
            }
            let queries = [
                point,
                (0.01, lines[0].block_size() / 2.0),
                (1_000_000.0, 1_000_000.0),
                (f32::INFINITY, f32::INFINITY),
                (f32::NAN, 0.0),
            ];
            let hits = queries
                .into_iter()
                .map(|(x, y)| {
                    json!({
                        "main":format!("{:?}",layout.hit_test(x,y)),
                        "ruby":layout.hit_test_ruby(x,y).map(|h|json!({
                            "hit":format!("{:?}",h.hit),"parent_line":h.parent_line(),
                            "annotation":h.annotation.node().map(|n|n.0),
                            "path":h.path().iter().map(|a|a.container().0).collect::<Vec<_>>()
                        }))
                    })
                })
                .collect::<Vec<_>>();
            let count = 3;
            let ruby_cost = cost(|| {
                for _ in 0..count {
                    assert!(
                        black_box(
                            layout.hit_test_ruby(black_box(1_000_000.0), black_box(1_000_000.0))
                        )
                        .is_none()
                    );
                }
            });
            let main_cost = cost(|| {
                for _ in 0..count {
                    assert!(
                        !black_box(layout.hit_test(black_box(1_000_000.0), black_box(1_000_000.0)))
                            .unwrap()
                            .inside
                    );
                }
            });
            let ruby_hit_cost = cost(|| {
                for _ in 0..count {
                    assert!(
                        black_box(layout.hit_test_ruby(black_box(point.0), black_box(point.1)))
                            .is_some()
                    );
                }
            });
            let main_hit_cost = cost(|| {
                for _ in 0..count {
                    assert!(
                        black_box(layout.hit_test(black_box(point.0), black_box(point.1)))
                            .unwrap()
                            .inside
                    );
                }
            });
            rows.push(json!({"budget":budget,"depth":depth,"actual_depth":fixture::actual_depth(&lines),
                "count":count,"ruby_cost":ruby_cost,"main_cost":main_cost,
                "ruby_hit_cost":ruby_hit_cost,"main_hit_cost":main_hit_cost,
                "oracle":{"warnings":warnings,"hits":hits,"lines":lines.iter().map(snapshot::line).collect::<Vec<_>>()}}));
        }
    }
    println!("{}", serde_json::to_string(&rows).unwrap());
}
