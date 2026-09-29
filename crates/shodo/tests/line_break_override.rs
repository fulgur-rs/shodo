mod common;

use common::*;
use shodo::geometry::WritingMode;
use shodo::node::{NodeId, TextSource};
use shodo::style::LineOptions;
use shodo::style::{
    OverflowWrap, TextCombineUpright, TextTransform, TextWrapMode, WhiteSpaceCollapse,
};
use shodo::{
    AtomicIntrinsics, AtomicSizes, LayoutContext, LineBreakOverride, LineConstraint, LineResult,
    SoftBreakOpportunity,
};
use std::sync::{Arc, Mutex};

#[test]
fn added_breaks_feed_layout_and_intrinsic_widths() {
    let plain = paragraph("abcd");
    let custom = build(&style(), |builder| {
        builder.with_line_break_override(|context| {
            if context.offset < context.text.len() {
                LineBreakOverride::Allow
            } else {
                LineBreakOverride::UseStandard
            }
        });
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "abcd");
    });
    let options = LineOptions::default();
    let intrinsic = |paragraph: &shodo::Paragraph| {
        paragraph
            .intrinsic_sizes(
                &mut LayoutContext::new(),
                &options,
                &AtomicIntrinsics::EMPTY,
            )
            .min_content
    };
    assert_eq!(intrinsic(&plain), 40.0);
    assert_eq!(intrinsic(&custom), 10.0);
    assert_eq!(
        plain
            .break_all(
                &mut LayoutContext::new(),
                &options,
                15.0,
                &AtomicSizes::EMPTY
            )
            .len(),
        1
    );
    assert_eq!(
        custom
            .break_all(
                &mut LayoutContext::new(),
                &options,
                15.0,
                &AtomicSizes::EMPTY
            )
            .iter()
            .map(|line| line.text_range())
            .collect::<Vec<_>>(),
        vec![0..1, 1..2, 2..3, 3..4]
    );
}

#[test]
fn suppressing_a_standard_break_updates_intrinsic_width_and_layout() {
    let plain = paragraph("aa bb");
    let custom = build(&style(), |builder| {
        builder.with_line_break_override(|_| LineBreakOverride::Prohibit);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "aa bb");
    });
    let min = |paragraph: &shodo::Paragraph| {
        paragraph
            .intrinsic_sizes(
                &mut LayoutContext::new(),
                &LineOptions::default(),
                &AtomicIntrinsics::EMPTY,
            )
            .min_content
    };
    assert_eq!(min(&plain), 20.0);
    assert_eq!(min(&custom), 50.0);
    assert_eq!(
        plain
            .break_all(
                &mut LayoutContext::new(),
                &LineOptions::default(),
                25.0,
                &AtomicSizes::EMPTY,
            )
            .len(),
        2
    );
    assert_eq!(
        custom
            .break_all(
                &mut LayoutContext::new(),
                &LineOptions::default(),
                25.0,
                &AtomicSizes::EMPTY,
            )
            .len(),
        1
    );
}

#[test]
fn delegation_preserves_standard_breaks() {
    let plain = paragraph("aa bb cc");
    let custom = build(&style(), |builder| {
        builder.with_line_break_override(|_| LineBreakOverride::UseStandard);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "aa bb cc");
    });
    let ranges = |paragraph: &shodo::Paragraph| {
        paragraph
            .break_all(
                &mut LayoutContext::new(),
                &LineOptions::default(),
                25.0,
                &AtomicSizes::EMPTY,
            )
            .iter()
            .map(|line| line.text_range())
            .collect::<Vec<_>>()
    };
    assert_eq!(ranges(&plain), ranges(&custom));
}

#[test]
fn forced_break_and_nowrap_take_priority() {
    let mut no_wrap = style();
    no_wrap.root.text_wrap_mode = TextWrapMode::NoWrap;
    let nowrap = build(&no_wrap, |builder| {
        builder.with_line_break_override(|_| LineBreakOverride::Allow);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "abcd");
    });
    assert_eq!(
        nowrap
            .break_all(
                &mut LayoutContext::new(),
                &LineOptions::default(),
                15.0,
                &AtomicSizes::EMPTY,
            )
            .len(),
        1
    );

    let mut preserved = style();
    preserved.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let forced = build(&preserved, |builder| {
        builder.with_line_break_override(|_| LineBreakOverride::Prohibit);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "a\nb");
    });
    let lines = forced.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        100.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].break_reason(), shodo::BreakReason::Forced);
}

#[test]
fn callback_only_sees_complete_graphemes_and_indivisible_transforms() {
    let visited = Arc::new(Mutex::new(Vec::new()));
    let records = Arc::clone(&visited);
    let grapheme = build(&style(), |builder| {
        builder.with_line_break_override(move |context| {
            records.lock().unwrap().push(context.offset);
            LineBreakOverride::Allow
        });
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "a\u{301}b");
    });
    assert_eq!(*visited.lock().unwrap(), vec![3]);
    assert_eq!(
        grapheme
            .break_all(
                &mut LayoutContext::new(),
                &LineOptions::default(),
                15.0,
                &AtomicSizes::EMPTY,
            )
            .iter()
            .map(|line| line.text_range())
            .collect::<Vec<_>>(),
        vec![0..3, 3..4]
    );

    let mut uppercase = style();
    uppercase.root.text_transform = TextTransform::Uppercase;
    let visited = Arc::new(Mutex::new(Vec::new()));
    let records = Arc::clone(&visited);
    let transformed = build(&uppercase, |builder| {
        builder.with_line_break_override(move |context| {
            records.lock().unwrap().push(context.offset);
            LineBreakOverride::Allow
        });
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "ßa");
    });
    assert_eq!(transformed.text(), "SSA");
    assert_eq!(*visited.lock().unwrap(), vec![2]);
}

#[test]
fn first_line_alternate_uses_the_same_override() {
    let mut root = style();
    let mut first = root.root.clone();
    first.text_transform = TextTransform::Lowercase;
    root.first_line = Some(first);
    let visited = Arc::new(Mutex::new(Vec::new()));
    let records = Arc::clone(&visited);
    let custom = build(&root, |builder| {
        builder.with_line_break_override(move |context| {
            records
                .lock()
                .unwrap()
                .push((context.text.to_owned(), context.offset));
            LineBreakOverride::Allow
        });
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "İa");
    });
    assert_eq!(custom.text(), "İa");
    assert_eq!(
        *visited.lock().unwrap(),
        vec![("İa".to_owned(), 2), ("i\u{307}a".to_owned(), 3)]
    );
    let options = LineOptions::default();
    let greedy = custom.break_all(
        &mut LayoutContext::new(),
        &options,
        15.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(greedy.len(), 2);
    let plan = custom.plan_breaks(
        &mut LayoutContext::new(),
        &options,
        15.0,
        &AtomicSizes::EMPTY,
    );
    let mut constraint = LineConstraint::new(15.0);
    constraint.break_plan = Some(&plan);
    let LineResult::Line(planned) = custom.next_line(
        &mut LayoutContext::new(),
        custom.start_token(),
        &options,
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("expected first line");
    };
    assert_eq!(planned.break_token(), greedy[0].break_token());
}

#[test]
fn combined_text_has_no_internal_override_boundary() {
    let mut root = style();
    root.writing_mode = WritingMode::VerticalRl;
    root.root.text_combine_upright = TextCombineUpright::All;
    let visited = Arc::new(Mutex::new(Vec::new()));
    let records = Arc::clone(&visited);
    let combined = build(&root, |builder| {
        builder.with_line_break_override(move |context| {
            records.lock().unwrap().push(context.offset);
            LineBreakOverride::Allow
        });
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "12");
    });
    assert!(visited.lock().unwrap().is_empty());
    let lines = combined.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        5.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text_range(), 0..2);
    assert_eq!(lines[0].inline_size(), 10.0);
}

#[test]
fn emergency_breaks_can_be_suppressed_without_changing_the_default() {
    let mut root = style();
    root.root.overflow_wrap = OverflowWrap::Anywhere;
    let plain = build(&root, |builder| {
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "abcd");
    });
    let visited = Arc::new(Mutex::new(Vec::new()));
    let records = Arc::clone(&visited);
    let custom = build(&root, |builder| {
        builder.with_line_break_override(move |context| {
            records.lock().unwrap().push(context.standard);
            LineBreakOverride::Prohibit
        });
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "abcd");
    });
    assert!(
        visited
            .lock()
            .unwrap()
            .contains(&SoftBreakOpportunity::Emergency)
    );
    let count = |paragraph: &shodo::Paragraph| {
        paragraph
            .break_all(
                &mut LayoutContext::new(),
                &LineOptions::default(),
                15.0,
                &AtomicSizes::EMPTY,
            )
            .len()
    };
    assert_eq!(count(&plain), 4);
    assert_eq!(count(&custom), 1);
}

#[test]
fn layout_cache_and_break_plans_keep_paragraphs_separate() {
    let plain = paragraph("abcd");
    let custom = build(&style(), |builder| {
        builder.with_line_break_override(|_| LineBreakOverride::Allow);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "abcd");
    });
    let options = LineOptions::default();
    let mut cx = LayoutContext::new();
    for (paragraph, expected) in [(&plain, 1), (&custom, 4), (&plain, 1)] {
        assert_eq!(
            paragraph
                .break_all(&mut cx, &options, 15.0, &AtomicSizes::EMPTY)
                .len(),
            expected
        );
    }
    let plan = custom.plan_breaks(&mut cx, &options, 15.0, &AtomicSizes::EMPTY);
    let mut constraint = LineConstraint::new(15.0);
    constraint.break_plan = Some(&plan);
    let LineResult::Line(line) = custom.next_line(
        &mut cx,
        custom.start_token(),
        &options,
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("expected custom line");
    };
    assert_eq!(line.text_range(), 0..1);
    let LineResult::Line(line) = plain.next_line(
        &mut cx,
        plain.start_token(),
        &options,
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("expected plain line");
    };
    assert_eq!(line.text_range(), 0..4);
}
