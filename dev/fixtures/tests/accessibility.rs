use shodo::accessibility::{
    AccessibleCharacterKind, AccessibleLayout, AccessibleSelection, SourcePosition,
};
use shodo::geometry::WritingMode;
use shodo::hit::TextPosition;
use shodo::mapping::{Affinity, TextOrigin};
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, OverflowWrap, ParagraphStyle, TextCombineUpright, TextTransform,
    UnicodeBidi, WhiteSpaceCollapse,
};
use shodo::{AtomicSizes, GlyphOrientation, LayoutContext, Line, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};

fn root() -> InlineStyle {
    InlineStyle {
        font_families: FONTS
            .iter()
            .map(|f| FontFamily::Named(f.family.into()))
            .collect(),
        font_size: 16.0,
        ..Default::default()
    }
}
fn lines(text: &str, style: ParagraphStyle, width: f32, mapping: bool) -> Vec<Line> {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.with_offset_mapping(mapping).push_text(
        TextSource::Dom {
            node: NodeId(42),
            offset: 10,
        },
        text,
    );
    b.build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        )
}
fn style() -> ParagraphStyle {
    ParagraphStyle {
        root: root(),
        ..Default::default()
    }
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.02, "{a} != {b}");
}
fn text_position(line: usize, offset: u32, affinity: Affinity) -> TextPosition {
    TextPosition {
        line,
        offset,
        affinity,
    }
}

#[test]
fn logical_reading_order_and_actual_attributes() {
    let mut s = style();
    s.root.lang = Some("ar".into());
    s.root.paint.color = [30, 40, 50, 255];
    let ls = lines("a سلام b", s, 1000.0, true);
    let a = AccessibleLayout::new(&ls);
    assert_eq!(a.logical_text(), "a سلام b");
    let arabic = a.lines()[0]
        .runs
        .iter()
        .find(|r| r.bidi_level % 2 == 1)
        .unwrap();
    assert_eq!(arabic.node, Some(NodeId(42)));
    assert_eq!(arabic.style.lang.as_deref(), Some("ar"));
    assert_eq!(arabic.style.paint.color, [30, 40, 50, 255]);
    let actual = ls[0]
        .fragments()
        .find_map(|f| match f {
            shodo::Fragment::GlyphRun(r) if r.bidi_level() % 2 == 1 => {
                Some((r.font(), r.font_size()))
            }
            _ => None,
        })
        .unwrap();
    assert_eq!((arabic.font, arabic.font_size), (Some(actual.0), actual.1));
    let ch = &a.lines()[0].characters[2];
    assert!(ch.leading.0 > ch.trailing.0);
}

#[test]
fn ligatures_emoji_and_combining_use_selectable_characters() {
    let ls = lines("ffi👩‍💻a\u{0301}", style(), 1000.0, true);
    let a = AccessibleLayout::new(&ls);
    let cs = &a.lines()[0].characters;
    let mut cuts: Vec<_> = cs.iter().map(|c| c.text_range.start).collect();
    cuts.push(cs.last().unwrap().text_range.end);
    assert_eq!(cuts, [0, 1, 2, 3, 14, 17]);
    close(cs[0].rect.inline_size, 5.04);
    close(cs[1].rect.inline_size, 5.056);
    assert_eq!(cs[3].text, "👩‍💻");
    assert_eq!(cs[4].text, "a\u{0301}");
    let interior = a
        .from_text_position(text_position(0, 8, Affinity::Upstream))
        .unwrap();
    assert_eq!(a.to_text_position(interior).unwrap().offset, 3);
    let interior = a
        .from_text_position(text_position(0, 8, Affinity::Downstream))
        .unwrap();
    assert_eq!(a.to_text_position(interior).unwrap().offset, 14);
}

#[test]
fn source_positions_normalize_collapsed_and_expanded_text() {
    let mut s = style();
    s.root.text_transform = TextTransform::Uppercase;
    let ls = lines("  ß a", s, 1000.0, true);
    let a = AccessibleLayout::new(&ls);
    assert_eq!(a.logical_text(), "SS A");
    assert_eq!(
        a.lines()[0]
            .characters
            .iter()
            .map(|c| c.text)
            .collect::<Vec<_>>(),
        ["SS", " ", "A"]
    );
    for (affinity, want) in [(Affinity::Upstream, 0), (Affinity::Downstream, 2)] {
        let p = a.from_text_position(text_position(0, 1, affinity)).unwrap();
        assert_eq!(a.to_text_position(p).unwrap().offset, want);
    }
    let p = a.from_source(SourcePosition {
        origin: TextOrigin::Dom {
            node: NodeId(42),
            offset: 13,
        },
        affinity: Affinity::Downstream,
    });
    assert_eq!(p.len(), 1);
    assert_eq!(a.to_text_position(p[0]).unwrap().offset, 0);
    assert_eq!(
        a.to_source(p[0]).unwrap().origin,
        TextOrigin::Dom {
            node: NodeId(42),
            offset: 12
        }
    );
    assert_eq!(
        a.from_source(SourcePosition {
            origin: TextOrigin::Dom {
                node: NodeId(42),
                offset: 10
            },
            affinity: Affinity::Downstream
        })[0],
        p[0]
    );
}

#[test]
fn first_line_uses_its_own_utf8_dataset() {
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let mut first = s.root.clone();
    first.text_transform = TextTransform::Uppercase;
    s.first_line = Some(first);
    let ls = lines("ſ\nb", s, 1000.0, true);
    let a = AccessibleLayout::new(&ls);
    assert_eq!(a.logical_text(), "S\nb");
    assert_eq!(
        a.lines()
            .iter()
            .map(|l| l.text_range.clone())
            .collect::<Vec<_>>(),
        [0..2, 3..4]
    );
    let p = a.position(1, 0, Affinity::Downstream).unwrap();
    let source = a.to_source(p).unwrap();
    assert_eq!(
        source.origin,
        TextOrigin::Dom {
            node: NodeId(42),
            offset: 13
        }
    );
    assert!(a.from_source(source).contains(&p));
}

#[test]
fn nonpainting_breaks_tabs_controls_and_empty_lines() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let mut b = ParagraphBuilder::new(&s, &limits);
    let mut inline = s.root.clone();
    inline.unicode_bidi = UnicodeBidi::Isolate;
    b.open_inline(NodeId(3), &inline, InlineEdges::default())
        .push_text(
            TextSource::Dom {
                node: NodeId(42),
                offset: 0,
            },
            "a\n\nb\tc",
        )
        .close_inline();
    let ls = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
    let a = AccessibleLayout::new(&ls);
    assert_eq!(
        a.logical_text(),
        "\u{2066}a\u{2069}\n\u{2066}\u{2069}\n\u{2066}b\tc\u{2069}"
    );
    assert_eq!(a.lines().len(), 3);
    let cs: Vec<_> = a.lines().iter().flat_map(|l| &l.characters).collect();
    assert_eq!(
        cs.iter()
            .filter(|c| c.kind == AccessibleCharacterKind::HardBreak)
            .count(),
        2
    );
    assert_eq!(cs.iter().filter(|c| c.text.contains('\t')).count(), 1);
    assert_eq!(
        cs.iter().map(|c| c.text).collect::<String>(),
        a.logical_text()
    );
    for l in a.lines() {
        assert!(
            a.position(l.index, l.characters.len(), Affinity::Upstream)
                .is_some()
        );
    }
    let empty = AccessibleLayout::new(&[]);
    assert!(empty.position(0, 0, Affinity::Downstream).is_none());
    assert!(empty.hit_test(f32::NAN, 0.0).is_none());
}

#[test]
fn positions_and_selection_follow_final_bidi_geometry() {
    let ls = lines("ab سلام cd", style(), 1000.0, true);
    let a = AccessibleLayout::new(&ls);
    let anchor = a.position(0, 1, Affinity::Downstream).unwrap();
    let focus = a.position(0, 6, Affinity::Upstream).unwrap();
    assert_eq!(a.to_text_position(focus).unwrap().offset, 9);
    let rects = a.selection_rects(AccessibleSelection { anchor, focus });
    assert_eq!(rects.len(), 2);
    assert_eq!(
        a.selection_rects(AccessibleSelection {
            anchor: focus,
            focus: anchor
        }),
        rects
    );
    let hit = a.hit_test(1.0, ls[0].block_size() / 2.0).unwrap();
    assert_eq!(a.to_text_position(hit).unwrap().offset, 0);
    assert!(a.position(1, 0, Affinity::Downstream).is_none());
    assert!(a.position(0, 100, Affinity::Downstream).is_none());
}

#[test]
fn reflow_rejects_stale_positions_and_resolves_source_anchors() {
    let wide = lines("a b c", style(), 1000.0, true);
    let old = AccessibleLayout::new(&wide);
    let p = old.position(0, 2, Affinity::Downstream).unwrap();
    let source = old.to_source(p).unwrap();
    let narrow = lines("a b c", style(), 16.0, true);
    let new = AccessibleLayout::new(&narrow);
    assert_ne!(new.snapshot_id(), old.snapshot_id());
    assert!(new.to_text_position(p).is_none());
    assert!(new.to_source(p).is_none());
    assert!(
        new.selection_rects(AccessibleSelection {
            anchor: p,
            focus: p
        })
        .is_empty()
    );
    assert!(
        new.from_source(source)
            .iter()
            .any(|p| p.line == 1 && p.character == 0)
    );
}

#[test]
fn indivisible_sources_and_large_graphemes_remain_whole() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let s = style();
    let mut b = ParagraphBuilder::new(&s, &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(10),
            offset: 0,
        },
        "a",
    );
    let mut mark = root();
    mark.paint.color = [255, 0, 0, 255];
    b.open_inline(NodeId(9), &mark, InlineEdges::default())
        .push_text(
            TextSource::Dom {
                node: NodeId(11),
                offset: 0,
            },
            &"\u{0301}".repeat(200),
        )
        .close_inline();
    let ls = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
    let a = AccessibleLayout::new(&ls);
    assert_eq!(a.lines()[0].characters.len(), 1);
    assert_eq!(a.lines()[0].characters[0].text.len(), 401);
    assert_eq!(a.lines()[0].runs[0].node, Some(NodeId(10)));
    assert_eq!(a.lines()[0].runs[0].style.paint.color, [0, 0, 0, 255]);
    assert!(
        !a.from_source(SourcePosition {
            origin: TextOrigin::Dom {
                node: NodeId(11),
                offset: 2
            },
            affinity: Affinity::Downstream
        })
        .is_empty()
    );
    let no_map = lines("ab", style(), 1000.0, false);
    let no_map = AccessibleLayout::new(&no_map);
    assert!(
        no_map
            .to_source(no_map.position(0, 1, Affinity::Downstream).unwrap())
            .is_none()
    );
    assert!(
        no_map
            .from_source(SourcePosition {
                origin: TextOrigin::Dom {
                    node: NodeId(42),
                    offset: 11
                },
                affinity: Affinity::Downstream
            })
            .is_empty()
    );
}

#[test]
fn vertical_and_combined_characters_keep_physical_axes() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut s = style();
        s.writing_mode = mode;
        s.root.text_combine_upright = TextCombineUpright::All;
        let ls = lines("12", s, 1000.0, true);
        let a = AccessibleLayout::new(&ls);
        let c = &a.lines()[0].characters[0];
        assert_eq!(a.lines()[0].runs[0].orientation, GlyphOrientation::Combined);
        close(c.leading.0, c.trailing.0);
        assert!(
            (c.trailing.1 - c.leading.1)
                * if mode == WritingMode::VerticalRl {
                    -1.0
                } else {
                    1.0
                }
                > 0.0
        );
        let mut s = style();
        s.writing_mode = mode;
        let ls = lines("水", s, 1000.0, true);
        let a = AccessibleLayout::new(&ls);
        assert_eq!(a.lines()[0].runs[0].orientation, GlyphOrientation::Upright);
        assert!(a.lines()[0].characters[0].trailing.0 > a.lines()[0].characters[0].leading.0);
    }
}

#[test]
fn word_boundaries_cross_soft_wraps_and_attributes() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut s = style();
    s.root.overflow_wrap = OverflowWrap::Anywhere;
    let mut b = ParagraphBuilder::new(&s, &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "hel",
    );
    let mut inline = root();
    inline.paint.color = [255, 0, 0, 255];
    b.open_inline(NodeId(9), &inline, InlineEdges::default())
        .push_text(
            TextSource::Dom {
                node: NodeId(2),
                offset: 0,
            },
            "lo world",
        )
        .close_inline();
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    for width in [1000.0, 24.0] {
        let ls = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        );
        let a = AccessibleLayout::new(&ls);
        let mut n = 0;
        let mut starts = Vec::new();
        for l in a.lines() {
            starts.extend(l.word_starts.iter().map(|i| n + i));
            n += l.characters.len();
        }
        assert_eq!(starts, [0, 6]);
        assert_eq!(a.logical_text(), "hello world");
    }
}

#[test]
fn retained_lines_outlive_layout_owners() {
    let ls = lines("ab", style(), 1000.0, true);
    let a = AccessibleLayout::new(&ls);
    assert_eq!(a.logical_text(), "ab");
    let p = a.position(0, 1, Affinity::Downstream).unwrap();
    assert_eq!(
        a.to_source(p).unwrap().origin,
        TextOrigin::Dom {
            node: NodeId(42),
            offset: 11
        }
    );
    assert!(a.lines()[0].runs[0].font.is_some());
}
