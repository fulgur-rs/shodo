//! Paragraphs built one after another with one context share the style and
//! limits they were given when those are the same.

use std::sync::Arc;

use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::style::{FontFamily, ParagraphStyle};
use crate::{LayoutContext, Paragraph, ParagraphBuilder};

fn style() -> ParagraphStyle {
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Noto Sans".into())];
    style
}

fn build(cx: &mut LayoutContext, style: &ParagraphStyle, limits: &Limits, text: &str) -> Paragraph {
    let fonts = crate::test_support::fonts::collection(limits);
    fonts
        .register(crate::test_support::fonts::LATIN.to_vec())
        .unwrap();
    let mut builder = ParagraphBuilder::new(style, limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        text,
    );
    builder.build(cx, &fonts).unwrap()
}

#[test]
fn paragraphs_of_one_style_and_limits_share_them() {
    let (style, limits) = (style(), Limits::default());
    let mut cx = LayoutContext::new();
    let one = build(&mut cx, &style, &limits, "Item 1");
    let other = build(&mut cx, &style, &limits, "Item 2");
    assert!(Arc::ptr_eq(&one.data.style, &other.data.style));
    assert!(Arc::ptr_eq(&one.data.limits, &other.data.limits));
    assert_eq!(*one.data.style, style);
    assert_eq!(*one.data.limits, limits);
}

#[test]
fn a_paragraph_of_another_style_or_limits_keeps_its_own() {
    let (style, limits) = (style(), Limits::default());
    let mut cx = LayoutContext::new();
    let one = build(&mut cx, &style, &limits, "Item 1");

    let mut larger = style.clone();
    larger.root.font_size = 20.0;
    let other = build(&mut cx, &larger, &limits, "Item 2");
    assert!(!Arc::ptr_eq(&one.data.style, &other.data.style));
    assert_eq!(other.data.style.root.font_size, 20.0);
    assert!(Arc::ptr_eq(&one.data.limits, &other.data.limits));

    let tighter = Limits {
        max_items: Some(64),
        ..limits.clone()
    };
    let third = build(&mut cx, &larger, &tighter, "Item 3");
    assert!(Arc::ptr_eq(&other.data.style, &third.data.style));
    assert!(!Arc::ptr_eq(&other.data.limits, &third.data.limits));
    assert_eq!(third.data.limits.max_items, Some(64));
}

#[test]
fn styles_that_compare_equal_but_differ_in_a_zero_sign_are_not_shared() {
    let (style, limits) = (style(), Limits::default());
    let mut cx = LayoutContext::new();
    let mut negative = style.clone();
    negative.root.letter_spacing = -0.0;
    assert_eq!(style, negative);
    let one = build(&mut cx, &style, &limits, "Item 1");
    let other = build(&mut cx, &negative, &limits, "Item 2");
    assert!(!Arc::ptr_eq(&one.data.style, &other.data.style));
    assert!(other.data.style.root.letter_spacing.is_sign_negative());
}

#[test]
fn a_first_line_style_tells_two_paragraph_styles_apart() {
    let (style, limits) = (style(), Limits::default());
    let mut cx = LayoutContext::new();
    let mut with_first_line = style.clone();
    with_first_line.first_line = Some(crate::style::InlineStyle {
        font_size: 20.0,
        ..style.root.clone()
    });
    let one = build(&mut cx, &style, &limits, "Item 1");
    let other = build(&mut cx, &with_first_line, &limits, "Item 2");
    let third = build(&mut cx, &with_first_line, &limits, "Item 3");
    assert!(!Arc::ptr_eq(&one.data.style, &other.data.style));
    assert!(Arc::ptr_eq(&other.data.style, &third.data.style));
}
