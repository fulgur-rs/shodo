use shodo::font::FontCollection;
use shodo::geometry::BaselineKind;
use shodo::limits::{Limits, WarningKind};
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{
    InlineStyle, LineHeight, LineOptions, ParagraphStyle, TabSize, TextIndent, WhiteSpaceCollapse,
};
use shodo::{
    AtomicSizes, BreakReason, LayoutContext, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder,
};

fn root(line_height: LineHeight) -> ParagraphStyle {
    let root = InlineStyle {
        font_size: 10.0,
        line_height,
        ..InlineStyle::default()
    };
    ParagraphStyle {
        root,
        ..ParagraphStyle::default()
    }
}

fn para_with(style: &ParagraphStyle, build: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
    let mut b = ParagraphBuilder::new(style, &Limits::default());
    build(&mut b);
    b.build(
        &mut LayoutContext::new(),
        &FontCollection::new(&Limits::default()),
    )
    .unwrap()
}

fn para(text: &str) -> Paragraph {
    para_with(&root(LineHeight::Normal), |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            text,
        );
    })
}

fn lines(para: &Paragraph, width: f32, options: &LineOptions) -> Vec<shodo::Line> {
    let mut cx = LayoutContext::new();
    let mut token = para.start_token();
    let mut out = Vec::new();
    loop {
        match para.next_line(
            &mut cx,
            token,
            options,
            &LineConstraint::new(width),
            &AtomicSizes::EMPTY,
        ) {
            LineResult::Line(line) => {
                token = line.break_token();
                out.push(line);
            }
            LineResult::Done => return out,
            other => panic!("unexpected {other:?}"),
        }
        assert!(out.len() <= para.text().len() + 1, "no progress");
    }
}

fn texts(para: &Paragraph, width: f32) -> Vec<String> {
    lines(para, width, &LineOptions::default())
        .iter()
        .map(|l| para.text()[l.text_range()].to_string())
        .collect()
}

#[test]
fn breaks_greedily_at_spaces() {
    let p = para("aaa bbb ccc");
    assert_eq!(texts(&p, 65.0), ["aaa ", "bbb ", "ccc"]);
    assert_eq!(texts(&p, 70.0), ["aaa bbb ", "ccc"]);
}

#[test]
fn trailing_spaces_hang() {
    let p = para("aaa bbb");
    let first = &lines(&p, 45.0, &LineOptions::default())[0];
    assert_eq!(first.inline_size(), 30.0);
    assert_eq!(first.break_reason(), BreakReason::Regular);
    assert!(!first.is_last());
}

#[test]
fn forced_breaks_end_lines() {
    let p = para_with(&root(LineHeight::Normal), |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "ab",
        )
        .push_forced_break(NodeId(2))
        .push_text(
            TextSource::Dom {
                node: NodeId(3),
                offset: 0,
            },
            "cd",
        );
    });
    let ls = lines(&p, 100.0, &LineOptions::default());
    assert_eq!(ls.len(), 2);
    assert_eq!(ls[0].break_reason(), BreakReason::Forced);
    assert!(ls[0].is_last());
    assert_eq!(ls[1].break_reason(), BreakReason::End);
}

#[test]
fn words_wider_than_the_line_overflow() {
    let p = para("aaaaaaaa b");
    let ls = lines(&p, 30.0, &LineOptions::default());
    assert_eq!(p.text()[ls[0].text_range()].to_string(), "aaaaaaaa ");
    assert_eq!(ls[0].inline_size(), 80.0);
    assert_eq!(ls.len(), 2);
}

#[test]
fn zero_and_negative_widths_still_progress() {
    let p = para("a b c");
    assert_eq!(texts(&p, 0.0), ["a ", "b ", "c"]);
    let mut cx = LayoutContext::new();
    let r = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(-5.0),
        &AtomicSizes::EMPTY,
    );
    assert!(matches!(r, LineResult::Line(_)));
    assert!(
        cx.take_warnings()
            .iter()
            .any(|w| w.kind == WarningKind::NegativeInput)
    );
    for width in [-1.0, 0.0, 1.0, 7.0, 13.0, f32::NAN, 1.0e9] {
        lines(&para("a bb ccc dddd"), width, &LineOptions::default());
    }
}

#[test]
fn negative_inline_start_offset_is_normalized() {
    let p = para("ab");
    let mut cx = LayoutContext::new();
    let constraint = LineConstraint {
        inline_start_offset: -5.0,
        ..LineConstraint::new(100.0)
    };
    let LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(line.inline_size(), 20.0);
    assert!(
        cx.take_warnings()
            .iter()
            .any(|w| w.kind == WarningKind::NegativeInput)
    );
}

#[test]
fn empty_paragraph_is_done_immediately() {
    let p = para("   ");
    let r = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(100.0),
        &AtomicSizes::EMPTY,
    );
    assert!(matches!(r, LineResult::Done));
}

#[test]
fn tokens_are_tied_to_their_paragraph() {
    let a = para("aaa");
    let b = para("aaa");
    let r = b.next_line(
        &mut LayoutContext::new(),
        a.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(100.0),
        &AtomicSizes::EMPTY,
    );
    assert!(matches!(r, LineResult::InvalidToken));
    let r = a.clone().next_line(
        &mut LayoutContext::new(),
        a.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(100.0),
        &AtomicSizes::EMPTY,
    );
    assert!(matches!(r, LineResult::Line(_)));
}

#[test]
fn a_saved_token_resumes_at_another_width() {
    let p = para("aaa bbb ccc ddd");
    let mut cx = LayoutContext::new();
    let opts = LineOptions::default();
    let LineResult::Line(first) = p.next_line(
        &mut cx,
        p.start_token(),
        &opts,
        &LineConstraint::new(35.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(&p.text()[first.text_range()], "aaa ");
    let LineResult::Line(rest) = p.next_line(
        &mut cx,
        first.break_token(),
        &opts,
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(&p.text()[rest.text_range()], "bbb ccc ddd");
}

#[test]
fn text_indent_applies_to_the_first_line() {
    let p = para("aaa bbb");
    let indented = LineOptions {
        text_indent: TextIndent {
            length: 20.0,
            ..TextIndent::default()
        },
        ..LineOptions::default()
    };
    assert_eq!(lines(&p, 70.0, &LineOptions::default()).len(), 1);
    assert_eq!(lines(&p, 70.0, &indented).len(), 2);
}

#[test]
fn strut_sets_block_size_and_baseline() {
    let normal = &lines(&para("a"), 100.0, &LineOptions::default())[0];
    assert_eq!(
        (
            normal.block_size(),
            normal.baseline(BaselineKind::Alphabetic)
        ),
        (10.0, 8.0)
    );
    let p = para_with(&root(LineHeight::Px(20.0)), |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "a",
        );
    });
    let tall = &lines(&p, 100.0, &LineOptions::default())[0];
    assert_eq!(
        (tall.block_size(), tall.baseline(BaselineKind::Alphabetic)),
        (20.0, 13.0)
    );
    assert_eq!(tall.baseline(BaselineKind::Central), 10.0);
}

#[test]
fn tab_stops_are_measured_from_the_content_edge() {
    let pre = InlineStyle {
        font_size: 10.0,
        white_space_collapse: WhiteSpaceCollapse::Preserve,
        tab_size: TabSize::Px(40.0),
        ..InlineStyle::default()
    };
    let p = para_with(&root(LineHeight::Normal), |b| {
        b.open_inline(NodeId(1), &pre, InlineEdges::default())
            .push_text(
                TextSource::Dom {
                    node: NodeId(2),
                    offset: 0,
                },
                "a\tb",
            )
            .close_inline();
    });
    let opts = LineOptions::default();
    let mut cx = LayoutContext::new();
    let LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &opts,
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(line.inline_size(), 50.0);
    let shifted = LineConstraint {
        inline_start_offset: 5.0,
        ..LineConstraint::new(1000.0)
    };
    let LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &opts,
        &shifted,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(line.inline_size(), 45.0);
}

#[test]
fn line_is_send_sync_and_static() {
    fn assert_bounds<T: Send + Sync + 'static>() {}
    assert_bounds::<shodo::Line>();
}

#[test]
fn negative_line_height_becomes_zero_with_a_warning() {
    let p = para_with(&root(LineHeight::Px(-20.0)), |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "a",
        );
    });
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::NegativeInput)
    );
    let line = &lines(&p, 100.0, &LineOptions::default())[0];
    assert_eq!(line.block_size(), 0.0);
    // Zero line height: half the leading (-5px) goes above the 8px ascent.
    assert_eq!(line.baseline(BaselineKind::Alphabetic), 3.0);
}

#[test]
fn non_finite_style_values_become_initial_or_zero() {
    let p = para_with(&root(LineHeight::Number(f32::NAN)), |b| {
        b.open_inline(
            NodeId(1),
            &InlineStyle {
                font_size: 10.0,
                letter_spacing: f32::INFINITY,
                word_spacing: f32::NAN,
                font_weight: f32::NAN,
                tab_size: TabSize::Spaces(f32::NEG_INFINITY),
                ..InlineStyle::default()
            },
            InlineEdges::default(),
        )
        .push_text(
            TextSource::Dom {
                node: NodeId(2),
                offset: 0,
            },
            "a",
        )
        .close_inline();
    });
    let count = p
        .warnings()
        .iter()
        .filter(|w| w.kind == WarningKind::NonFiniteInput)
        .count();
    assert!(count >= 5, "{:?}", p.warnings());
    let line = &lines(&p, 100.0, &LineOptions::default())[0];
    assert_eq!(line.block_size(), 0.0);
}

fn next(p: &Paragraph, token: shodo::BreakToken) -> LineResult {
    p.next_line(
        &mut LayoutContext::new(),
        token,
        &LineOptions::default(),
        &LineConstraint::new(100.0),
        &AtomicSizes::EMPTY,
    )
}

fn dom(node: u64) -> TextSource {
    TextSource::Dom {
        node: NodeId(node),
        offset: 0,
    }
}

#[test]
fn a_block_in_inline_ends_the_line_and_is_reported_next() {
    let p = para_with(&root(LineHeight::Normal), |b| {
        b.push_text(dom(1), "ab")
            .push_block_in_inline(NodeId(5))
            .push_text(dom(2), "cd");
    });
    let LineResult::Line(first) = next(&p, p.start_token()) else {
        panic!()
    };
    assert_eq!(first.break_reason(), BreakReason::BlockInInline);
    assert!(first.is_last());
    assert_eq!(&p.text()[first.text_range()], "ab");
    let LineResult::BlockInInline { node, token_after } = next(&p, first.break_token()) else {
        panic!()
    };
    assert_eq!(node, NodeId(5));
    let LineResult::Line(last) = next(&p, token_after) else {
        panic!()
    };
    assert_eq!(&p.text()[last.text_range()], "cd");
    assert_eq!(last.break_reason(), BreakReason::End);
    assert!(matches!(next(&p, last.break_token()), LineResult::Done));
}

#[test]
fn a_leading_block_in_inline_is_reported_from_the_start_token() {
    let p = para_with(&root(LineHeight::Normal), |b| {
        b.push_block_in_inline(NodeId(5)).push_text(dom(2), "cd");
    });
    let LineResult::BlockInInline { node, token_after } = next(&p, p.start_token()) else {
        panic!()
    };
    assert_eq!(node, NodeId(5));
    let LineResult::Line(line) = next(&p, token_after) else {
        panic!()
    };
    assert_eq!(&p.text()[line.text_range()], "cd");
}
