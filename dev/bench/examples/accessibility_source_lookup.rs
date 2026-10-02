//! Separate cold and warm source inverse scopes; compact scaling output.
#[allow(dead_code)]
#[path = "support/completed_height_snapshot.rs"]
mod snapshot;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::accessibility::{AccessibleLayout, AccessiblePosition, SourcePosition};
use shodo::limits::Limits;
use shodo::mapping::{Affinity, TextOrigin};
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle, TextTransform, WhiteSpaceCollapse};
use shodo::{AtomicSizes, LayoutContext, ParagraphBuilder};
use std::hint::black_box;
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn measured<T>(f: impl FnOnce() -> T) -> (T, Value) {
    #[cfg(feature = "allocation-counting")]
    {
        let scope = ALLOC.begin().unwrap();
        let result = f();
        black_box(&result);
        let allocation = scope.finish();
        (result, json!({"allocation":allocation}))
    }
    #[cfg(not(feature = "allocation-counting"))]
    {
        let start = std::time::Instant::now();
        let result = f();
        black_box(&result);
        let ns = start.elapsed().as_nanos() as u64;
        (result, json!({"ns":ns}))
    }
}
fn positions(layout: &AccessibleLayout<'_>, ps: &[AccessiblePosition]) -> Value {
    json!(
        ps.iter()
            .map(|&p| json!({
                "text":format!("{:?}",layout.to_text_position(p)),
                "source":format!("{:?}",layout.to_source(p)),
            }))
            .collect::<Vec<_>>()
    )
}
fn main() {
    let mut rows = Vec::new();
    for (kind, count, budget) in [
        ("unique", 32, "default"),
        ("unique", 128, "default"),
        ("unique", 512, "default"),
        ("repeated", 32, "default"),
        ("repeated", 128, "default"),
        ("repeated", 512, "default"),
        ("mixed", 4, "default"),
        ("mixed", 4, "shape-zero"),
        ("mixed", 4, "warning-zero"),
        ("mixed", 4, "glyph-zero"),
        ("mixed", 4, "no-mapping"),
    ] {
        let mut limits = Limits::default();
        match budget {
            "shape-zero" => limits.max_shaping_run_bytes = Some(0),
            "warning-zero" => {
                limits.max_shaping_run_bytes = Some(0);
                limits.max_warnings = Some(0);
            }
            "glyph-zero" => limits.max_shaped_glyphs = Some(0),
            _ => {}
        }
        let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
        let root = InlineStyle {
            font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())],
            font_size: 16.0,
            white_space_collapse: if kind == "mixed" {
                WhiteSpaceCollapse::Collapse
            } else {
                WhiteSpaceCollapse::Preserve
            },
            ..Default::default()
        };
        let style = ParagraphStyle {
            root: root.clone(),
            first_line: (kind == "mixed").then(|| InlineStyle {
                text_transform: TextTransform::Uppercase,
                ..root.clone()
            }),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.with_offset_mapping(budget != "no-mapping");
        if kind == "mixed" {
            b.push_text(
                TextSource::Dom {
                    node: NodeId(7),
                    offset: 0,
                },
                "ßa  ",
            )
            .push_text(TextSource::Generated { node: NodeId(8) }, "X")
            .push_text(
                TextSource::Dom {
                    node: NodeId(7),
                    offset: 0,
                },
                "ab",
            )
            .push_forced_break(NodeId(10))
            .push_text(
                TextSource::Dom {
                    node: NodeId(7),
                    offset: 2,
                },
                "c  d",
            )
            .push_text(
                TextSource::Dom {
                    node: NodeId(9),
                    offset: 20,
                },
                "ſb",
            );
        } else {
            for node in 0..count {
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(if kind == "repeated" { 7 } else { node as u64 }),
                        offset: 0,
                    },
                    "ab",
                );
                if node + 1 < count {
                    b.push_forced_break(NodeId(10000 + node as u64));
                }
            }
        }
        let mut cx = LayoutContext::new();
        let p = match b.build(&mut cx, &fonts.collection) {
            Ok(p) => p,
            Err(error) => {
                rows.push(
                    json!({"kind":kind,"count":count,"budget":budget,"error":format!("{error:?}")}),
                );
                continue;
            }
        };
        let build_warnings = format!("{:?}", p.warnings());
        let lines = p.break_all(
            &mut cx,
            &Default::default(),
            if kind == "mixed" { 24.0 } else { 1000.0 },
            &AtomicSizes::EMPTY,
        );
        let warnings = format!("{build_warnings}/{:?}", cx.take_warnings());
        let run = shodo_bench::Run {
            lines,
            ..Default::default()
        };
        let digest = shodo_bench::digest(&run, &fonts).unwrap();
        let source = SourcePosition {
            origin: TextOrigin::Dom {
                node: NodeId(if kind == "unique" {
                    count as u64 - 1
                } else {
                    7
                }),
                offset: 1,
            },
            affinity: Affinity::Downstream,
        };
        let (layout, new_cost) = measured(|| AccessibleLayout::new(&run.lines));
        let (cold, cold_cost) = measured(|| layout.from_source(black_box(source)));
        let (warm, warm_cost): ([Vec<AccessiblePosition>; 3], _) =
            measured(|| std::array::from_fn(|_| layout.from_source(black_box(source))));
        for ps in &warm {
            assert_eq!(*ps, cold);
        }
        if kind != "mixed" {
            assert_eq!(cold.len(), if kind == "unique" { 1 } else { count });
        }
        // Source datasets are hashed once, without dumping full paragraph text
        // or mapping once per line. Cloning drops only private lazy-index state.
        let mut source_hash = Sha256::new();
        let mut datasets = Vec::new();
        for line in &run.lines {
            if let Some(mapping) = line.offset_mapping() {
                let ordinal = datasets
                    .iter()
                    .position(|m| std::ptr::eq(*m, mapping))
                    .unwrap_or_else(|| {
                        datasets.push(mapping);
                        source_hash.update(line.text().as_bytes());
                        source_hash.update(format!("{:?}", mapping.clone()).as_bytes());
                        datasets.len() - 1
                    });
                source_hash.update(format!("{ordinal}/{:?}", line.text_range()).as_bytes());
            }
        }
        let query_output = positions(&layout, &cold);
        let query_hash = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&query_output).unwrap())
        );
        let small_oracle = if kind == "mixed" {
            let queries = [Affinity::Upstream,Affinity::Downstream].into_iter().flat_map(|affinity|
                [7,9,99].into_iter().flat_map(move |node|(0..25).chain([u32::MAX]).map(move |offset|
                    SourcePosition {origin:TextOrigin::Dom {node:NodeId(node),offset},affinity})))
                .chain([Affinity::Upstream,Affinity::Downstream].into_iter().map(|affinity|SourcePosition {origin:TextOrigin::Generated {node:NodeId(8)},affinity}))
                .map(|s|json!({"input":format!("{s:?}"),"positions":positions(&layout,&layout.from_source(s))})).collect::<Vec<_>>();
            json!({"lines":run.lines.iter().map(|l|{
                let mut value=snapshot::line(l);
                value["mapping"]=json!(l.offset_mapping().map(|m|format!("{:?}",m.clone())));
                value
            }).collect::<Vec<_>>(),"queries":queries})
        } else {
            Value::Null
        };
        rows.push(json!({"kind":kind,"count":count,"budget":budget,"lines":run.lines.len(),
            "result_count":cold.len(),"warm_queries":3,"new_cost":new_cost,"cold_cost":cold_cost,"warm_cost":warm_cost,
            "oracle":{"digest":digest,"source_sha256":format!("{:x}",source_hash.finalize()),
                "query_sha256":query_hash,"warnings":warnings,"small":small_oracle}}));
    }
    println!("{}", serde_json::to_string(&rows).unwrap());
}
