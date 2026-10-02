use super::*;
use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::limits::Limits;
use crate::mapping::MappingKind;
use crate::node::TextSource;
use crate::style::{FontFamily, ParagraphStyle, TextTransform, WhiteSpaceCollapse};
use crate::{AtomicSizes, LayoutContext, ParagraphBuilder};

fn fonts(limits: &Limits) -> FontCollection {
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
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
            font_size: 16.0,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn dom(node: u64, offset: u32) -> TextSource {
    TextSource::Dom {
        node: NodeId(node),
        offset,
    }
}
fn finish(builder: ParagraphBuilder, fonts: &FontCollection, width: f32) -> Vec<Line> {
    builder
        .build(&mut LayoutContext::new(), fonts)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        )
}

// Independent copy of the original linear source inverse, including dataset-wide
// preferred affinity, line-local caret normalization, stable ordering and dedup.
fn linear(layout: &AccessibleLayout<'_>, source: SourcePosition) -> Vec<AccessiblePosition> {
    let mut result = Vec::new();
    for (line, accepted) in layout.accepted.iter().enumerate() {
        let Some(mapping) = accepted.offset_mapping() else {
            continue;
        };
        match source.origin {
            TextOrigin::Dom { node, offset } => {
                let candidates = mapping.units().iter().filter_map(|u| {
                    if u.node != node || offset < u.dom.start || offset > u.dom.end {
                        return None;
                    }
                    let interior = u.dom.start < offset && offset < u.dom.end;
                    let (text, affinity) = if offset == u.dom.end {
                        (u.text.end, Affinity::Upstream)
                    } else {
                        match u.kind {
                            MappingKind::Identity => (
                                u.text.start.saturating_add(offset - u.dom.start),
                                if interior {
                                    source.affinity
                                } else {
                                    Affinity::Downstream
                                },
                            ),
                            MappingKind::Collapsed => (u.text.end, Affinity::Downstream),
                            MappingKind::Expanded => (u.text.start, Affinity::Downstream),
                        }
                    };
                    Some((text, affinity, interior))
                });
                let preferred = candidates
                    .clone()
                    .any(|(_, a, interior)| interior || a == source.affinity);
                let begin = result.len();
                for (offset, affinity, interior) in candidates {
                    if (interior || affinity == source.affinity || !preferred)
                        && let Some(p) = layout.from_text_position(TextPosition {
                            line,
                            offset,
                            affinity,
                        })
                    {
                        result.push(p);
                    }
                }
                result[begin..].sort_by_key(|p| (p.character, p.affinity == Affinity::Downstream));
            }
            TextOrigin::Generated { .. } => {
                for character in 0..=layout.lines[line].characters.len() {
                    if let Some(p) = layout.position(line, character, source.affinity)
                        && layout
                            .to_source(p)
                            .is_some_and(|s| s.origin == source.origin)
                        && result.last() != Some(&p)
                    {
                        result.push(p);
                    }
                }
            }
        }
    }
    result.dedup();
    result
}

#[test]
fn source_inverse_queries_shared_dataset_instead_of_each_line() {
    let limits = Limits::default();
    let fonts = fonts(&limits);
    let style = style();
    for count in [64, 256, 1024] {
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.with_offset_mapping(true);
        for node in 0..count {
            b.push_text(dom(node as u64, 0), "ab");
            if node + 1 < count {
                b.push_forced_break(NodeId(10000 + node as u64));
            }
        }
        let lines = finish(b, &fonts, 1000.0);
        assert_eq!(lines.len(), count);
        let layout = AccessibleLayout::new(&lines);
        let source = SourcePosition {
            origin: TextOrigin::Dom {
                node: NodeId(count as u64 - 1),
                offset: 1,
            },
            affinity: Affinity::Downstream,
        };
        assert_eq!(layout.from_source(source), linear(&layout, source));
        for _ in 0..2 {
            crate::mapping::tests::reset_visits();
            let result = layout.from_source(source);
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].line, count - 1);
            let visited = crate::mapping::tests::visits();
            eprintln!("source inverse: {count} nodes/lines, {visited} mapping index visits");
            assert!(
                visited > 0 && visited < 64,
                "{count} nodes/lines visited {visited} mapping records for one source candidate"
            );
        }
    }
}

#[test]
fn preferred_affinity_includes_candidates_outside_accepted_lines() {
    let limits = Limits::default();
    let fonts = fonts(&limits);
    let style = style();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.with_offset_mapping(true)
        .push_text(dom(7, 0), "a")
        .push_forced_break(NodeId(20))
        .push_text(TextSource::Generated { node: NodeId(8) }, "X")
        .push_forced_break(NodeId(21))
        .push_text(dom(7, 1), "b");
    let lines = finish(b, &fonts, 1000.0);
    assert_eq!(lines.len(), 3);
    let layout = AccessibleLayout::new(&lines[..1]);
    let source = |affinity| SourcePosition {
        origin: TextOrigin::Dom {
            node: NodeId(7),
            offset: 1,
        },
        affinity,
    };
    assert!(layout.from_source(source(Affinity::Downstream)).is_empty());
    assert_eq!(layout.from_source(source(Affinity::Upstream)).len(), 1);
}

#[test]
fn source_inverse_matches_linear_with_first_line_repetition_and_transforms() {
    let limits = Limits::default();
    let fonts = fonts(&limits);
    for first in [false, true] {
        for collapse in [false, true] {
            let mut style = style();
            if collapse {
                style.root.white_space_collapse = WhiteSpaceCollapse::Collapse;
            }
            if first {
                style.first_line = Some(InlineStyle {
                    text_transform: TextTransform::Uppercase,
                    ..style.root.clone()
                });
            }
            let mut b = ParagraphBuilder::new(&style, &limits);
            b.with_offset_mapping(true)
                .push_text(dom(7, 0), "ßa    ")
                .push_text(TextSource::Generated { node: NodeId(8) }, "X")
                .push_text(dom(7, 0), "ab")
                .push_forced_break(NodeId(10))
                .push_text(dom(7, 2), "c  d")
                .push_text(dom(9, 20), "ſb");
            let original = finish(b, &fonts, 24.0);
            assert!(original.len() > 1);
            if first {
                assert_eq!(original[0].data.id, original.last().unwrap().data.id);
                assert!(!std::ptr::eq(
                    original[0].offset_mapping().unwrap(),
                    original.last().unwrap().offset_mapping().unwrap()
                ));
            }
            let mut repeated = original.clone();
            repeated.push(original[0].clone());
            repeated.reverse();
            for lines in [&original, &repeated] {
                let layout = AccessibleLayout::new(lines);
                for affinity in [Affinity::Upstream, Affinity::Downstream] {
                    for origin in [7, 9, 99]
                        .into_iter()
                        .flat_map(|node| {
                            (0..25)
                                .chain([u32::MAX])
                                .map(move |offset| TextOrigin::Dom {
                                    node: NodeId(node),
                                    offset,
                                })
                        })
                        .chain([
                            TextOrigin::Generated { node: NodeId(8) },
                            TextOrigin::Generated { node: NodeId(99) },
                        ])
                    {
                        let source = SourcePosition { origin, affinity };
                        assert_eq!(
                            layout.from_source(source),
                            linear(&layout, source),
                            "first={first} collapse={collapse} source={source:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn saturated_and_empty_dom_ranges_match_linear_normalization() {
    let limits = Limits::default();
    let fonts = fonts(&limits);
    let style = style();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.with_offset_mapping(true)
        .push_text(dom(7, u32::MAX - 1), "ab")
        .push_text(dom(9, 0), "c")
        .push_text(dom(7, u32::MAX), "d");
    let lines = finish(b, &fonts, 1000.0);
    let layout = AccessibleLayout::new(&lines);
    for affinity in [Affinity::Upstream, Affinity::Downstream] {
        for offset in [u32::MAX - 2, u32::MAX - 1, u32::MAX] {
            let source = SourcePosition {
                origin: TextOrigin::Dom {
                    node: NodeId(7),
                    offset,
                },
                affinity,
            };
            assert_eq!(layout.from_source(source), linear(&layout, source));
        }
    }
}
