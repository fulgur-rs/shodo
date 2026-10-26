#![cfg(feature = "allocation-counting")]

use shodo::ParagraphBuilder;
use shodo::limits::{LimitKind, Limits};
use shodo::node::{InlineEdges, NodeId};
use shodo::ruby::*;
use shodo::style::{InlineStyle, ParagraphStyle};
use shodo_bench::allocator::CountingAllocator;
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

fn input(shared: &RubyContent, bases: bool) -> Ruby {
    let base = RubyBase {
        node: NodeId(1),
        content: shared.clone(),
        align: RubyAlign::default(),
    };
    if bases {
        Ruby::new(vec![base; 128], vec![]).unwrap()
    } else {
        Ruby::new(
            vec![],
            vec![RubyLevel {
                annotations: vec![
                    RubyAnnotation {
                        node: NodeId(2),
                        content: shared.clone(),
                        span: RubySpan::Auto,
                        visibility: RubyVisibility::Hidden
                    };
                    128
                ],
                style: RubyStyle::default(),
            }],
        )
        .unwrap()
    }
}

#[test]
fn aliased_payload_is_rejected_without_normalization_or_payload_allocations() {
    let style = InlineStyle::default();
    let mut source = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
    for _ in 0..256 {
        source
            .open_inline(NodeId(1), &style, InlineEdges::default())
            .close_inline();
    }
    let shared = RubyContent::from_builder(source);
    for (bases, limits, kind) in [
        (
            true,
            Limits {
                max_items: Some(1024),
                ..Default::default()
            },
            LimitKind::Items,
        ),
        (
            false,
            Limits {
                max_items: Some(1024),
                ..Default::default()
            },
            LimitKind::Items,
        ),
        (
            false,
            Limits {
                max_styles: Some(8),
                ..Default::default()
            },
            LimitKind::Styles,
        ),
        (
            false,
            Limits {
                max_nesting_depth: Some(1),
                ..Default::default()
            },
            LimitKind::NestingDepth,
        ),
        (
            true,
            Limits {
                max_style_work: Some(25),
                ..Default::default()
            },
            LimitKind::StyleWork,
        ),
    ] {
        let ruby = input(&shared, bases);
        let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        let scope = ALLOCATOR.begin().unwrap();
        parent.push_ruby(NodeId(3), &style, ruby);
        let counts = scope.finish();
        assert_eq!(parent.error().unwrap().kind, kind);
        assert_eq!(counts.calls, 0, "{bases}/{kind:?}: {counts:?}");
        assert_eq!(counts.allocated_bytes, 0, "{bases}/{kind:?}: {counts:?}");
    }
    let ruby = input(&shared, true);
    let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    let scope = ALLOCATOR.begin().unwrap();
    parent.push_ruby(NodeId(4), &style, ruby);
    let counts = scope.finish();
    assert_eq!(parent.error(), None, "the same aliases fit ordinary limits");
    println!("accepted 128 aliases of 512 raw items: {counts:?}");
}
