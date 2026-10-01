mod common;
use common::*;
use shodo::style::{LineOptions, TextAlign, TextIndent, TextWrapStyle};
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult};

fn planned(
    p: &shodo::Paragraph,
    o: &LineOptions,
    w: f32,
    a: &AtomicSizes,
    plan: &shodo::BreakPlan,
) -> Vec<std::ops::Range<usize>> {
    let mut cx = LayoutContext::new();
    let mut token = p.start_token();
    let mut ranges = Vec::new();
    let mut c = LineConstraint::new(w);
    c.break_plan = Some(plan);
    loop {
        match p.next_line(&mut cx, token, o, &c, a) {
            LineResult::Line(l) => {
                token = l.break_token();
                ranges.push(l.text_range());
            }
            LineResult::BlockInInline { token_after, .. } => token = token_after,
            LineResult::Done => break,
            other => panic!("{other:?}"),
        }
    }
    ranges
}

#[test]
fn balanced_plan_reproduces_and_mismatches_fall_back() {
    let p = paragraph("a a a a a a a");
    let o = LineOptions {
        text_wrap_style: TextWrapStyle::Balance,
        ..LineOptions::default()
    };
    let plan = p.plan_breaks(&mut LayoutContext::new(), &o, 100.0, &AtomicSizes::EMPTY);
    let a = planned(&p, &o, 100.0, &AtomicSizes::EMPTY, &plan);
    assert_eq!(a, planned(&p, &o, 100.0, &AtomicSizes::EMPTY, &plan));
    assert_eq!(a.len(), 2);
    assert_eq!(a[0], 0..8);
    let other = paragraph(p.text());
    for (q, options, width) in [
        (&other, o, 100.0),
        (&p, o, 60.0),
        (
            &p,
            LineOptions {
                text_align: TextAlign::Center,
                ..o
            },
            100.0,
        ),
    ] {
        assert_eq!(
            planned(q, &options, width, &AtomicSizes::EMPTY, &plan),
            q.break_all(
                &mut LayoutContext::new(),
                &options,
                width,
                &AtomicSizes::EMPTY
            )
            .iter()
            .map(|l| l.text_range())
            .collect::<Vec<_>>()
        );
    }
}

#[test]
fn zero_search_limits_warn_and_nan_empty_inputs_terminate() {
    let root = style();
    let limits = shodo::limits::Limits {
        max_balance_iterations: Some(0),
        max_pretty_window_lines: Some(0),
        ..Default::default()
    };
    let mut b = shodo::ParagraphBuilder::new(&root, &limits);
    b.push_text(
        shodo::node::TextSource::Generated {
            node: shodo::node::NodeId(1),
        },
        "a a a a",
    );
    let p = b
        .build(
            &mut LayoutContext::new(),
            &shodo::font::FontCollection::with_options(
                &limits,
                shodo::font::FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            ),
        )
        .unwrap();
    for wrap in [TextWrapStyle::Balance, TextWrapStyle::Pretty] {
        let o = LineOptions {
            text_wrap_style: wrap,
            ..LineOptions::default()
        };
        let mut cx = LayoutContext::new();
        let plan = p.plan_breaks(&mut cx, &o, 50.0, &AtomicSizes::EMPTY);
        assert!(!cx.take_warnings().is_empty());
        assert_eq!(
            planned(&p, &o, 50.0, &AtomicSizes::EMPTY, &plan).len(),
            p.break_all(&mut cx, &o, 50.0, &AtomicSizes::EMPTY).len()
        );
    }
    for text in ["", "a", "aaaaaaaaaaaa"] {
        let p = paragraph(text);
        let o = LineOptions::default();
        let plan = p.plan_breaks(&mut LayoutContext::new(), &o, f32::NAN, &AtomicSizes::EMPTY);
        planned(&p, &o, f32::NAN, &AtomicSizes::EMPTY, &plan);
    }
}

#[test]
fn pretty_reduces_raggedness_and_atomic_instances_do_not_match() {
    let p = paragraph("aaa b cccc dd e");
    let o = LineOptions {
        text_wrap_style: TextWrapStyle::Pretty,
        ..LineOptions::default()
    };
    let plan = p.plan_breaks(&mut LayoutContext::new(), &o, 70.0, &AtomicSizes::EMPTY);
    let ranges = planned(&p, &o, 70.0, &AtomicSizes::EMPTY, &plan);
    // Moving only the second break has lower cost than moving both breaks.
    assert_eq!(ranges, vec![0..6, 6..11, 11..15]);
    let root = style();
    let p = build(&root, |b| {
        b.push_atomic(
            shodo::node::NodeId(2),
            &root.root,
            shodo::node::InlineEdges::default(),
        )
        .push_text(
            shodo::node::TextSource::Generated {
                node: shodo::node::NodeId(1),
            },
            "a a a",
        );
    });
    let mut a = AtomicSizes::new();
    let mut b = AtomicSizes::new();
    a.insert(
        shodo::node::NodeId(2),
        shodo::AtomicSize {
            inline_size: 10.0,
            ..Default::default()
        },
    );
    b.insert(
        shodo::node::NodeId(2),
        shodo::AtomicSize {
            inline_size: 40.0,
            ..Default::default()
        },
    );
    assert_eq!(a.generation(), b.generation());
    let plan = p.plan_breaks(&mut LayoutContext::new(), &o, 50.0, &a);
    assert_eq!(
        planned(&p, &o, 50.0, &b, &plan),
        p.break_all(&mut LayoutContext::new(), &o, 50.0, &b)
            .iter()
            .map(|l| l.text_range())
            .collect::<Vec<_>>()
    );
}

#[test]
fn pretty_accounts_for_each_line_indent_after_block() {
    let mut root = style();
    root.root.font_size = 5.0;
    let p = build(&root, |b| {
        b.push_text(
            shodo::node::TextSource::Generated {
                node: shodo::node::NodeId(1),
            },
            "a",
        )
        .push_block_in_inline(shodo::node::NodeId(2))
        .push_text(
            shodo::node::TextSource::Generated {
                node: shodo::node::NodeId(3),
            },
            "aaa aaa aaa aaa aaa aaaa",
        );
    });
    let o = LineOptions {
        text_indent: TextIndent {
            length: 40.0,
            each_line: true,
            ..Default::default()
        },
        text_wrap_style: TextWrapStyle::Pretty,
        ..LineOptions::default()
    };
    let plan = p.plan_breaks(&mut LayoutContext::new(), &o, 100.0, &AtomicSizes::EMPTY);
    let ranges = planned(&p, &o, 100.0, &AtomicSizes::EMPTY, &plan);
    let text_start = p.text().find("aaa").unwrap();
    assert_eq!(
        &ranges[ranges.len() - 2..],
        &[text_start..text_start + 8, text_start + 8..p.text().len()]
    );
}

#[test]
fn forced_boundaries_and_insets_preserve_greedy_fallback() {
    let root = style();
    let p = build(&root, |b| {
        b.push_text(
            shodo::node::TextSource::Generated {
                node: shodo::node::NodeId(1),
            },
            "aa bb cc",
        )
        .push_forced_break(shodo::node::NodeId(2))
        .push_text(
            shodo::node::TextSource::Generated {
                node: shodo::node::NodeId(1),
            },
            "d e",
        );
    });
    for wrap in [
        TextWrapStyle::Auto,
        TextWrapStyle::Balance,
        TextWrapStyle::Pretty,
    ] {
        let o = LineOptions {
            text_wrap_style: wrap,
            ..LineOptions::default()
        };
        let mut cx = LayoutContext::new();
        let plan = p.plan_breaks(&mut cx, &o, 70.0, &AtomicSizes::EMPTY);
        let ranges = planned(&p, &o, 70.0, &AtomicSizes::EMPTY, &plan);
        assert!(
            ranges
                .iter()
                .all(|r| r.start < 8 && r.end <= 9 || r.start >= 9)
        );
        let mut c = LineConstraint::new(70.0);
        c.inline_start_offset = 10.0;
        c.break_plan = Some(&plan);
        let LineResult::Line(l) =
            p.next_line(&mut cx, p.start_token(), &o, &c, &AtomicSizes::EMPTY)
        else {
            panic!()
        };
        c.break_plan = None;
        let LineResult::Line(g) =
            p.next_line(&mut cx, p.start_token(), &o, &c, &AtomicSizes::EMPTY)
        else {
            panic!()
        };
        assert_eq!(l.text_range(), g.text_range());
        assert!(!cx.take_warnings().is_empty());
    }
}
