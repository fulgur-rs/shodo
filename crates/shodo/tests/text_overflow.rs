//! `text-overflow: ellipsis` truncation of laid-out lines.

mod common;

use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::geometry::Direction;
use shodo::hit::{LineLayout, TextPosition};
use shodo::limits::Limits;
use shodo::mapping::Affinity;
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{FontFamily, LineOptions, ParagraphStyle, WhiteSpaceCollapse};
use shodo::{AtomicSize, AtomicSizes, Fragment, LayoutContext, Line, Paragraph, ParagraphBuilder};

fn fonts() -> FontCollection {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Fixture".into(),
                ..Default::default()
            },
        )
        .unwrap();
    fonts
}

fn style() -> ParagraphStyle {
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Fixture".into())];
    style.root.font_size = 10.0;
    style.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    style
}

fn build(style: &ParagraphStyle, input: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
    let mut b = ParagraphBuilder::new(style, &Limits::default());
    input(&mut b);
    b.build(&mut LayoutContext::new(), &fonts()).unwrap()
}

fn text(style: &ParagraphStyle, text: &str) -> Paragraph {
    build(style, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, text);
    })
}

/// The whole paragraph on one line, as `nowrap` lays it out.
fn line(p: &Paragraph, atomics: &AtomicSizes) -> Line {
    common::first_line(p, 1e6, &LineOptions::default(), atomics)
}

/// Glyph runs as (text range, inline start, inline size, ellipsis).
fn runs(line: &Line) -> Vec<(std::ops::Range<usize>, f32, f32, bool)> {
    line.fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(run) => Some((
                run.text_range(),
                run.inline_start(),
                run.inline_size(),
                run.is_ellipsis(),
            )),
            _ => None,
        })
        .collect()
}

fn ellipsis_width(style: &ParagraphStyle) -> f32 {
    line(&text(style, "..."), &AtomicSizes::EMPTY).inline_size()
}

#[test]
fn fitting_lines_are_left_alone() {
    let style = style();
    let p = text(&style, "abc");
    let mut l = line(&p, &AtomicSizes::EMPTY);
    let before = runs(&l);
    let width = l.inline_size();
    assert_eq!(
        l.truncate_with_ellipsis(&mut LayoutContext::new(), width),
        None
    );
    assert_eq!(
        l.truncate_with_ellipsis(&mut LayoutContext::new(), f32::NAN),
        None
    );
    assert_eq!(runs(&l), before);
}

#[test]
fn hides_clusters_that_would_overlap_the_ellipsis() {
    let style = style();
    let dots = ellipsis_width(&style);
    let p = text(&style, "abcdefghij");
    let mut l = line(&p, &AtomicSizes::EMPTY);
    let full = l.inline_size();
    let available = full / 2.0;
    let cut = l
        .truncate_with_ellipsis(&mut LayoutContext::new(), available)
        .expect("overflowing line truncates");
    let runs = runs(&l);
    let (kept, start, size, ellipsis) = runs[0].clone();
    assert!(!ellipsis);
    assert_eq!(kept.start, 0);
    assert!(start + size <= available - dots + 0.01, "{runs:?}");
    // The ellipsis follows the remaining content and fits.
    let (dots_text, dots_start, dots_size, is_ellipsis) = runs.last().unwrap().clone();
    assert!(is_ellipsis);
    assert_eq!(dots_text, kept.end..kept.end);
    assert_eq!(dots_start, start + size);
    assert_eq!((cut.inline_start, cut.inline_size), (dots_start, dots_size));
    assert!((dots_size - dots).abs() < 0.01);
    assert!(dots_start + dots_size <= available + 0.01);
    assert_eq!(cut.fragments, runs.len() - 1..runs.len());
    assert_eq!(l.inline_size(), dots_start + dots_size);
    // One more cluster would not have fit.
    let next = line(
        &text(&style, &"abcdefghij"[..kept.end + 1]),
        &AtomicSizes::EMPTY,
    );
    assert!(next.inline_size() > available - dots);
    // The ellipsis paints with the root style and has no source node.
    let run = l
        .fragments()
        .find_map(|f| match f {
            Fragment::GlyphRun(run) if run.is_ellipsis() => Some(run),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (run.node(), run.source(), run.style_index()),
        (None, None, 0)
    );
    assert_eq!(run.glyphs().len(), 3);
    // A truncated line does not truncate again.
    assert_eq!(
        l.truncate_with_ellipsis(&mut LayoutContext::new(), 1.0),
        None
    );
}

#[test]
fn rtl_lines_truncate_at_their_inline_end() {
    let mut style = style();
    style.direction = Direction::Rtl;
    style.root.direction = Direction::Rtl;
    let dots = ellipsis_width(&style);
    let p = text(&style, "abcdefghij");
    let mut l = line(&p, &AtomicSizes::EMPTY);
    let available = l.inline_size() / 2.0;
    l.truncate_with_ellipsis(&mut LayoutContext::new(), available)
        .unwrap();
    let runs = runs(&l);
    // The left-to-right run displays reversed against the paragraph, so its
    // logical end sits at inline-start and stays: "…hij".
    let (text, start, size, _) = runs[0].clone();
    assert_eq!(text.end, 10, "{runs:?}");
    assert!(text.start > 0);
    assert!(start + size <= available - dots + 0.01, "{runs:?}");
    assert!(runs.last().unwrap().3);
}

#[test]
fn reversed_runs_keep_their_logical_suffix() {
    // A left-to-right paragraph with a right-to-left override: the run
    // displays reversed, so its logical end is nearest inline-start.
    let style = style();
    let mut rtl = style.root.clone();
    rtl.direction = Direction::Rtl;
    rtl.unicode_bidi = shodo::style::UnicodeBidi::BidiOverride;
    let dots = ellipsis_width(&style);
    let p = build(&style, |b| {
        b.open_inline(NodeId(2), &rtl, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "abcdefghij")
            .close_inline();
    });
    let mut l = line(&p, &AtomicSizes::EMPTY);
    let available = l.inline_size() / 2.0;
    l.truncate_with_ellipsis(&mut LayoutContext::new(), available)
        .unwrap();
    let runs = runs(&l);
    let (text, start, size, _) = runs[0].clone();
    // The processed text starts with the override's control character.
    assert_eq!(text.end, p.text().len() - 3, "{runs:?}");
    assert!(text.start > 3);
    assert!(start + size <= available - dots + 0.01);
    // Remaining glyphs keep their positions.
    let original = line(&p, &AtomicSizes::EMPTY);
    let glyphs = |l: &Line| {
        l.fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(run) if !run.is_ellipsis() => Some(
                    run.glyphs()
                        .map(|g| (g.cluster, g.inline_position))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>()
    };
    let kept = glyphs(&l);
    let all = glyphs(&original);
    assert!(kept.iter().all(|g| all.contains(g)), "{kept:?} {all:?}");
}

#[test]
fn atomics_hide_and_boxes_keep_their_geometry() {
    let style = style();
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(5),
        AtomicSize {
            inline_size: 30.0,
            block_size: 10.0,
            ..Default::default()
        },
    );
    let edges = InlineEdges {
        padding: Sides {
            inline_end: 4.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let p = build(&style, |b| {
        b.open_inline(NodeId(2), &style.root, edges)
            .push_text(TextSource::Generated { node: NodeId(3) }, "ab")
            .push_atomic(NodeId(5), &style.root, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(4) }, "cd")
            .close_inline();
    });
    let mut l = line(&p, &atomics);
    let original_box = l
        .fragments()
        .find_map(|f| match f {
            Fragment::InlineBox(b) => Some(b),
            _ => None,
        })
        .unwrap();
    let text_width = line(&text(&style, "ab"), &AtomicSizes::EMPTY).inline_size();
    let available = text_width + ellipsis_width(&style) + 10.0;
    let cut = l
        .truncate_with_ellipsis(&mut LayoutContext::new(), available)
        .unwrap();
    let mut boxes = 0;
    for fragment in l.fragments() {
        match fragment {
            Fragment::Atomic(_) => panic!("the atomic inline does not fit"),
            Fragment::InlineBox(b) => {
                // As in Blink, the caller's overflow clip trims the box.
                boxes += 1;
                assert_eq!(b, original_box);
            }
            _ => {}
        }
    }
    assert_eq!(boxes, 1);
    assert_eq!(runs(&l).len(), 2);
    assert!((cut.inline_start - text_width).abs() < 0.01);
}

#[test]
fn the_first_cluster_stays_when_nothing_fits() {
    let style = style();
    let p = text(&style, "abcdef");
    let mut l = line(&p, &AtomicSizes::EMPTY);
    l.truncate_with_ellipsis(&mut LayoutContext::new(), 1.0)
        .unwrap();
    let runs = runs(&l);
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(runs[0].0, 0..1);
    assert_eq!(runs[1].1, runs[0].1 + runs[0].2);
}

#[test]
fn hit_testing_skips_hidden_text() {
    let style = style();
    let p = text(&style, "abcdefghij");
    let mut l = line(&p, &AtomicSizes::EMPTY);
    let available = l.inline_size() / 2.0;
    l.truncate_with_ellipsis(&mut LayoutContext::new(), available)
        .unwrap();
    let lines = [l];
    let layout = LineLayout::new(&lines);
    for x in 0..120 {
        let _ = layout.hit_test(x as f32, 5.0);
    }
    for offset in 0..=10 {
        for affinity in [Affinity::Upstream, Affinity::Downstream] {
            let _ = layout.caret(TextPosition {
                line: 0,
                offset,
                affinity,
            });
        }
    }
    let hit = layout.hit_test(1000.0, 5.0).expect("hit");
    assert!(hit.position.offset <= 10);
    let _ = layout.selection_rects(
        TextPosition {
            line: 0,
            offset: 0,
            affinity: Affinity::Downstream,
        },
        TextPosition {
            line: 0,
            offset: 10,
            affinity: Affinity::Upstream,
        },
    );
    let accessible = shodo::accessibility::AccessibleLayout::new(&lines);
    // Assistive technology still reads the hidden text.
    assert_eq!(accessible.logical_text(), "abcdefghij");
    let _ = lines[0].overflow_rect();
    let _ = lines[0].paint_spans();
}

#[test]
fn ruby_hides_with_its_annotations() {
    use shodo::{Ruby, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan};
    let style = style();
    let limits = Limits::default();
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: RubyContent::text(
                TextSource::Generated { node: NodeId(11) },
                "defg",
                &style.root,
                &limits,
            ),
            align: Default::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(12),
                content: RubyContent::text(
                    TextSource::Generated { node: NodeId(13) },
                    "x",
                    &style.root,
                    &limits,
                ),
                span: RubySpan::Auto,
                visibility: Default::default(),
            }],
            style: Default::default(),
        }],
    )
    .unwrap();
    let p = build(&style, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "abc");
        b.push_ruby(NodeId(9), &style.root, ruby);
        b.push_text(TextSource::Generated { node: NodeId(2) }, "hij");
    });
    let mut l = line(&p, &AtomicSizes::EMPTY);
    assert_eq!(l.ruby_annotations().len(), 1);
    let abc = line(&text(&style, "abc"), &AtomicSizes::EMPTY).inline_size();
    let abcde = line(&text(&style, "abcde"), &AtomicSizes::EMPTY).inline_size();
    // Room for "abcde…": the base would be cut, so all of it goes.
    let available = abcde + ellipsis_width(&style) + 0.5;
    let cut = l
        .truncate_with_ellipsis(&mut LayoutContext::new(), available)
        .unwrap();
    assert_eq!(l.ruby_annotations().len(), 0);
    assert!((cut.inline_start - abc).abs() < 0.01, "{:?}", runs(&l));
    assert!(
        runs(&l)
            .iter()
            .all(|(text, _, _, ellipsis)| *ellipsis || text.end <= 3),
        "{:?}",
        runs(&l)
    );
}

#[test]
fn negative_letter_spacing_keeps_glyph_positions() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        let mut style = style();
        style.direction = direction;
        style.root.direction = direction;
        style.root.letter_spacing = -4.0;
        let p = text(&style, "iiiiiiWWWWWWiiii");
        let original = line(&p, &AtomicSizes::EMPTY);
        let positions = |l: &Line| {
            l.fragments()
                .filter_map(|f| match f {
                    Fragment::GlyphRun(run) if !run.is_ellipsis() => Some(
                        run.glyphs()
                            .map(|g| (g.cluster, g.inline_position))
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                })
                .flatten()
                .collect::<Vec<_>>()
        };
        let all = positions(&original);
        for step in 1..40 {
            let mut l = original.clone();
            if l.truncate_with_ellipsis(&mut LayoutContext::new(), step as f32 * 2.0)
                .is_none()
            {
                continue;
            }
            let kept = positions(&l);
            assert!(!kept.is_empty());
            assert!(kept.iter().all(|g| all.contains(g)), "{direction:?} {step}");
        }
    }
}

#[test]
fn boxes_cut_in_their_start_edges_keep_their_geometry() {
    let style = style();
    let edges = InlineEdges {
        margin: Sides {
            inline_start: 5.0,
            ..Default::default()
        },
        padding: Sides {
            inline_start: 20.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let p = build(&style, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "abcd");
        b.open_inline(NodeId(2), &style.root, edges)
            .push_text(TextSource::Generated { node: NodeId(3) }, "efghij")
            .close_inline();
    });
    let mut l = line(&p, &AtomicSizes::EMPTY);
    let abcd = line(&text(&style, "abcd"), &AtomicSizes::EMPTY).inline_size();
    l.truncate_with_ellipsis(
        &mut LayoutContext::new(),
        abcd + ellipsis_width(&style) + 3.0,
    )
    .unwrap();
    for fragment in l.fragments() {
        if let Fragment::InlineBox(b) = fragment {
            assert!(
                b.rect.inline_size >= 0.0 && b.content_rect.inline_size >= 0.0,
                "{b:?}"
            );
        }
    }
}

#[test]
fn tabs_end_at_the_ellipsis() {
    let style = style();
    let p = text(&style, "ab\tcdefghijklmnop");
    let mut l = line(&p, &AtomicSizes::EMPTY);
    let ab = line(&text(&style, "ab"), &AtomicSizes::EMPTY).inline_size();
    let cut = l
        .truncate_with_ellipsis(&mut LayoutContext::new(), ab + ellipsis_width(&style) + 4.0)
        .unwrap();
    let lines = [l];
    let layout = LineLayout::new(&lines);
    let rects = layout.selection_rects(
        TextPosition {
            line: 0,
            offset: 0,
            affinity: Affinity::Downstream,
        },
        TextPosition {
            line: 0,
            offset: 3,
            affinity: Affinity::Upstream,
        },
    );
    for rect in rects {
        assert!(
            rect.inline_start + rect.inline_size <= cut.inline_start + 0.01,
            "{rect:?}"
        );
    }
}
