//! Prepared lanes must retain real fonts and their own source projection.
use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::limits::Limits;
use crate::mapping::TextOrigin;
use crate::node::{NodeId, TextSource};
use crate::ruby::*;
use crate::style::{FontFamily, InlineStyle, ParagraphStyle};
use crate::{LayoutContext, ParagraphBuilder};

const CJK: &[u8] = include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf");

fn style(size: f32) -> InlineStyle {
    InlineStyle {
        font_size: size,
        font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
        ..Default::default()
    }
}

#[test]
fn annotation_retains_real_fonts() {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let face = fonts
        .register_face(
            CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: RubyContent::text(
                TextSource::Dom {
                    node: NodeId(10),
                    offset: 40,
                },
                "日",
                &style(24.0),
                &Limits::default(),
            ),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: RubyContent::text(
                    TextSource::Dom {
                        node: NodeId(20),
                        offset: 70,
                    },
                    "にほん",
                    &style(12.0),
                    &Limits::default(),
                ),
                span: RubySpan::Auto,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    builder.push_ruby(NodeId(8), &style(24.0), ruby);
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    drop(fonts);
    let container = &paragraph.data.ruby.containers[0];
    assert_eq!(container.node, NodeId(8));
    let lane = &container.lanes[0];
    assert_eq!(lane.node, Some(NodeId(20)));
    let child = &lane.paragraph;
    // Literal IDs independently extracted from the pinned fixture's cmap with
    // FontTools before implementation: に=208, ほ=224, ん=248.
    assert_eq!(child.data.glyphs.id, [208, 224, 248]);
    assert!(!child.data.runs.is_empty());
    for run in &child.data.runs {
        assert_eq!(run.font, face);
        assert_eq!(run.font_size, 12.0);
    }
    assert_eq!(child.data.fonts.font_data(face).unwrap().data.as_ref(), CJK);
    let mapping = child.offset_mapping().unwrap();
    for offset in [70, 73, 76, 79] {
        let (at, affinity) = mapping.dom_to_text(NodeId(20), offset).unwrap();
        assert_eq!(
            mapping.text_to_dom(at, affinity),
            Some(TextOrigin::Dom {
                node: NodeId(20),
                offset
            })
        );
    }
    assert!(
        paragraph
            .offset_mapping()
            .unwrap()
            .dom_to_text(NodeId(20), 70)
            .is_none()
    );
}

#[test]
fn annotation_only_first_line_change_prepares_both_sets() {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut reading = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(12.0),
            first_line: Some(style(18.0)),
            ..Default::default()
        },
        &Limits::default(),
    );
    reading.push_text(
        TextSource::Dom {
            node: NodeId(20),
            offset: 0,
        },
        "にほん",
    );
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: RubyContent::text(
                TextSource::Dom {
                    node: NodeId(10),
                    offset: 0,
                },
                "日",
                &style(24.0),
                &Limits::default(),
            ),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: RubyContent::from_builder(reading),
                span: RubySpan::Auto,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    // Two independently retained parent sets and two reading sets. Each parent
    // has15 bytes/13 items/3 styles; each reading15/5/2. Pairing metadata and
    // two two-cell cuts add8 items plus2 interval-index cells per parent set. Glyphs are2+6=8.
    for (kind, total) in [
        (crate::limits::LimitKind::TextBytes, 60),
        (crate::limits::LimitKind::Items, 56),
        (crate::limits::LimitKind::Styles, 10),
        (crate::limits::LimitKind::ShapedGlyphs, 8),
    ] {
        for cap in [total - 1, total] {
            let mut limits = Limits::default();
            match kind {
                crate::limits::LimitKind::TextBytes => limits.max_text_bytes = Some(cap),
                crate::limits::LimitKind::Items => limits.max_items = Some(cap),
                crate::limits::LimitKind::Styles => limits.max_styles = Some(cap),
                crate::limits::LimitKind::ShapedGlyphs => limits.max_shaped_glyphs = Some(cap),
                _ => unreachable!(),
            }
            let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
            builder.push_ruby(NodeId(8), &style(24.0), ruby.clone());
            let result = builder.build(&mut LayoutContext::new(), &fonts);
            if cap < total {
                let error = result.expect_err("both sets share the aggregate budget");
                assert_eq!((error.kind, error.limit), (kind, cap));
            } else {
                let p = result.unwrap();
                let normal = &p.data.ruby.containers[0].lanes[0].paragraph;
                assert!(normal.data.runs.iter().all(|run| run.font_size == 12.0));
                let first = p
                    .data
                    .first_line
                    .as_ref()
                    .expect("annotation changes activate first-line geometry");
                let alternate = &first.data.ruby.containers[0].lanes[0].paragraph;
                assert!(alternate.data.runs.iter().all(|run| run.font_size == 18.0));
                assert_eq!(normal.data.glyphs.len(), 3);
                assert_eq!(alternate.data.glyphs.len(), 3);
            }
        }
    }
}

#[test]
fn annotation_forced_breaks_preserve_noncollapsible_spaces() {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for (collapse, expected) in [
        (crate::style::WhiteSpaceCollapse::Preserve, "に     ほん"),
        (crate::style::WhiteSpaceCollapse::Collapse, "にほん"),
    ] {
        let mut reading = ParagraphBuilder::new(
            &ParagraphStyle {
                root: InlineStyle {
                    white_space_collapse: collapse,
                    ..style(12.0)
                },
                ..Default::default()
            },
            &Limits::default(),
        );
        reading
            .push_text(
                TextSource::Dom {
                    node: NodeId(20),
                    offset: 70,
                },
                "に  ",
            )
            .push_forced_break(NodeId(21))
            .push_text(
                TextSource::Dom {
                    node: NodeId(22),
                    offset: 100,
                },
                "  ほん",
            );
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: RubyContent::text(
                    TextSource::Dom {
                        node: NodeId(10),
                        offset: 0,
                    },
                    "日",
                    &style(24.0),
                    &Limits::default(),
                ),
                align: RubyAlign::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(20),
                    content: RubyContent::from_builder(reading),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            }],
        )
        .unwrap();
        let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        builder.push_ruby(NodeId(8), &style(24.0), ruby);
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        let child = &p.data.ruby.containers[0].lanes[0].paragraph;
        let logical: String = child
            .text()
            .chars()
            .filter(|ch| !matches!(*ch, '\u{2066}'..='\u{2069}' | '\u{202a}'..='\u{202e}'))
            .collect();
        assert_eq!(logical, expected);
        assert!(
            !child
                .data
                .units
                .iter()
                .any(|u| matches!(u.kind, crate::analysis::units::UnitKind::ForcedBreak))
        );
        assert!(
            child
                .offset_mapping()
                .unwrap()
                .dom_to_text(NodeId(22), 108)
                .is_some()
        );
    }
}
