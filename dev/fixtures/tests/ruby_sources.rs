//! Public real-font contracts for independent base/annotation source datasets.
use shodo::accessibility::AccessibleLayout;
use shodo::geometry::WritingMode;
use shodo::hit::{LineLayout, TextPosition};
use shodo::limits::Limits;
use shodo::mapping::{Affinity, TextOrigin};
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{
    AtomicSizes, Fragment, LayoutContext, Line, ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation,
    RubyBase, RubyContent, RubyLevel, RubyOverhang, RubySpan, RubyStyle, RubyVisibility,
};
use shodo_fixtures::load_fonts;

fn style(size: f32) -> InlineStyle {
    InlineStyle {
        font_size: size,
        font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
        ..Default::default()
    }
}
fn content(node: u64, offset: u32, text: &str, size: f32) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset,
        },
        text,
        &style(size),
        &Limits::default(),
    )
}
fn pair(visibility: RubyVisibility, overhang: RubyOverhang) -> Ruby {
    pair_with_reading(
        "日",
        content(20, 70, "にほん", 12.0),
        20,
        visibility,
        RubyStyle {
            overhang,
            ..Default::default()
        },
    )
}
fn pair_with_reading(
    base_text: &str,
    reading: RubyContent,
    node: u64,
    visibility: RubyVisibility,
    ruby_style: RubyStyle,
) -> Ruby {
    Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: content(10, 40, base_text, 24.0),
            align: RubyAlign::Center,
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(node),
                content: reading,
                span: RubySpan::All,
                visibility,
            }],
            style: ruby_style,
        }],
    )
    .unwrap()
}
fn layout(builder: ParagraphBuilder) -> Vec<Line> {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    let p = builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        1000.0,
        &AtomicSizes::EMPTY,
    )
}
fn builder(mode: WritingMode) -> ParagraphBuilder {
    ParagraphBuilder::new(
        &ParagraphStyle {
            writing_mode: mode,
            root: style(24.0),
            ..Default::default()
        },
        &Limits::default(),
    )
}
fn parent_point(a: shodo::RubyAnnotationView<'_>, x: f32, y: f32) -> (f32, f32) {
    let t = a.transform();
    (
        t.inline_inline * x + t.inline_block * y + t.inline_offset,
        t.block_inline * x + t.block_block * y + t.block_offset,
    )
}

#[test]
fn ruby_annotation_hit_routes_to_base_and_local_source() {
    let mut b = builder(WritingMode::HorizontalTb);
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 10,
        },
        "本",
    );
    b.push_ruby(
        NodeId(8),
        &style(24.0),
        pair(RubyVisibility::Visible, RubyOverhang::Auto),
    );
    b.push_text(
        TextSource::Dom {
            node: NodeId(2),
            offset: 90,
        },
        "語",
    );
    let lines = layout(b);
    assert_eq!(lines[0].inline_size(), 72.0);
    let a = lines[0].ruby_annotations().next().unwrap();
    assert_eq!(a.origin().0, 18.0);
    let run = a
        .line()
        .fragments()
        .find_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        run.glyphs().map(|g| g.id).collect::<Vec<_>>(),
        [208, 224, 248]
    );
    let (x, y) = parent_point(a, 2.0, run.baseline() - 6.0);
    let index = LineLayout::new(&lines);
    let local = index.hit_test_ruby(x, y).unwrap();
    assert_eq!(local.annotation.node(), Some(NodeId(20)));
    assert_eq!(
        local.hit.origin,
        Some(TextOrigin::Dom {
            node: NodeId(20),
            offset: 70
        })
    );
    assert_eq!(local.hit.position.line, 0);
    assert!(local.hit.inside);
    let main = index.hit_test(x, y).unwrap();
    assert_eq!(
        main.origin,
        Some(TextOrigin::Dom {
            node: NodeId(10),
            offset: 40
        }),
        "the overhang point lies above a neighboring base source, but belongs to this pair"
    );
    assert!(main.inside);
    let (tail_x, tail_y) = parent_point(a, 34.0, run.baseline() - 6.0);
    let tail = index.hit_test(tail_x, tail_y).unwrap();
    assert_eq!(
        tail.origin,
        Some(TextOrigin::Dom {
            node: NodeId(10),
            offset: 43
        }),
        "trailing overhang routes to paired base end rather than the next plain glyph"
    );
    assert!(tail.inside);
    let child_index = LineLayout::new(std::slice::from_ref(a.line()));
    let range = a.text_range();
    let rects = child_index.selection_rects(
        TextPosition {
            line: 0,
            offset: range.start as u32,
            affinity: Affinity::Downstream,
        },
        TextPosition {
            line: 0,
            offset: range.end as u32,
            affinity: Affinity::Upstream,
        },
    );
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0].inline_size, 36.0);
}

#[test]
fn accessible_ruby_relationships_keep_annotation_offsets_out_of_main_text() {
    let mut b = builder(WritingMode::HorizontalTb);
    b.push_ruby(
        NodeId(8),
        &style(24.0),
        pair(RubyVisibility::Visible, RubyOverhang::None),
    );
    let lines = layout(b);
    let accessible = AccessibleLayout::new(&lines);
    assert!(accessible.logical_text().contains('日'));
    assert!(!accessible.logical_text().contains('に'));
    let relationships: Vec<_> = accessible.ruby_annotations().collect();
    assert_eq!(relationships.len(), 1);
    assert_eq!(relationships[0].parent_line, 0);
    let a = relationships[0].annotation;
    assert_eq!(a.container(), NodeId(8));
    assert_eq!(a.base_nodes(), [NodeId(10)]);
    assert_eq!(a.node(), Some(NodeId(20)));
    let child = AccessibleLayout::new(std::slice::from_ref(a.line()));
    assert!(child.logical_text().contains("にほん"));
    let text = a.line().text().find('に').unwrap() as u32;
    let p = child
        .from_text_position(TextPosition {
            line: 0,
            offset: text,
            affinity: Affinity::Downstream,
        })
        .unwrap();
    assert_eq!(
        child.to_source(p).unwrap().origin,
        TextOrigin::Dom {
            node: NodeId(20),
            offset: 70
        }
    );
}

#[test]
fn ruby_hit_inverts_all_positions_vertical_modes_directions_and_parent_offsets() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        for direction in [
            shodo::geometry::Direction::Ltr,
            shodo::geometry::Direction::Rtl,
        ] {
            for position in [
                shodo::RubyPosition::Over,
                shodo::RubyPosition::Under,
                shodo::RubyPosition::InterCharacter,
            ] {
                let mut b = ParagraphBuilder::new(
                    &ParagraphStyle {
                        writing_mode: mode,
                        direction,
                        root: style(24.0),
                        ..Default::default()
                    },
                    &Limits::default(),
                );
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(1),
                        offset: 10,
                    },
                    "本",
                )
                .push_forced_break(NodeId(2));
                let ruby = pair_with_reading(
                    "日",
                    content(20, 70, "にほん", 12.0),
                    20,
                    RubyVisibility::Visible,
                    RubyStyle {
                        position,
                        overhang: RubyOverhang::None,
                        ..Default::default()
                    },
                );
                b.push_ruby(NodeId(8), &style(24.0), ruby);
                let lines = layout(b);
                assert_eq!(lines.len(), 2);
                let a = lines[1].ruby_annotations().next().unwrap();
                let run = a
                    .line()
                    .fragments()
                    .find_map(|f| {
                        if let Fragment::GlyphRun(r) = f {
                            Some(r)
                        } else {
                            None
                        }
                    })
                    .unwrap();
                let (x, y) = parent_point(a, 2.0, run.baseline());
                let y = y + lines[1].block_offset();
                let index = LineLayout::new(&lines);
                let hit = index
                    .hit_test_ruby(x, y)
                    .unwrap_or_else(|| panic!("{mode:?}/{direction:?}/{position:?}"));
                assert_eq!(hit.parent_line(), 1);
                assert_eq!(hit.path().len(), 1);
                assert_eq!(
                    hit.hit.origin,
                    Some(TextOrigin::Dom {
                        node: NodeId(20),
                        offset: 70
                    })
                );
                let main = index.hit_test(x, y).unwrap();
                assert!(main.inside);
                assert_eq!(main.position.line, 1);
                assert!(matches!(
                    main.origin,
                    Some(TextOrigin::Dom {
                        node: NodeId(10),
                        ..
                    })
                ));
            }
        }
    }
}

#[test]
fn hidden_collapsed_and_empty_readings_have_no_annotation_hit() {
    for (visibility, empty) in [
        (RubyVisibility::Hidden, false),
        (RubyVisibility::Collapse, false),
        (RubyVisibility::Visible, true),
    ] {
        let ruby = pair_with_reading(
            "日",
            content(20, 70, if empty { "" } else { "にほん" }, 12.0),
            20,
            visibility,
            RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        );
        let mut b = builder(WritingMode::HorizontalTb);
        b.push_ruby(NodeId(8), &style(24.0), ruby);
        let lines = layout(b);
        let index = LineLayout::new(&lines);
        for x in [0.0, 6.0, 12.0, 24.0, 36.0] {
            for y in [0.0, 6.0, 12.0, 24.0] {
                assert!(
                    index.hit_test_ruby(x, y).is_none(),
                    "{visibility:?}/empty={empty}"
                );
            }
        }
        if visibility == RubyVisibility::Hidden {
            assert_eq!(
                AccessibleLayout::new(&lines)
                    .ruby_annotations()
                    .next()
                    .unwrap()
                    .annotation
                    .visibility(),
                visibility
            );
        }
    }
}

#[test]
fn nested_ruby_hit_keeps_the_composed_path_and_deepest_local_source() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        let mut child = builder(mode);
        child.push_ruby(
            NodeId(81),
            &style(24.0),
            pair_with_reading(
                "本",
                content(20, 70, "にほん", 12.0),
                20,
                RubyVisibility::Visible,
                RubyStyle {
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            ),
        );
        let outer = pair_with_reading(
            "日",
            RubyContent::from_builder(child),
            82,
            RubyVisibility::Visible,
            RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        );
        let mut b = builder(mode);
        b.push_ruby(NodeId(8), &style(24.0), outer);
        let lines = layout(b);
        let outside = lines[0].ruby_annotations().next().unwrap();
        let inside = outside.line().ruby_annotations().next().unwrap();
        let run = inside
            .line()
            .fragments()
            .find_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap();
        let p = parent_point(inside, 2.0, run.baseline());
        let p = parent_point(outside, p.0, p.1);
        let index = LineLayout::new(&lines);
        let hit = index.hit_test_ruby(p.0, p.1).unwrap();
        assert_eq!(
            hit.path().iter().map(|a| a.container()).collect::<Vec<_>>(),
            [NodeId(8), NodeId(81)]
        );
        assert_eq!(hit.annotation.container(), NodeId(81));
        assert_eq!(
            hit.hit.origin,
            Some(TextOrigin::Dom {
                node: NodeId(20),
                offset: 70
            })
        );
        let main = index.hit_test(p.0, p.1).unwrap();
        assert_eq!(main.position.line, 0);
        assert!(matches!(
            main.origin,
            Some(TextOrigin::Dom {
                node: NodeId(10),
                ..
            })
        ));
    }
}

#[test]
fn ruby_dom_split_selection_preserves_each_anchor_in_one_actual_ligature() {
    let latin = InlineStyle {
        font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
        ..style(24.0)
    };
    let mut base = ParagraphBuilder::new(
        &ParagraphStyle {
            root: latin.clone(),
            ..Default::default()
        },
        &Limits::default(),
    );
    base.push_text(
        TextSource::Dom {
            node: NodeId(10),
            offset: 40,
        },
        "f",
    );
    base.push_text(
        TextSource::Dom {
            node: NodeId(11),
            offset: 80,
        },
        "fi",
    );
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(99),
            content: RubyContent::from_builder(base),
            align: RubyAlign::Start,
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: content(20, 70, "にほん", 12.0),
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: latin.clone(),
            ..Default::default()
        },
        &Limits::default(),
    );
    b.push_ruby(NodeId(8), &latin, ruby);
    let lines = layout(b);
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
    assert_eq!(glyphs.len(), 1);
    assert_ne!(glyphs[0].id, 0);
    let map = lines[0].offset_mapping().unwrap();
    let index = LineLayout::new(&lines);
    for (node, offset) in [
        (NodeId(10), 40),
        (NodeId(10), 41),
        (NodeId(11), 80),
        (NodeId(11), 81),
        (NodeId(11), 82),
    ] {
        let (offset_text, affinity) = map.dom_to_text(node, offset).unwrap();
        let caret = index
            .caret(TextPosition {
                line: 0,
                offset: offset_text,
                affinity,
            })
            .unwrap();
        assert_eq!(
            map.text_to_dom(caret.position.offset, caret.position.affinity),
            Some(TextOrigin::Dom { node, offset })
        );
    }
    let (start, sa) = map.dom_to_text(NodeId(10), 40).unwrap();
    let (end, ea) = map.dom_to_text(NodeId(11), 82).unwrap();
    let rects = index.selection_rects(
        TextPosition {
            line: 0,
            offset: start,
            affinity: sa,
        },
        TextPosition {
            line: 0,
            offset: end,
            affinity: ea,
        },
    );
    assert_eq!(rects.len(), 1);
    let natural: f32 = lines[0]
        .fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .map(|r| r.clusters().map(|c| c.shaping_advance).sum::<f32>())
        .sum();
    assert!((rects[0].inline_size - natural).abs() < 0.02);
}

fn painted_text(line: &Line) -> String {
    line.fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(&line.text()[r.text_range()]),
            _ => None,
        })
        .collect()
}

#[test]
fn first_line_annotation_sources_keep_expansions_and_normal_continuation_separate() {
    use shodo::style::{LineBreak, TextTransform};
    let latin = InlineStyle {
        font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
        line_break: LineBreak::Anywhere,
        ..style(24.0)
    };
    let input = |node, offset, text, size| {
        let normal = InlineStyle {
            font_size: size,
            ..latin.clone()
        };
        let first = InlineStyle {
            font_size: 24.0,
            text_transform: TextTransform::Uppercase,
            ..normal.clone()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: normal,
                first_line: Some(first),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(
            TextSource::Dom {
                node: NodeId(node),
                offset,
            },
            text,
        );
        RubyContent::from_builder(b)
    };
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: input(10, 40, "ßa", 24.0),
            align: RubyAlign::Center,
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: input(20, 70, "ßb", 12.0),
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: latin.clone(),
            ..Default::default()
        },
        &Limits::default(),
    );
    b.push_ruby(NodeId(8), &latin, ruby);
    let fonts = load_fonts(&Limits::default()).unwrap();
    let paragraph = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let lines = paragraph.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        30.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        lines.len(),
        2,
        "{:?}",
        lines
            .iter()
            .map(|l| (
                painted_text(l),
                l.inline_size(),
                l.ruby_annotations()
                    .map(|a| (painted_text(a.line()), a.line().inline_size()))
                    .collect::<Vec<_>>()
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(painted_text(&lines[0]), "SS");
    assert_eq!(painted_text(&lines[1]), "a");
    let main = AccessibleLayout::new(&lines);
    for (i, expected, offset, size) in [(0, "SS", 70, 24.0), (1, "b", 72, 12.0)] {
        let a = lines[i].ruby_annotations().next().unwrap();
        assert_eq!(painted_text(a.line()), expected);
        let run = a
            .line()
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(run.font_size(), size);
        assert!(run.glyphs().all(|g| g.id != 0));
        let child = AccessibleLayout::new(std::slice::from_ref(a.line()));
        let p = child
            .from_text_position(TextPosition {
                line: 0,
                offset: run.text_range().start as u32,
                affinity: Affinity::Downstream,
            })
            .unwrap();
        assert_eq!(
            child.to_source(p).unwrap().origin,
            TextOrigin::Dom {
                node: NodeId(20),
                offset
            }
        );
        let map = a.line().offset_mapping().unwrap();
        if i == 0 {
            assert_eq!(
                map.text_to_dom(run.text_range().start as u32 + 1, Affinity::Downstream),
                Some(TextOrigin::Dom {
                    node: NodeId(20),
                    offset: 70
                })
            );
            for (affinity, want) in [(Affinity::Upstream, 70), (Affinity::Downstream, 72)] {
                let snapped = child
                    .from_text_position(TextPosition {
                        line: 0,
                        offset: run.text_range().start as u32 + 1,
                        affinity,
                    })
                    .unwrap();
                assert_eq!(
                    child.to_source(snapped).unwrap().origin,
                    TextOrigin::Dom {
                        node: NodeId(20),
                        offset: want
                    }
                );
            }
        }
        let (x, y) = parent_point(
            a,
            run.glyphs().next().unwrap().inline_position + 0.1,
            run.baseline(),
        );
        let hit = LineLayout::new(&lines)
            .hit_test_ruby(x, y + lines[i].block_offset())
            .unwrap();
        assert_eq!(hit.parent_line(), i);
        assert_eq!(
            hit.hit.origin,
            Some(TextOrigin::Dom {
                node: NodeId(20),
                offset
            })
        );
        assert!(
            !main.logical_text().contains(expected) || expected == "SS",
            "reading remains separate from normal base"
        );
    }
}

#[test]
fn ruby_bidi_reading_order_keeps_logical_sources_under_visual_reversal() {
    use shodo::geometry::Direction;
    use shodo::style::UnicodeBidi;
    let rtl = InlineStyle {
        direction: Direction::Rtl,
        unicode_bidi: UnicodeBidi::BidiOverride,
        ..style(24.0)
    };
    let mut bases = Vec::new();
    let mut annotations = Vec::new();
    for (node, text, reading) in [(10, "日", "に"), (11, "本", "ほん")] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                direction: Direction::Rtl,
                root: rtl.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(
            TextSource::Dom {
                node: NodeId(node),
                offset: 40,
            },
            text,
        );
        bases.push(RubyBase {
            node: NodeId(node),
            content: RubyContent::from_builder(b),
            align: RubyAlign::Center,
        });
        annotations.push(RubyAnnotation {
            node: NodeId(node + 10),
            content: content(node + 10, 70, reading, 12.0),
            span: RubySpan::Auto,
            visibility: RubyVisibility::Visible,
        });
    }
    let ruby = Ruby::new(
        bases,
        vec![RubyLevel {
            annotations,
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let mut b = builder(WritingMode::HorizontalTb);
    b.push_ruby(NodeId(8), &rtl, ruby);
    let lines = layout(b);
    let glyphs: Vec<_> = lines[0]
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some((r.node(), r.inline_start())),
            _ => None,
        })
        .collect();
    assert_eq!(glyphs, [(Some(NodeId(11)), 0.0), (Some(NodeId(10)), 24.0)]);
    let accessible = AccessibleLayout::new(&lines);
    let main: String = accessible.lines()[0]
        .characters
        .iter()
        .filter_map(|c| (c.text == "日" || c.text == "本").then_some(c.text))
        .collect();
    assert_eq!(main, "日本");
    let index = LineLayout::new(&lines);
    for a in lines[0].ruby_annotations() {
        let run = a
            .line()
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .unwrap();
        let (x, y) = parent_point(a, run.inline_size() / 2.0, run.baseline());
        let hit = index.hit_test(x, y).unwrap();
        assert!(
            matches!(hit.origin, Some(TextOrigin::Dom {node, ..}) if a.base_nodes().contains(&node)),
            "base={:?} range={:?}, point=({x},{y}), hit={hit:?}",
            a.base_nodes(),
            a.base_text_range()
        );
        let reading = index.hit_test_ruby(x, y).unwrap();
        assert!(
            matches!(reading.hit.origin, Some(TextOrigin::Dom {node, ..}) if Some(node) == a.node())
        );
    }
}

#[cfg(feature = "accesskit")]
#[test]
fn accesskit_ruby_content_is_related_separately_from_main_character_positions() {
    use shodo::accessibility::AccessibleSelection;
    use shodo::accessibility::accesskit::{AccessKitAdapter, NodeSemantics, types as ak};
    use shodo::geometry::PhysicalRect;

    let mut b = builder(WritingMode::HorizontalTb);
    b.push_ruby(
        NodeId(8),
        &style(24.0),
        pair(RubyVisibility::Visible, RubyOverhang::None),
    );
    b.push_text(
        TextSource::Dom {
            node: NodeId(30),
            offset: 100,
        },
        "語",
    );
    let lines = layout(b);
    let accessible = AccessibleLayout::new(&lines);
    let character = accessible.lines()[0]
        .characters
        .iter()
        .position(|c| c.text == "日")
        .unwrap();
    let selection = AccessibleSelection {
        anchor: accessible
            .position(0, character, Affinity::Downstream)
            .unwrap(),
        focus: accessible
            .position(0, character + 1, Affinity::Upstream)
            .unwrap(),
    };
    let frame = PhysicalRect {
        x: 50.0,
        y: 30.0,
        width: 1000.0,
        height: 500.0,
    };
    let mut adapter = AccessKitAdapter::new(ak::NodeId(1));
    let mut counter = 10;
    let update = adapter
        .update(
            &accessible,
            ak::Node::new(ak::Role::Document),
            frame,
            Some(selection),
            |_| NodeSemantics::default(),
            || {
                counter += 1;
                ak::NodeId(counter)
            },
        )
        .unwrap();
    let reading = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == ak::Role::RubyAnnotation)
        .expect("retained ruby reading must be exposed separately");
    assert!(reading.1.value().unwrap().contains("にほん"));
    assert!(
        reading.1.character_lengths().is_empty(),
        "reading must not become main TextRun characters"
    );
    let reading_id = reading.0;
    let base = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == ak::Role::TextRun && n.value().unwrap().contains('日'))
        .unwrap();
    assert!(
        base.1.details().contains(&reading_id),
        "paired base owns the reading relationship"
    );
    assert_ne!(reading_id, base.0);
    assert!(
        adapter
            .from_position(
                ak::TextPosition {
                    node: reading_id,
                    character_index: 0
                },
                Affinity::Downstream
            )
            .is_none()
    );
    let at = adapter.to_position(selection.anchor).unwrap();
    assert_eq!(
        adapter.from_position(at, Affinity::Downstream),
        Some(selection.anchor)
    );
    assert_eq!(
        accessible.to_source(selection.anchor).unwrap().origin,
        TextOrigin::Dom {
            node: NodeId(10),
            offset: 40
        }
    );
    let ids: Vec<_> = update.nodes.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        ids.len()
    );
    let tree = accesskit_consumer::Tree::new(update, true);
    assert_eq!(
        tree.state().root().document_range().text(),
        accessible.logical_text()
    );
    assert_eq!(tree.state().root().text_selection().unwrap().text(), "日");
    assert!(
        !tree
            .state()
            .root()
            .document_range()
            .text()
            .contains("にほん")
    );
    let before = counter;
    let next = adapter
        .update(
            &accessible,
            ak::Node::new(ak::Role::Document),
            frame,
            None,
            |_| NodeSemantics::default(),
            || {
                counter += 1;
                ak::NodeId(counter)
            },
        )
        .unwrap();
    assert_eq!(
        counter, before,
        "same source identities must reuse allocated ownership IDs"
    );
    assert_eq!(
        next.nodes
            .iter()
            .find(|(_, n)| n.role() == ak::Role::RubyAnnotation)
            .unwrap()
            .0,
        reading_id
    );
}

#[cfg(feature = "accesskit")]
#[test]
fn accesskit_nested_ruby_keeps_ownership_visibility_and_composed_bounds() {
    use shodo::accessibility::accesskit::{AccessKitAdapter, NodeSemantics, types as ak};
    use shodo::geometry::{PhysicalConverter, PhysicalRect, PhysicalSize};
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        for visibility in [RubyVisibility::Visible, RubyVisibility::Hidden] {
            // The base and both annotation datasets deliberately share the same
            // DOM anchor. They must still own distinct SDK nodes.
            let mut child = builder(mode);
            child.push_ruby(
                NodeId(81),
                &style(24.0),
                pair_with_reading(
                    "本",
                    content(10, 40, "にほん", 12.0),
                    10,
                    RubyVisibility::Visible,
                    RubyStyle {
                        overhang: RubyOverhang::None,
                        ..Default::default()
                    },
                ),
            );
            let outer = pair_with_reading(
                "日",
                RubyContent::from_builder(child),
                10,
                visibility,
                RubyStyle {
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            );
            let mut b = builder(mode);
            b.push_text(
                TextSource::Dom {
                    node: NodeId(30),
                    offset: 100,
                },
                "語",
            )
            .push_forced_break(NodeId(31));
            b.push_ruby(NodeId(8), &style(24.0), outer);
            let lines = layout(b);
            let outside = lines[1].ruby_annotations().next().unwrap();
            let inside = outside.line().ruby_annotations().next().unwrap();
            let a = AccessibleLayout::new(&lines);
            let frame = PhysicalRect {
                x: 50.0,
                y: 30.0,
                width: 1000.0,
                height: 500.0,
            };
            let mut counter = 10;
            let mut adapter = AccessKitAdapter::new(ak::NodeId(1));
            let update = adapter
                .update(
                    &a,
                    ak::Node::new(ak::Role::Document),
                    frame,
                    None,
                    |_| NodeSemantics::default(),
                    || {
                        counter += 1;
                        ak::NodeId(counter)
                    },
                )
                .unwrap();
            let readings: Vec<_> = update
                .nodes
                .iter()
                .filter(|(_, n)| n.role() == ak::Role::RubyAnnotation)
                .collect();
            assert_eq!(readings.len(), 2);
            let outer = readings
                .iter()
                .find(|(_, n)| n.value().unwrap().contains('本'))
                .unwrap();
            let inner = readings
                .iter()
                .find(|(_, n)| n.value().unwrap().contains("にほん"))
                .unwrap();
            assert_eq!(outer.1.children(), [inner.0]);
            assert_ne!(outer.0, inner.0);
            assert_eq!(outer.1.is_hidden(), visibility == RubyVisibility::Hidden);
            assert_eq!(inner.1.is_hidden(), visibility == RubyVisibility::Hidden);
            for reading in &readings {
                assert!(
                    adapter
                        .from_position(
                            ak::TextPosition {
                                node: reading.0,
                                character_index: 0
                            },
                            Affinity::Downstream
                        )
                        .is_none()
                );
            }
            let rect = inside.line().overflow_rect();
            let point = parent_point(
                inside,
                rect.inline_start + rect.inline_size / 2.0,
                rect.block_start + rect.block_size / 2.0,
            );
            let point = parent_point(outside, point.0, point.1);
            let converter = PhysicalConverter::new(
                mode,
                lines[1].used_direction(),
                PhysicalSize {
                    width: frame.width,
                    height: frame.height,
                },
            );
            let point = converter.point(point.0, point.1 + lines[1].block_offset());
            let rect = inner.1.bounds().unwrap();
            assert!(
                rect.contains(ak::Point::new(
                    f64::from(frame.x + point.0),
                    f64::from(frame.y + point.1)
                )),
                "{mode:?}/{visibility:?}: {rect:?}, point={point:?}"
            );
            let tree = accesskit_consumer::Tree::new(update, true);
            assert_eq!(
                tree.state().root().document_range().text(),
                a.logical_text().replace('\u{2028}', "\n")
            );
            assert!(!tree.state().root().document_range().text().contains('本'));
        }
    }
}
