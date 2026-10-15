//! Percentage inline edges resolved against a basis known at line layout.

use crate::font::FontFaceDescriptor;
use crate::geometry::Direction;
use crate::limits::{Limits, WarningKind};
use crate::node::{InlineEdgePercents, InlineEdges, NodeId, Sides, TextSource};
use crate::style::{FontFamily, InlineStyle, LineHeight, LineOptions, ParagraphStyle};
use crate::{
    AtomicIntrinsics, AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder,
};

const BASIS: f32 = 200.0;

fn fonts(limits: &Limits) -> crate::font::FontCollection {
    let fonts = crate::test_support::fonts::collection(limits);
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    fonts
}

fn style() -> ParagraphStyle {
    ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("Latin".into())],
            font_size: 10.0,
            line_height: LineHeight::Px(12.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn inline_start(v: f32) -> Sides {
    Sides {
        inline_start: v,
        ..Sides::default()
    }
}

fn inline_end(v: f32) -> Sides {
    Sides {
        inline_end: v,
        ..Sides::default()
    }
}

/// `before` + an inline box (node 1) holding `inside` + `after`, with the box
/// given px `edges` and percentage `percents`.
fn build_with(
    style: &ParagraphStyle,
    (before, inside, after): (&str, &str, &str),
    edges: InlineEdges,
    percents: InlineEdgePercents,
) -> Paragraph {
    let limits = Limits::unlimited();
    let fonts = fonts(&limits);
    let mut b = ParagraphBuilder::new(style, &limits);
    let source = |offset: usize| TextSource::Dom {
        node: NodeId(0),
        offset: offset as u32,
    };
    b.push_text(source(0), before)
        .open_inline(NodeId(1), &style.root, edges)
        .set_inline_edge_percents(percents)
        .push_text(source(before.len()), inside)
        .close_inline()
        .push_text(source(before.len() + inside.len()), after);
    b.build(&mut LayoutContext::new(), &fonts).unwrap()
}

fn build(edges: InlineEdges, percents: InlineEdgePercents) -> Paragraph {
    build_with(&style(), ("ab", "cd", "ef"), edges, percents)
}

fn resolve(p: &Paragraph, basis: f32) -> Paragraph {
    let fonts = fonts(&Limits::unlimited());
    p.with_edge_basis(basis, &mut LayoutContext::new(), &fonts)
        .unwrap()
}

/// The single line at an unconstrained width.
fn line(p: &Paragraph) -> crate::Line {
    let mut cx = LayoutContext::new();
    let mut lines = p.break_all(&mut cx, &LineOptions::default(), 1.0e4, &AtomicSizes::EMPTY);
    assert_eq!(lines.len(), 1);
    lines.pop().unwrap()
}

fn inline_box(line: &crate::Line) -> crate::InlineBoxFragment {
    line.fragments()
        .find_map(|f| match f {
            Fragment::InlineBox(b) if b.node == NodeId(1) => Some(b),
            _ => None,
        })
        .unwrap()
}

/// Logical inline start of the first glyph run whose text starts at `offset`.
fn run_start(line: &crate::Line, offset: usize) -> f32 {
    line.fragments()
        .find_map(|f| match f {
            Fragment::GlyphRun(r) if r.text_range().start == offset => Some(r.inline_start()),
            _ => None,
        })
        .unwrap()
}

fn padding(percents: Sides) -> InlineEdgePercents {
    InlineEdgePercents {
        padding: percents,
        ..Default::default()
    }
}

fn margin(percents: Sides) -> InlineEdgePercents {
    InlineEdgePercents {
        margin: percents,
        ..Default::default()
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-3
}

#[test]
fn inline_start_padding_percent_shifts_following_text() {
    let plain = line(&build(
        InlineEdges::default(),
        InlineEdgePercents::default(),
    ));
    let p = build(InlineEdges::default(), padding(inline_start(0.1)));
    assert!(p.has_percent_edges());
    assert_eq!(p.edge_basis(), 0.0);
    let resolved = resolve(&p, BASIS);
    assert!(resolved.has_percent_edges());
    assert_eq!(resolved.edge_basis(), BASIS);
    let l = line(&resolved);
    assert!(close(l.inline_size(), plain.inline_size() + 20.0));
    assert!(close(run_start(&l, 2), run_start(&plain, 2) + 20.0));
    let b = inline_box(&l);
    assert!(close(
        b.content_rect.inline_start - b.rect.inline_start,
        20.0
    ));
    assert!(resolved.warnings().is_empty());
}

#[test]
fn inline_end_padding_percent_shifts_following_text() {
    let plain = line(&build(
        InlineEdges::default(),
        InlineEdgePercents::default(),
    ));
    let l = line(&resolve(
        &build(InlineEdges::default(), padding(inline_end(0.1))),
        BASIS,
    ));
    assert!(close(l.inline_size(), plain.inline_size() + 20.0));
    // The box's own text stays in place; the text after it moves.
    assert!(close(run_start(&l, 2), run_start(&plain, 2)));
    assert!(close(run_start(&l, 4), run_start(&plain, 4) + 20.0));
    let b = inline_box(&l);
    let content_end = b.content_rect.inline_start + b.content_rect.inline_size;
    assert!(close(
        b.rect.inline_start + b.rect.inline_size - content_end,
        20.0
    ));
}

#[test]
fn margin_percents_shift_text_outside_the_border_box() {
    let plain = line(&build(
        InlineEdges::default(),
        InlineEdgePercents::default(),
    ));
    let l = line(&resolve(
        &build(
            InlineEdges::default(),
            margin(Sides {
                inline_start: 0.1,
                inline_end: 0.05,
                ..Sides::default()
            }),
        ),
        BASIS,
    ));
    assert!(close(l.inline_size(), plain.inline_size() + 30.0));
    assert!(close(run_start(&l, 2), run_start(&plain, 2) + 20.0));
    assert!(close(run_start(&l, 4), run_start(&plain, 4) + 30.0));
    let b = inline_box(&l);
    // Margins lie outside the border box.
    assert!(close(b.content_rect.inline_start, b.rect.inline_start));
    assert!(close(b.content_rect.inline_size, b.rect.inline_size));
}

#[test]
fn base_paragraph_resolves_percents_against_zero_including_intrinsic_sizes() {
    let percents = InlineEdgePercents {
        margin: inline_start(0.25),
        padding: Sides {
            inline_start: 0.1,
            inline_end: 0.5,
            block_start: 0.1,
            block_end: 0.1,
        },
    };
    let mut px = InlineEdges::default();
    px.padding.inline_start = 3.0;
    let plain = build(px, InlineEdgePercents::default());
    let p = build(px, percents);
    assert!(p.has_percent_edges());
    assert!(!plain.has_percent_edges());
    assert!(close(line(&p).inline_size(), line(&plain).inline_size()));
    let mut cx = LayoutContext::new();
    let options = LineOptions::default();
    let base = p.intrinsic_sizes(&mut cx, &options, &AtomicIntrinsics::EMPTY);
    let expected = plain.intrinsic_sizes(&mut cx, &options, &AtomicIntrinsics::EMPTY);
    assert_eq!(base, expected);
    // A resolved paragraph does include them.
    let resolved = resolve(&p, BASIS).intrinsic_sizes(&mut cx, &options, &AtomicIntrinsics::EMPTY);
    assert!(close(
        resolved.max_content,
        expected.max_content + 50.0 + 20.0 + 100.0
    ));
    // Resolving against zero reproduces the base geometry.
    let zero = resolve(&p, 0.0);
    assert_ne!(zero.id(), p.id());
    assert_eq!(line(&zero).inline_size(), line(&p).inline_size());
}

#[test]
fn px_and_percent_parts_add_and_resolved_padding_is_not_negative() {
    let plain = line(&build(
        InlineEdges::default(),
        InlineEdgePercents::default(),
    ));
    // calc(10% + 4px)
    let mut px = InlineEdges::default();
    px.padding.inline_start = 4.0;
    let l = line(&resolve(&build(px, padding(inline_start(0.1))), BASIS));
    assert!(close(run_start(&l, 2), run_start(&plain, 2) + 24.0));
    // calc(4px - 10%) resolves below zero and is used as zero.
    let p = resolve(&build(px, padding(inline_start(-0.1))), BASIS);
    let l = line(&p);
    assert!(close(run_start(&l, 2), run_start(&plain, 2)));
    assert!(close(l.inline_size(), plain.inline_size()));
    let b = inline_box(&l);
    assert!(close(b.content_rect.inline_start, b.rect.inline_start));
    assert!(p.warnings().is_empty());
    // calc(10% - 4px) keeps its negative px part until the sum is resolved.
    let mut px = InlineEdges::default();
    px.padding.inline_start = -4.0;
    let p = resolve(&build(px, padding(inline_start(0.1))), BASIS);
    assert!(close(run_start(&line(&p), 2), run_start(&plain, 2) + 16.0));
    assert!(p.warnings().is_empty());
    // A negative resolved margin pulls the following content back:
    // calc(4px - 10%) = -16px.
    let mut px = InlineEdges::default();
    px.margin.inline_end = 4.0;
    let l = line(&resolve(&build(px, margin(inline_end(-0.1))), BASIS));
    assert!(close(l.inline_size(), plain.inline_size() - 16.0));
    assert!(close(run_start(&l, 4), run_start(&plain, 4) - 16.0));
}

#[test]
fn rebuild_identity_and_tokens() {
    let plain = build(InlineEdges::default(), InlineEdgePercents::default());
    assert!(!plain.has_percent_edges());
    let same = resolve(&plain, BASIS);
    assert_eq!(same.id(), plain.id());
    assert_eq!(same.edge_basis(), 0.0);

    let p = build(InlineEdges::default(), padding(inline_start(0.1)));
    let first = resolve(&p, BASIS);
    assert_ne!(first.id(), p.id());
    // Re-resolving uses the original input instead of adding percentages
    // to the already resolved edges.
    let second = resolve(&first, 100.0);
    assert_ne!(second.id(), first.id());
    assert_eq!(second.edge_basis(), 100.0);
    let direct = resolve(&p, 100.0);
    assert_eq!(line(&second).inline_size(), line(&direct).inline_size());

    let mut cx = LayoutContext::new();
    let result = first.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(1.0e4),
        &AtomicSizes::EMPTY,
    );
    assert!(matches!(result, LineResult::InvalidToken));
}

#[test]
fn invalid_basis_is_reported_and_treated_as_zero() {
    let p = build(InlineEdges::default(), padding(inline_start(0.1)));
    for basis in [f32::NAN, -5.0] {
        let resolved = resolve(&p, basis);
        assert_eq!(resolved.edge_basis(), 0.0);
        assert_eq!(line(&resolved).inline_size(), line(&p).inline_size());
        assert!(resolved.warnings().iter().any(|w| matches!(
            w.kind,
            WarningKind::NonFiniteInput | WarningKind::NegativeInput
        )));
    }
    // Percentages that sanitize to zero leave an ordinary paragraph.
    let p = build(InlineEdges::default(), padding(inline_start(f32::NAN)));
    assert!(!p.has_percent_edges());
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::NonFiniteInput)
    );
}

#[test]
fn misplaced_percent_call_is_ignored_with_a_warning() {
    let limits = Limits::unlimited();
    let fonts = fonts(&limits);
    let style = style();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.open_inline(NodeId(1), &style.root, InlineEdges::default())
        .push_text(TextSource::Generated { node: NodeId(1) }, "ab")
        .set_inline_edge_percents(padding(inline_start(0.1)))
        .close_inline();
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    assert!(!p.has_percent_edges());
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::UnbalancedInline)
    );
}

/// Glyph ids and advances of a line, in visual order.
fn glyphs(line: &crate::Line) -> Vec<(u32, u32)> {
    line.fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .flat_map(|r| {
            r.glyphs()
                .map(|g| (g.id, g.advance.to_bits()))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn percent_boundaries_separate_shaping_for_every_basis() {
    // "AV" kerns in the fixture font when shaped as one run.
    let words = ("A", "V", "");
    let style = style();
    let plain = build_with(
        &style,
        words,
        InlineEdges::default(),
        InlineEdgePercents::default(),
    );
    let mut px = InlineEdges::default();
    px.padding.inline_start = 1.0;
    let px_edge = build_with(&style, words, px, InlineEdgePercents::default());
    let p = build_with(
        &style,
        words,
        InlineEdges::default(),
        padding(inline_start(0.1)),
    );
    let resolved = resolve(&p, BASIS);
    let base = glyphs(&line(&p));
    assert_ne!(base, glyphs(&line(&plain)), "fixture pair should kern");
    assert_eq!(base, glyphs(&line(&resolved)));
    // The percentage side breaks shaping exactly like a nonzero px edge.
    assert_eq!(base, glyphs(&line(&px_edge)));
    assert_eq!(p.data.shape_items.len(), 2);
    assert_eq!(resolved.data.shape_items.len(), 2);
    assert_eq!(plain.data.shape_items.len(), 1);
    assert_eq!(p.data.shaping_barriers, px_edge.data.shaping_barriers);
    assert_eq!(p.data.shaping_barriers, resolved.data.shaping_barriers);
    assert!(close(
        line(&resolved).inline_size(),
        line(&px_edge).inline_size() - 1.0 + 20.0
    ));
    // A box with only block-side percentages keeps shaping across it.
    let block = build_with(
        &style,
        words,
        InlineEdges::default(),
        padding(Sides {
            block_start: 0.1,
            ..Sides::default()
        }),
    );
    assert!(block.has_percent_edges());
    assert_eq!(block.data.shape_items.len(), 1);
    assert_eq!(glyphs(&line(&block)), glyphs(&line(&plain)));
}

#[test]
fn first_line_dataset_is_resolved() {
    let mut style = style();
    style.first_line = Some(InlineStyle {
        font_size: 20.0,
        ..style.root.clone()
    });
    let plain = build_with(
        &style,
        ("ab", "cd", "ef"),
        InlineEdges::default(),
        InlineEdgePercents::default(),
    );
    let p = build_with(
        &style,
        ("ab", "cd", "ef"),
        InlineEdges::default(),
        padding(inline_start(0.1)),
    );
    assert!(p.data.first_line.is_some());
    let resolved = resolve(&p, BASIS);
    let (plain, l) = (line(&plain), line(&resolved));
    // The single line is the first line and uses the first-line font size.
    assert!(
        l.fragments()
            .any(|f| matches!(f, Fragment::GlyphRun(r) if r.font_size() == 20.0))
    );
    assert!(close(l.inline_size(), plain.inline_size() + 20.0));
    assert!(close(run_start(&l, 2), run_start(&plain, 2) + 20.0));
}

#[test]
fn rtl_inline_start_is_on_the_right() {
    let mut style = style();
    style.direction = Direction::Rtl;
    style.root.direction = Direction::Rtl;
    let words = ("ab", "cd", "ef");
    let plain = build_with(
        &style,
        words,
        InlineEdges::default(),
        InlineEdgePercents::default(),
    );
    let mut px = InlineEdges::default();
    px.padding.inline_start = 20.0;
    let px_start = build_with(&style, words, px, InlineEdgePercents::default());
    let resolved = resolve(
        &build_with(
            &style,
            words,
            InlineEdges::default(),
            padding(inline_start(0.1)),
        ),
        BASIS,
    );
    let (plain, px_start, l) = (line(&plain), line(&px_start), line(&resolved));
    assert!(close(l.inline_size(), plain.inline_size() + 20.0));
    // The resolved percentage lands exactly where a 20px inline-start
    // padding does.
    let (b, expected) = (inline_box(&l), inline_box(&px_start));
    assert_eq!(b.rect, expected.rect);
    assert_eq!(b.content_rect, expected.content_rect);
    for offset in [0, 2, 4] {
        assert_eq!(run_start(&l, offset), run_start(&px_start, offset));
    }
    // Runs are placed left to right as "ef", "cd", "ab": the padding opens
    // to the right of the box's text, so only the logically preceding "ab"
    // moves.
    assert!(close(run_start(&l, 0), run_start(&plain, 0) + 20.0));
    assert_eq!(run_start(&l, 2), run_start(&plain, 2));
    assert_eq!(run_start(&l, 4), run_start(&plain, 4));
    assert!(run_start(&l, 4) < run_start(&l, 2) && run_start(&l, 2) < run_start(&l, 0));
}

/// Inline position of the `index`th glyph of a left-to-right line.
fn glyph_start(line: &crate::Line, index: usize) -> f32 {
    line.fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .flat_map(|r| r.glyphs().collect::<Vec<_>>())
        .nth(index)
        .unwrap()
        .inline_position
}

/// Annotation text without a kerning pair at the box boundary.
const WORDS: (&str, &str) = ("ab", "cd");

/// Ruby with base text `base` whose single annotation is `content`.
fn ruby_with(base: &str, content: crate::RubyContent) -> crate::Ruby {
    use crate::ruby::{RubyAlign, RubyAnnotation, RubyBase, RubyLevel, RubySpan, RubyVisibility};
    let style = style();
    crate::Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: crate::RubyContent::text(
                TextSource::Dom {
                    node: NodeId(10),
                    offset: 0,
                },
                base,
                &style.root,
                &Limits::unlimited(),
            ),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content,
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: Default::default(),
        }],
    )
    .unwrap()
}

/// Annotation content `before` + an inline box (node 21) holding `inside`, the
/// box given percentage `percents`; `nested` wraps it in one more ruby level.
fn annotation_content(
    (before, inside): (&str, &str),
    percents: InlineEdgePercents,
    nested: bool,
) -> crate::RubyContent {
    let style = style();
    let limits = Limits::unlimited();
    let mut b = ParagraphBuilder::new(&style, &limits);
    let source = |offset: u32| TextSource::Dom {
        node: NodeId(20),
        offset,
    };
    b.push_text(source(0), before)
        .open_inline(NodeId(21), &style.root, InlineEdges::default())
        .set_inline_edge_percents(percents)
        .push_text(source(before.len() as u32), inside)
        .close_inline();
    let content = crate::RubyContent::from_builder(b);
    if !nested {
        return content;
    }
    let mut outer = ParagraphBuilder::new(&style, &limits);
    // A base text unlike the outer base keeps the lane visible.
    outer.push_ruby(NodeId(30), &style.root, ruby_with("y", content));
    crate::RubyContent::from_builder(outer)
}

/// A paragraph holding only a ruby whose annotation is `content`.
fn build_ruby(content: crate::RubyContent) -> Paragraph {
    let style = style();
    let limits = Limits::unlimited();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_ruby(NodeId(8), &style.root, ruby_with("x", content));
    b.build(&mut LayoutContext::new(), &fonts(&limits)).unwrap()
}

/// The annotation line of the single ruby, descending `depth` nested levels.
fn annotation_line(line: &crate::Line, depth: usize) -> crate::Line {
    let annotation = line.ruby_annotations().next().unwrap();
    let inner = annotation.line().clone();
    if depth == 0 {
        inner
    } else {
        annotation_line(&inner, depth - 1)
    }
}

#[test]
fn annotation_percent_edges_resolve_against_the_parent_basis() {
    for (nested, depth) in [(false, 0), (true, 1)] {
        let plain = build_ruby(annotation_content(
            WORDS,
            InlineEdgePercents::default(),
            nested,
        ));
        assert!(!plain.has_percent_edges());
        let p = build_ruby(annotation_content(
            WORDS,
            padding(inline_start(0.1)),
            nested,
        ));
        // The parent's own items carry no percentages.
        assert!(p.has_percent_edges());
        assert_eq!(p.edge_basis(), 0.0);
        let resolved = resolve(&p, BASIS);
        assert!(resolved.has_percent_edges());
        assert_eq!(resolved.edge_basis(), BASIS);
        assert_ne!(resolved.id(), p.id());
        let (plain, base, wide) = (
            annotation_line(&line(&plain), depth),
            annotation_line(&line(&p), depth),
            annotation_line(&line(&resolved), depth),
        );
        // The base build resolves the annotation percentage against zero.
        assert!(close(base.inline_size(), plain.inline_size()));
        assert!(close(glyph_start(&base, 2), glyph_start(&plain, 2)));
        let pad = |l: &crate::Line| {
            let b = l
                .fragments()
                .find_map(|f| match f {
                    Fragment::InlineBox(b) if b.node == NodeId(21) => Some(b),
                    _ => None,
                })
                .unwrap();
            b.content_rect.inline_start - b.rect.inline_start
        };
        assert!(close(pad(&base), 0.0));
        // 10% of 200 opens 20px before "cd" inside the annotation.
        assert!(close(pad(&wide), 20.0));
        assert!(close(wide.inline_size(), base.inline_size() + 20.0));
        assert!(close(glyph_start(&wide, 0), glyph_start(&base, 0)));
        assert!(close(glyph_start(&wide, 2), glyph_start(&base, 2) + 20.0));
        // Resolving again starts from the recorded input.
        let again = annotation_line(&line(&resolve(&resolved, 0.0)), depth);
        assert!(close(again.inline_size(), base.inline_size()));
        // The annotation paragraph keeps its own percentage metadata.
        let first = |p: &Paragraph| {
            line(p)
                .ruby_annotations()
                .next()
                .unwrap()
                .paragraph()
                .clone()
        };
        let annotation = first(&resolved);
        assert!(annotation.has_percent_edges());
        assert_eq!(annotation.edge_basis(), BASIS);
        assert!(first(&p).has_percent_edges());
        assert_eq!(first(&p).edge_basis(), 0.0);
        let plain = build_ruby(annotation_content(
            WORDS,
            InlineEdgePercents::default(),
            nested,
        ));
        assert!(!first(&plain).has_percent_edges());
        if !nested {
            let rebuilt = resolve(&annotation, 0.0);
            assert_ne!(rebuilt.id(), annotation.id());
            assert_eq!(rebuilt.edge_basis(), 0.0);
            assert!(close(line(&rebuilt).inline_size(), base.inline_size()));
        }
    }
}

#[test]
fn annotation_shaping_is_the_same_for_every_basis() {
    // "AV" kerns in the fixture font when shaped as one run.
    let plain = build_ruby(annotation_content(
        ("A", "V"),
        InlineEdgePercents::default(),
        false,
    ));
    let p = build_ruby(annotation_content(
        ("A", "V"),
        padding(inline_start(0.1)),
        false,
    ));
    let resolved = resolve(&p, BASIS);
    let lane = |p: &Paragraph| {
        let line = line(p);
        let annotation = line.ruby_annotations().next().unwrap();
        let shape_items = annotation.paragraph().data.shape_items.len();
        let barriers = annotation.paragraph().data.shaping_barriers.clone();
        (glyphs(annotation.line()), shape_items, barriers)
    };
    let (plain, base, wide) = (lane(&plain), lane(&p), lane(&resolved));
    assert_ne!(base.0, plain.0, "fixture pair should kern");
    assert_eq!(plain.1, 1);
    assert_eq!(base.1, 2);
    assert_eq!(base, wide);
}

#[test]
fn ruby_paragraph_without_percents_is_not_rebuilt() {
    let p = build_ruby(annotation_content(
        WORDS,
        InlineEdgePercents::default(),
        true,
    ));
    assert!(!p.has_percent_edges());
    let resolved = resolve(&p, BASIS);
    assert_eq!(resolved.id(), p.id());
    assert_eq!(resolved.edge_basis(), 0.0);
    // Percentages that sanitize to zero do not count either.
    let p = build_ruby(annotation_content(
        WORDS,
        padding(inline_start(f32::NAN)),
        false,
    ));
    assert!(!p.has_percent_edges());
    assert_eq!(resolve(&p, BASIS).id(), p.id());
}

#[test]
fn first_line_annotation_keeps_percent_metadata() {
    let mut style = style();
    style.first_line = Some(InlineStyle {
        font_size: 20.0,
        ..style.root.clone()
    });
    let limits = Limits::unlimited();
    let mut b = ParagraphBuilder::new(&style, &limits);
    let source = |offset: u32| TextSource::Dom {
        node: NodeId(20),
        offset,
    };
    b.push_text(source(0), "ab")
        .open_inline(NodeId(21), &style.root, InlineEdges::default())
        .set_inline_edge_percents(padding(inline_start(0.1)))
        .push_text(source(2), "cd")
        .close_inline();
    let p = build_ruby(crate::RubyContent::from_builder(b));
    let resolved = resolve(&p, BASIS);
    let first = line(&resolved);
    let annotation = first.ruby_annotations().next().unwrap().paragraph();
    // The first line exposes the annotation's first-line dataset.
    assert!(annotation.has_percent_edges());
    assert_eq!(annotation.edge_basis(), BASIS);
    let rebuilt = resolve(annotation, 0.0);
    assert_ne!(rebuilt.id(), annotation.id());
    assert_eq!(rebuilt.edge_basis(), 0.0);
}
