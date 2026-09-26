use shodo::font::FontCollection;
use shodo::limits::{Limits, WarningKind};
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{LineOptions, ParagraphStyle};
use shodo::{
    AtomicSize, AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
};

#[test]
fn constraint_block_offset_is_normalized() {
    let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    let mut cx = LayoutContext::new();
    let p = builder
        .build(&mut cx, &FontCollection::new(&Limits::default()))
        .unwrap();
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let c = LineConstraint {
            block_offset: value,
            ..LineConstraint::new(100.0)
        };
        let LineResult::Line(line) = p.next_line(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        assert_eq!(line.block_offset(), 0.0);
        assert!(
            cx.take_warnings()
                .iter()
                .any(|w| w.kind == WarningKind::NonFiniteInput)
        );
    }
}

#[test]
fn atomic_geometry_never_exposes_nonfinite_input() {
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_atomic(
        NodeId(1),
        &ParagraphStyle::default().root,
        InlineEdges::default(),
    );
    let mut cx = LayoutContext::new();
    let p = b
        .build(&mut cx, &FontCollection::new(&Limits::default()))
        .unwrap();
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut atomics = AtomicSizes::new();
        atomics.insert(
            NodeId(1),
            AtomicSize {
                inline_size: value,
                block_size: value,
                baseline: Some(value),
                margins: Sides {
                    inline_start: value,
                    inline_end: value,
                    block_start: value,
                    block_end: value,
                },
            },
        );
        let LineResult::Line(line) = p.next_line(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            &LineConstraint::new(100.0),
            &atomics,
        ) else {
            panic!()
        };
        let a = line
            .fragments()
            .find_map(|f| {
                if let Fragment::Atomic(a) = f {
                    Some(a)
                } else {
                    None
                }
            })
            .unwrap();
        for v in [
            a.margin_rect.inline_start,
            a.margin_rect.block_start,
            a.margin_rect.inline_size,
            a.margin_rect.block_size,
            a.border_rect.inline_size,
            a.border_rect.block_size,
            a.baseline,
        ] {
            assert!(v.is_finite(), "{a:?}");
        }
        assert_eq!(a.margin_rect.inline_size, 0.0);
        assert_eq!(a.margin_rect.block_size, 0.0);
    }
}

#[test]
fn negative_atomic_dimensions_preserve_signed_margins_and_baseline() {
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_atomic(
        NodeId(1),
        &ParagraphStyle::default().root,
        InlineEdges::default(),
    );
    let mut cx = LayoutContext::new();
    let p = b
        .build(&mut cx, &FontCollection::new(&Limits::default()))
        .unwrap();
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(1),
        AtomicSize {
            inline_size: -5.0,
            block_size: -5.0,
            baseline: Some(-3.0),
            margins: Sides {
                inline_start: -2.0,
                ..Sides::default()
            },
        },
    );
    let LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(100.0),
        &atomics,
    ) else {
        panic!()
    };
    let a = line
        .fragments()
        .find_map(|f| {
            if let Fragment::Atomic(a) = f {
                Some(a)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(a.border_rect.inline_size, 0.0);
    assert_eq!(a.border_rect.block_size, 0.0);
    assert_eq!(a.margin_rect.inline_size, -2.0);
    assert_eq!(a.margin_rect.block_start, a.baseline + 3.0);
}

#[test]
fn caller_dimensions_round_to_nearest_unit() {
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_atomic(
        NodeId(1),
        &ParagraphStyle::default().root,
        InlineEdges::default(),
    );
    let mut cx = LayoutContext::new();
    let p = b
        .build(&mut cx, &FontCollection::new(&Limits::default()))
        .unwrap();
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(1),
        AtomicSize {
            inline_size: 100.000001,
            block_size: 100.000001,
            ..AtomicSize::default()
        },
    );
    let c = LineConstraint {
        block_offset: -3.000001,
        ..LineConstraint::new(100.000001)
    };
    let LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &c,
        &atomics,
    ) else {
        panic!()
    };
    assert_eq!(line.inline_size(), 100.0);
    assert_eq!(line.block_offset(), -3.0);
    let a = line
        .fragments()
        .find_map(|f| {
            if let Fragment::Atomic(a) = f {
                Some(a)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(a.margin_rect.block_size, 100.0);
}
