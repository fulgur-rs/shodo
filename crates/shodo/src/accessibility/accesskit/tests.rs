use super::{AccessKitAdapter, NodeSemantics, types};
use crate::accessibility::AccessibleLayout;
use crate::font::FontCollection;
use crate::geometry::PhysicalRect;
use crate::limits::Limits;
use crate::mapping::{Affinity, TextOrigin, tests as lookup_work};
use crate::node::{NodeId, TextSource};
use crate::style::ParagraphStyle;
use crate::{AtomicSizes, LayoutContext, ParagraphBuilder};

#[test]
fn exporting_many_dom_runs_does_not_scan_all_sources_per_anchor() {
    // Restoring a linear text_to_dom scan must fail the work bound, even when
    // every exported value and source position remains correct. Exercise the
    // real public adapter with cold and reused mappings, rather than its keys.
    for count in [256, 512, 1024, 2048] {
        let limits = Limits::default();
        let style = ParagraphStyle::default();
        let mut builder = ParagraphBuilder::new(&style, &limits);
        for node in 0..count {
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(node as u64),
                    offset: 0,
                },
                "a",
            );
        }
        let paragraph = builder
            .build(&mut LayoutContext::new(), &FontCollection::new(&limits))
            .unwrap();
        let lines = paragraph.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1_000_000.0,
            &AtomicSizes::EMPTY,
        );
        let layout = AccessibleLayout::new(&lines);
        assert_eq!(layout.lines().len(), 1);
        assert_eq!(layout.lines()[0].runs.len(), count);
        let mut adapter = AccessKitAdapter::new(types::NodeId(1));
        let mut next_id = 1;
        let mut previous_ids = None;
        for pass in 0..2 {
            lookup_work::reset_visits();
            let update = adapter
                .update(
                    &layout,
                    types::Node::new(types::Role::Document),
                    PhysicalRect {
                        x: 0.0,
                        y: 0.0,
                        width: 1_000_000.0,
                        height: 100.0,
                    },
                    None,
                    |_| NodeSemantics::default(),
                    || {
                        next_id += 1;
                        types::NodeId(next_id)
                    },
                )
                .unwrap();
            let visits = lookup_work::visits();
            eprintln!("{count} DOM runs, export {pass}: {visits} query visits");
            // Count query comparisons, not allocator work or lazy index build.
            // The lower bound prevents a bypassed lookup path passing silently.
            assert!(visits >= count, "missing source queries: {visits}");
            assert!(
                visits < 128 * count,
                "{count} runs, pass {pass}: source queries visited {visits} records"
            );
            let runs: Vec<_> = update
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == types::Role::TextRun)
                .collect();
            assert_eq!(runs.len(), count);
            for (node, (id, run)) in runs.iter().enumerate() {
                assert_eq!(run.value(), Some("a"));
                let at = adapter
                    .from_position(
                        types::TextPosition {
                            node: *id,
                            character_index: 0,
                        },
                        Affinity::Downstream,
                    )
                    .unwrap();
                assert_eq!(
                    layout.to_source(at).unwrap().origin,
                    TextOrigin::Dom {
                        node: NodeId(node as u64),
                        offset: 0,
                    }
                );
            }
            let ids: Vec<_> = runs.iter().map(|(id, _)| *id).collect();
            if let Some(previous) = previous_ids {
                assert_eq!(ids, previous);
            }
            previous_ids = Some(ids);
        }
    }
}
