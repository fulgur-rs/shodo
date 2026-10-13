#![cfg(feature = "allocation-counting")]

use shodo::font::{FontCollection, FontOptions};
use shodo::limits::{Limits, WarningKind};
use shodo::node::NodeId;
use shodo::ruby::{Ruby, RubyAlign, RubyBase, RubyContent};
use shodo::style::{InlineStyle, ParagraphStyle};
use shodo::{LayoutContext, ParagraphBuilder};
use shodo_bench::allocator::{AllocationCounts, CountingAllocator};

#[global_allocator]
static ALLOC: CountingAllocator<std::alloc::System> = CountingAllocator::new(std::alloc::System);

fn content(warnings: usize) -> RubyContent {
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    for _ in 0..warnings {
        b.close_inline();
    }
    RubyContent::from_builder(b)
}

fn import(source: &RubyContent, cap: u64, prefilled: bool) -> (AllocationCounts, ParagraphBuilder) {
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle::default(),
        &Limits {
            max_warnings: Some(cap),
            ..Default::default()
        },
    );
    if prefilled {
        for _ in 0..=cap {
            b.close_inline();
        }
    }
    let ruby = Ruby::new(
        vec![
            RubyBase {
                node: NodeId(1),
                content: source.clone(),
                align: RubyAlign::default(),
            };
            32
        ],
        vec![],
    )
    .unwrap();
    let style = InlineStyle::default();
    // Snapshot messages, Arc aliases, parent and Ruby vectors are all set up
    // outside measurement. Only normal append/import work is measured.
    let scope = ALLOC.begin().unwrap();
    b.push_ruby(NodeId(2), &style, ruby);
    let counts = scope.finish();
    assert!(b.error().is_none());
    (counts, b)
}

#[test]
fn warning_rich_aliases_only_allocate_for_the_admitted_parent_prefix() {
    let clean = content(0);
    // Public API trigger: empty content with the default 1,024 warnings plus
    // its source suppression marker, cheaply shared across 32 base columns.
    let rich = content(1025);
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for cap in [0, 3] {
        for prefilled in [false, true] {
            let (plain_counts, plain) = import(&clean, cap, prefilled);
            let (rich_counts, rich_builder) = import(&rich, cap, prefilled);
            if prefilled {
                // Already suppressed: exactly the ordinary clean import cost,
                // independent of the source warning count and alias count.
                assert_eq!(
                    rich_counts.calls, plain_counts.calls,
                    "cap={cap}: {rich_counts:?}/{plain_counts:?}"
                );
                assert_eq!(rich_counts.allocated_bytes, plain_counts.allocated_bytes);
            } else {
                // At most three retained String copies, one marker and sink
                // Vec storage. One dropped source batch alone exceeds this.
                assert!(
                    rich_counts.calls <= plain_counts.calls + 8,
                    "cap={cap}: {rich_counts:?}/{plain_counts:?}"
                );
                assert!(
                    rich_counts.allocated_bytes <= plain_counts.allocated_bytes + 512,
                    "cap={cap}: {rich_counts:?}/{plain_counts:?}"
                );
            }
            let plain = plain.build(&mut LayoutContext::new(), &fonts).unwrap();
            let rich = rich_builder
                .build(&mut LayoutContext::new(), &fonts)
                .unwrap();
            assert_eq!(rich.text(), plain.text());
            assert_eq!(rich.warnings().len(), cap as usize + 1);
            assert!(
                rich.warnings()[..cap as usize]
                    .iter()
                    .all(|w| w.kind == WarningKind::UnbalancedInline
                        && w.message == "close_inline without open_inline ignored")
            );
            assert_eq!(
                rich.warnings().last().unwrap().kind,
                WarningKind::Suppressed
            );
            assert_eq!(
                rich.warnings().last().unwrap().message,
                "further warnings suppressed"
            );
            if prefilled {
                assert_eq!(rich.warnings(), plain.warnings());
            } else {
                assert!(plain.warnings().is_empty());
            }
        }
    }
}
