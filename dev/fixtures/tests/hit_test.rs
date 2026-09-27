use shodo::hit::{LineLayout, TextPosition};
use shodo::mapping::Affinity;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, LayoutContext, Line, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1.0 / 32.0, "{a} vs {b}");
}
fn style() -> InlineStyle {
    InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[0].family.into())],
        font_size: 20.0,
        ..Default::default()
    }
}
fn layout(text: &str, mapping: bool) -> Vec<Line> {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(),
            ..Default::default()
        },
        &limits,
    );
    b.with_offset_mapping(mapping).push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        text,
    );
    b.build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        )
}
fn position(offset: u32, affinity: Affinity) -> TextPosition {
    TextPosition {
        line: 0,
        offset,
        affinity,
    }
}

#[test]
fn glyph_midpoints_and_caret_round_trips_without_mapping() {
    for mapping in [false, true] {
        let lines = layout("ab", mapping);
        let index = LineLayout::new(&lines);
        let glyphs: Vec<_> = lines[0]
            .fragments()
            .filter_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .flat_map(|r| r.glyphs())
            .collect();
        let start = index.caret(position(0, Affinity::Downstream)).unwrap();
        let middle = index.caret(position(1, Affinity::Downstream)).unwrap();
        close(start.rect.inline_start, 0.0);
        close(middle.rect.inline_start, glyphs[0].advance);
        let block = middle.rect.block_start + middle.rect.block_size / 2.0;
        assert_eq!(
            index
                .hit_test(glyphs[0].advance * 0.25, block)
                .unwrap()
                .position
                .offset,
            0
        );
        let hit = index.hit_test(glyphs[0].advance * 0.75, block).unwrap();
        assert_eq!(hit.position.offset, 1);
        assert!(hit.inside);
        assert_eq!(hit.origin.is_some(), mapping);
        close(
            index.caret(hit.position).unwrap().rect.inline_start,
            middle.rect.inline_start,
        );
        assert!(index.hit_test(f32::NAN, block).is_none());
        assert!(!index.hit_test(-100.0, block).unwrap().inside);
        assert_eq!(
            index
                .hit_test(f32::INFINITY, block)
                .unwrap()
                .position
                .offset,
            2
        );
        assert!(index.caret(position(3, Affinity::Downstream)).is_none());
    }
}

#[test]
fn bidi_affinity_has_two_visual_locations() {
    let lines = layout("aאבz", false);
    let index = LineLayout::new(&lines);
    let rtl = lines[0]
        .fragments()
        .find_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                if r.bidi_level() % 2 == 1 {
                    Some(r)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap();
    let upstream = index.caret(position(1, Affinity::Upstream)).unwrap();
    let downstream = index.caret(position(1, Affinity::Downstream)).unwrap();
    close(upstream.rect.inline_start, rtl.inline_start());
    close(
        downstream.rect.inline_start,
        rtl.inline_start() + rtl.inline_size(),
    );
    assert!(downstream.rect.inline_start > upstream.rect.inline_start);
    close(
        index
            .caret(position(5, Affinity::Upstream))
            .unwrap()
            .rect
            .inline_start,
        rtl.inline_start(),
    );
    close(
        index
            .caret(position(5, Affinity::Downstream))
            .unwrap()
            .rect
            .inline_start,
        rtl.inline_start() + rtl.inline_size(),
    );
}

#[test]
fn ligature_and_combining_carets_preserve_graphemes() {
    let lines = layout("ffi", false);
    let index = LineLayout::new(&lines);
    let width = lines[0].inline_size();
    let glyphs = lines[0]
        .fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .flat_map(|r| r.glyphs())
        .count();
    assert_eq!(glyphs, 1);
    let one = index
        .caret(position(1, Affinity::Downstream))
        .unwrap()
        .rect
        .inline_start;
    let two = index
        .caret(position(2, Affinity::Downstream))
        .unwrap()
        .rect
        .inline_start;
    assert!(one > 0.0 && two > one && width > two);
    for text in ["e\u{301}", "👩\u{200d}👩\u{200d}👧"] {
        let lines = layout(text, false);
        let index = LineLayout::new(&lines);
        for byte in 1..text.len() as u32 {
            assert_eq!(
                index
                    .caret(position(byte, Affinity::Upstream))
                    .unwrap()
                    .position
                    .offset,
                0
            );
            assert_eq!(
                index
                    .caret(position(byte, Affinity::Downstream))
                    .unwrap()
                    .position
                    .offset,
                text.len() as u32
            );
        }
    }
}

#[test]
fn first_line_and_indivisible_transform_carets_use_actual_dataset() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut first = style();
    first.text_transform = shodo::style::TextTransform::Uppercase;
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(),
            first_line: Some(first),
            ..Default::default()
        },
        &limits,
    );
    b.with_offset_mapping(false)
        .push_text(TextSource::Generated { node: NodeId(1) }, "ßa")
        .push_forced_break(NodeId(2))
        .push_text(TextSource::Generated { node: NodeId(3) }, "ßa");
    let lines = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
    assert_eq!(lines.len(), 2);
    assert!(lines[0].text().starts_with("SSA"));
    assert!(lines[1].text().ends_with("ßa"));
    let index = LineLayout::new(&lines);
    assert_eq!(
        index
            .caret(position(1, Affinity::Upstream))
            .unwrap()
            .position
            .offset,
        0
    );
    assert_eq!(
        index
            .caret(position(1, Affinity::Downstream))
            .unwrap()
            .position
            .offset,
        2
    );
    close(
        index
            .caret(position(0, Affinity::Downstream))
            .unwrap()
            .rect
            .inline_start,
        0.0,
    );
    close(
        index
            .caret(position(2, Affinity::Upstream))
            .unwrap()
            .rect
            .inline_start,
        lines[0]
            .fragments()
            .filter_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .flat_map(|r| r.glyphs())
            .take(2)
            .map(|g| g.advance)
            .sum(),
    );
    let offset = lines[1].text_range().start as u32;
    let caret = index
        .caret(TextPosition {
            line: 1,
            offset,
            affinity: Affinity::Downstream,
        })
        .unwrap();
    let run = lines[1]
        .fragments()
        .find_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .unwrap();
    close(
        caret.rect.block_start,
        lines[1].block_offset() + run.baseline() - run.metrics().ascent,
    );
    assert_eq!(
        index
            .hit_test(
                caret.rect.inline_start,
                caret.rect.block_start + caret.rect.block_size / 2.0
            )
            .unwrap()
            .position
            .line,
        1
    );
}

#[test]
fn tab_carets_use_final_layout_slots_in_both_directions() {
    for direction in [
        shodo::geometry::Direction::Ltr,
        shodo::geometry::Direction::Rtl,
    ] {
        let limits = Default::default();
        let fonts = load_fonts(&limits).unwrap();
        let mut root = style();
        root.white_space_collapse = shodo::style::WhiteSpaceCollapse::Preserve;
        root.tab_size = shodo::style::TabSize::Px(50.0);
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root,
                direction,
                ..Default::default()
            },
            &limits,
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a\tb");
        let lines = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap()
            .break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                1000.0,
                &AtomicSizes::EMPTY,
            );
        let index = LineLayout::new(&lines);
        let before = index.caret(position(1, Affinity::Downstream)).unwrap();
        let after = index.caret(position(2, Affinity::Upstream)).unwrap();
        close(after.rect.inline_start, 50.0);
        assert!(after.rect.inline_start > before.rect.inline_start);
        let middle = (before.rect.inline_start + after.rect.inline_start) / 2.0;
        assert!(
            index
                .hit_test(
                    middle,
                    before.rect.block_start + before.rect.block_size / 2.0
                )
                .unwrap()
                .inside
        );
    }
}

#[test]
fn justified_and_owned_overlay_hit_geometry_matches_glyphs() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(),
            ..Default::default()
        },
        &limits,
    );
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a b c");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let options = shodo::style::LineOptions {
        text_align: shodo::style::TextAlign::JustifyAll,
        text_justify: shodo::style::TextJustify::InterWord,
        ..Default::default()
    };
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &options,
        100.0,
        &AtomicSizes::EMPTY,
    );
    let index = LineLayout::new(&lines);
    let glyph = lines[0]
        .fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .flat_map(|r| r.glyphs())
        .find(|g| g.cluster == 4)
        .unwrap();
    close(
        index
            .caret(position(4, Affinity::Downstream))
            .unwrap()
            .rect
            .inline_start,
        glyph.inline_position,
    );
    close(
        index
            .caret(position(5, Affinity::Upstream))
            .unwrap()
            .rect
            .inline_start,
        100.0,
    );
    {
        let limits = shodo::limits::Limits::default();
        let fonts = load_fonts(&limits).unwrap();
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(),
                ..Default::default()
            },
            &limits,
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a\u{ad}bc");
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let width = (10..50)
            .map(|v| v as f32)
            .find(|width| {
                p.break_all(
                    &mut LayoutContext::new(),
                    &Default::default(),
                    *width,
                    &AtomicSizes::EMPTY,
                )[0]
                .text_range()
                .end == 3
            })
            .unwrap();
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        );
        let index = LineLayout::new(&lines);
        close(
            index
                .caret(position(3, Affinity::Upstream))
                .unwrap()
                .rect
                .inline_start,
            lines[0].inline_size(),
        );
        let hyphen_start = index.caret(position(1, Affinity::Downstream)).unwrap();
        assert!(hyphen_start.rect.inline_start < lines[0].inline_size());
        let hit = index
            .hit_test(
                lines[0].inline_size() - 0.01,
                hyphen_start.rect.block_start + hyphen_start.rect.block_size / 2.0,
            )
            .unwrap();
        assert_eq!(hit.position.line, 0);
        assert_eq!(hit.position.offset, 3);
    }
}

#[test]
fn atomic_empty_and_control_only_lines_have_safe_carets() {
    for text in ["", "\u{200e}", "\u{ad}"] {
        let lines = layout(text, false);
        let index = LineLayout::new(&lines);
        if lines.is_empty() {
            assert!(index.hit_test(0.0, 0.0).is_none());
            continue;
        }
        let caret = index.caret(position(0, Affinity::Downstream)).unwrap();
        assert_eq!(caret.rect.inline_size, 0.0);
        assert!(caret.rect.inline_start.is_finite());
        let hit = index.hit_test(0.0, 0.0).unwrap();
        assert!(!hit.inside);
    }
    assert!(LineLayout::new(&[]).hit_test(0.0, 0.0).is_none());
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(),
            ..Default::default()
        },
        &limits,
    );
    b.push_atomic(NodeId(1), &style(), Default::default());
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut sizes = AtomicSizes::new();
    sizes.insert(
        NodeId(1),
        shodo::AtomicSize {
            inline_size: 30.0,
            block_size: 20.0,
            baseline: Some(15.0),
            ..Default::default()
        },
    );
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        1000.0,
        &sizes,
    );
    let index = LineLayout::new(&lines);
    close(
        index
            .caret(position(0, Affinity::Downstream))
            .unwrap()
            .rect
            .inline_start,
        0.0,
    );
    close(
        index
            .caret(position(3, Affinity::Upstream))
            .unwrap()
            .rect
            .inline_start,
        30.0,
    );
    let atom = lines[0]
        .fragments()
        .find_map(|f| {
            if let Fragment::Atomic(a) = f {
                Some(a)
            } else {
                None
            }
        })
        .unwrap();
    assert!(
        index
            .hit_test(15.0, atom.margin_rect.block_start + 10.0)
            .unwrap()
            .inside
    );
}

#[test]
fn whole_cluster_resource_fallback_carets_use_accepted_source() {
    let limits = shodo::limits::Limits {
        max_reshape_window_bytes: Some(0),
        ..Default::default()
    };
    let fonts = load_fonts(&limits).unwrap();
    let mut root = style();
    root.overflow_wrap = shodo::style::OverflowWrap::Anywhere;
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root,
            ..Default::default()
        },
        &limits,
    );
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ffi");
    let lines = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1.0,
            &AtomicSizes::EMPTY,
        );
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text_range(), 0..3);
    let index = LineLayout::new(&lines);
    close(
        index
            .caret(position(0, Affinity::Downstream))
            .unwrap()
            .rect
            .inline_start,
        0.0,
    );
    close(
        index
            .caret(position(3, Affinity::Upstream))
            .unwrap()
            .rect
            .inline_start,
        lines[0].inline_size(),
    );
    let a = index
        .caret(position(1, Affinity::Downstream))
        .unwrap()
        .rect
        .inline_start;
    let b = index
        .caret(position(2, Affinity::Downstream))
        .unwrap()
        .rect
        .inline_start;
    assert!(a > 0.0 && b > a && b < lines[0].inline_size());
}

#[test]
fn selections_keep_bidi_gaps_and_partial_ligatures() {
    let lines = layout("aאבz", false);
    let index = LineLayout::new(&lines);
    let rects = index.selection_rects(
        position(0, Affinity::Downstream),
        position(3, Affinity::Upstream),
    );
    assert_eq!(rects.len(), 2);
    let a = index
        .caret(position(1, Affinity::Upstream))
        .unwrap()
        .rect
        .inline_start;
    let m = index
        .caret(position(3, Affinity::Upstream))
        .unwrap()
        .rect
        .inline_start;
    let end = index
        .caret(position(1, Affinity::Downstream))
        .unwrap()
        .rect
        .inline_start;
    close(rects[0].inline_start, 0.0);
    close(rects[0].inline_size, a);
    close(rects[1].inline_start, m);
    close(rects[1].inline_size, end - m);
    assert_eq!(
        rects,
        index.selection_rects(
            position(3, Affinity::Upstream),
            position(0, Affinity::Downstream)
        )
    );
    assert!(
        index
            .selection_rects(
                position(1, Affinity::Upstream),
                position(1, Affinity::Downstream)
            )
            .is_empty()
    );
    let lines = layout("ffi", false);
    let index = LineLayout::new(&lines);
    let rect = index.selection_rects(
        position(1, Affinity::Downstream),
        position(2, Affinity::Upstream),
    );
    assert_eq!(rect.len(), 1);
    close(rect[0].inline_start, lines[0].inline_size() / 3.0);
    close(rect[0].inline_size, lines[0].inline_size() / 3.0);
}

#[test]
fn logical_and_visual_navigation_keep_distinct_bidi_affinities() {
    use shodo::hit::{CaretDirection, NavigationOrder};
    let lines = layout("aאבz", false);
    let index = LineLayout::new(&lines);
    for (order, expected) in [
        (NavigationOrder::Logical, vec![0, 1, 3, 5, 6]),
        (NavigationOrder::Visual, vec![0, 1, 3, 1, 6]),
    ] {
        let mut stop = position(0, Affinity::Downstream);
        let mut offsets = vec![0];
        let mut xs = vec![0.0];
        while let Some(next) = index.move_caret(stop, CaretDirection::Forward, order) {
            stop = next;
            offsets.push(stop.offset);
            xs.push(index.caret(stop).unwrap().rect.inline_start);
            assert!(offsets.len() < 10);
        }
        assert_eq!(offsets, expected);
        if order == NavigationOrder::Visual {
            assert!(xs.windows(2).all(|w| w[1] > w[0]));
        }
        for _ in 1..offsets.len() {
            stop = index
                .move_caret(stop, CaretDirection::Backward, order)
                .unwrap();
        }
        assert_eq!(stop.offset, 0);
        assert!(
            index
                .move_caret(stop, CaretDirection::Backward, order)
                .is_none()
        );
    }
}

#[test]
fn cross_line_selection_and_navigation_survive_owner_drop() {
    use shodo::hit::{CaretDirection, NavigationOrder};
    let lines = {
        let limits = Default::default();
        let fonts = load_fonts(&limits).unwrap();
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(),
                ..Default::default()
            },
            &limits,
        );
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "a",
        )
        .push_forced_break(NodeId(2))
        .push_text(
            TextSource::Dom {
                node: NodeId(3),
                offset: 0,
            },
            "b",
        );
        b.build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap()
            .break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                1000.0,
                &AtomicSizes::EMPTY,
            )
    };
    assert_eq!(lines.len(), 2);
    let index = LineLayout::new(&lines);
    let start = position(lines[0].text_range().start as u32, Affinity::Downstream);
    let end = TextPosition {
        line: 1,
        offset: lines[1].text_range().end as u32,
        affinity: Affinity::Upstream,
    };
    let rects = index.selection_rects(start, end);
    assert_eq!(rects.len(), 2);
    close(rects[0].inline_size, lines[0].inline_size());
    close(rects[1].inline_size, lines[1].inline_size());
    assert!(rects[1].block_start > rects[0].block_start);
    assert_eq!(rects, index.selection_rects(end, start));
    for order in [NavigationOrder::Logical, NavigationOrder::Visual] {
        let boundary = position(lines[0].text_range().end as u32, Affinity::Upstream);
        let next = index
            .move_caret(boundary, CaretDirection::Forward, order)
            .unwrap();
        assert_eq!(next.line, 1);
        assert_eq!(next.offset, lines[1].text_range().start as u32);
        assert!(
            index
                .move_caret(end, CaretDirection::Forward, order)
                .is_none()
        );
    }
    assert!(
        index
            .selection_rects(start, TextPosition { line: 99, ..end })
            .is_empty()
    );
    assert!(index.hit_test(0.0, f32::INFINITY).unwrap().position.line == 1);
}

#[test]
fn expanded_selection_and_control_navigation_terminate() {
    use shodo::hit::{CaretDirection, NavigationOrder};
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut root = style();
    root.text_transform = shodo::style::TextTransform::Uppercase;
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root,
            ..Default::default()
        },
        &limits,
    );
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "ß",
    );
    let lines = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
    let index = LineLayout::new(&lines);
    let rect = index.selection_rects(
        position(1, Affinity::Upstream),
        position(1, Affinity::Downstream),
    );
    assert_eq!(rect.len(), 1);
    close(rect[0].inline_size, lines[0].inline_size());
    for order in [NavigationOrder::Logical, NavigationOrder::Visual] {
        let next = index
            .move_caret(
                position(0, Affinity::Downstream),
                CaretDirection::Forward,
                order,
            )
            .unwrap();
        assert_eq!(next.offset, 2);
        assert!(
            index
                .move_caret(next, CaretDirection::Forward, order)
                .is_none()
        );
    }
    let lines = layout("\u{200e}\u{200e}", false);
    let index = LineLayout::new(&lines);
    if !lines.is_empty() {
        for order in [NavigationOrder::Logical, NavigationOrder::Visual] {
            let mut p = position(0, Affinity::Downstream);
            for step in 0..5 {
                let Some(next) = index.move_caret(p, CaretDirection::Forward, order) else {
                    break;
                };
                p = next;
                assert!(step < 4);
            }
        }
    }
}

#[test]
fn collapsed_mapping_and_first_line_selections_use_line_sources() {
    let lines = layout("a   b", true);
    assert_eq!(lines[0].text(), "a b");
    let index = LineLayout::new(&lines);
    let rect = index.selection_rects(
        position(2, Affinity::Downstream),
        position(3, Affinity::Upstream),
    );
    assert_eq!(rect.len(), 1);
    let run = lines[0]
        .fragments()
        .find_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .unwrap();
    let glyph = run.glyphs().find(|g| g.cluster == 2).unwrap();
    close(rect[0].inline_start, glyph.inline_position);
    close(rect[0].inline_size, glyph.advance);
    let hit = index
        .hit_test(
            rect[0].inline_start + 0.01,
            rect[0].block_start + rect[0].block_size / 2.0,
        )
        .unwrap();
    assert_eq!(
        hit.origin,
        Some(shodo::mapping::TextOrigin::Dom {
            node: NodeId(1),
            offset: 4
        })
    );
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut first = style();
    first.text_transform = shodo::style::TextTransform::Uppercase;
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(),
            first_line: Some(first),
            ..Default::default()
        },
        &limits,
    );
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "ßa",
    )
    .push_forced_break(NodeId(2))
    .push_text(
        TextSource::Dom {
            node: NodeId(3),
            offset: 0,
        },
        "ßa",
    );
    let lines = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
    let index = LineLayout::new(&lines);
    let start = position(0, Affinity::Downstream);
    let end = TextPosition {
        line: 1,
        offset: lines[1].text_range().end as u32,
        affinity: Affinity::Upstream,
    };
    let rects = index.selection_rects(start, end);
    assert_eq!(rects.len(), 2);
    close(rects[0].inline_size, lines[0].inline_size());
    close(rects[1].inline_size, lines[1].inline_size());
}
