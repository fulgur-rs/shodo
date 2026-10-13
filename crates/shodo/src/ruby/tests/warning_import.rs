use crate::ParagraphBuilder;
use crate::limits::{
    BORROWED_WARNING_CLONES, BORROWED_WARNING_VISITS, LimitKind, Limits, WarningKind,
};
use crate::node::{NodeId, TextSource};
use crate::ruby::*;
use crate::style::{InlineStyle, ParagraphStyle};

fn reset() {
    BORROWED_WARNING_VISITS.with(|n| n.set(0));
    BORROWED_WARNING_CLONES.with(|n| n.set(0));
}

fn content(cap: Option<u64>, count: usize, text: &str) -> RubyContent {
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle::default(),
        &Limits {
            max_warnings: cap,
            ..Default::default()
        },
    );
    for _ in 0..count {
        b.close_inline();
    }
    b.push_text(TextSource::Generated { node: NodeId(1) }, text);
    RubyContent::from_builder(b)
}

fn bases(content: &RubyContent, count: usize) -> Ruby {
    Ruby::new(
        vec![
            RubyBase {
                node: NodeId(2),
                content: content.clone(),
                align: RubyAlign::default()
            };
            count
        ],
        vec![],
    )
    .unwrap()
}

#[test]
fn shared_base_aliases_obey_parent_warning_cap_in_normal_and_first_line_paths() {
    let content = content(Some(1024), 1025, "");
    assert_eq!(content.0.warnings.len(), 1025);
    for cap in [Some(0), Some(3), None] {
        for first_line in [false, true] {
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle::default(),
                &Limits {
                    max_warnings: cap,
                    ..Default::default()
                },
            );
            let ruby = bases(&content, 16);
            reset();
            if first_line {
                b.push_ruby_with_first_line(
                    NodeId(3),
                    &InlineStyle::default(),
                    &InlineStyle::default(),
                    ruby,
                );
            } else {
                b.push_ruby(NodeId(3), &InlineStyle::default(), ruby);
            }
            assert!(b.error().is_none());
            let copies = cap.map_or(16 * 1025, |n| n as usize);
            assert_eq!(
                BORROWED_WARNING_CLONES.with(|n| n.get()),
                copies,
                "{cap:?}/{first_line}"
            );
            assert_eq!(
                BORROWED_WARNING_VISITS.with(|n| n.get()),
                cap.map_or(copies, |n| n as usize + 1)
            );
            let warnings = b.warnings.as_slice();
            if cap.is_some() {
                assert_eq!(warnings.len(), copies + 1);
                assert_eq!(warnings.last().unwrap().kind, WarningKind::Suppressed);
                assert_eq!(
                    warnings.last().unwrap().message,
                    "further warnings suppressed"
                );
            } else {
                assert_eq!(warnings.len(), 16 * content.0.warnings.len());
                for chunk in warnings.chunks(content.0.warnings.len()) {
                    assert_eq!(chunk, content.0.warnings);
                }
                assert!(!b.warnings.is_suppressed());
            }
            assert!(
                warnings[..copies]
                    .iter()
                    .all(|w| cap.is_none() || w.kind == WarningKind::UnbalancedInline)
            );
        }
    }
}

#[test]
fn nested_shared_snapshots_stop_at_parent_cap_and_resume_after_take() {
    let leaf = content(Some(1024), 1025, "");
    let mut inner = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
    inner.push_ruby(NodeId(3), &InlineStyle::default(), bases(&leaf, 1));
    let nested = RubyContent::from_builder(inner);
    assert_eq!(nested.0.warnings, leaf.0.warnings);
    let mut parent = ParagraphBuilder::new(
        &ParagraphStyle::default(),
        &Limits {
            max_warnings: Some(3),
            ..Default::default()
        },
    );
    for _ in 0..2 {
        reset();
        parent.push_ruby(NodeId(4), &InlineStyle::default(), bases(&nested, 2));
        assert!(parent.error().is_none());
        assert_eq!(BORROWED_WARNING_CLONES.with(|n| n.get()), 3);
        assert_eq!(BORROWED_WARNING_VISITS.with(|n| n.get()), 4);
        let warnings = parent.warnings.take();
        assert_eq!(warnings.len(), 4);
        assert_eq!(&warnings[..3], &leaf.0.warnings[..3]);
        assert_eq!(warnings[3].kind, WarningKind::Suppressed);
    }
}

#[test]
fn restored_annotations_keep_their_own_caps_and_warning_records() {
    for cap in [Some(0), Some(2), None] {
        let source = content(cap, 6, "");
        reset();
        let restored = ParagraphBuilder::from_ruby_content(&source.0, NodeId(5)).unwrap();
        assert_eq!(restored.warnings.as_slice(), source.0.warnings);
        assert_eq!(
            BORROWED_WARNING_CLONES.with(|n| n.get()),
            cap.map_or(6, |n| n as usize)
        );
        assert_eq!(
            BORROWED_WARNING_VISITS.with(|n| n.get()),
            source.0.warnings.len()
        );
    }
}

#[test]
fn hidden_annotation_warnings_survive_parent_suppression_while_collapse_skips_restore() {
    let source = content(Some(2), 3, "b");
    let base = content(Some(1024), 0, "a");
    let limits = Limits {
        max_warnings: Some(0),
        ..Default::default()
    };
    let fonts = crate::font::FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for visibility in [
        RubyVisibility::Visible,
        RubyVisibility::Hidden,
        RubyVisibility::Collapse,
    ] {
        for first_line in [false, true] {
            let style = ParagraphStyle {
                first_line: first_line.then_some(InlineStyle {
                    font_size: 20.0,
                    ..Default::default()
                }),
                ..Default::default()
            };
            let mut b = ParagraphBuilder::new(&style, &limits);
            b.close_inline();
            let ruby = Ruby::new(
                vec![RubyBase {
                    node: NodeId(6),
                    content: base.clone(),
                    align: RubyAlign::default(),
                }],
                vec![RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(7),
                        content: source.clone(),
                        span: RubySpan::All,
                        visibility,
                    }],
                    style: RubyStyle::default(),
                }],
            )
            .unwrap();
            b.push_ruby(NodeId(8), &InlineStyle::default(), ruby);
            reset();
            let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
            assert_eq!(p.warnings().len(), 1);
            assert_eq!(p.warnings()[0].kind, WarningKind::Suppressed);
            let lanes = &p.data.ruby.containers[0].lanes;
            if visibility == RubyVisibility::Collapse {
                assert!(lanes.is_empty());
                assert_eq!(BORROWED_WARNING_CLONES.with(|n| n.get()), 0);
            } else {
                assert_eq!(lanes.len(), 1);
                assert_eq!(lanes[0].paragraph.warnings(), source.0.warnings);
                assert_eq!(BORROWED_WARNING_CLONES.with(|n| n.get()), 2);
            }
        }
    }
}

#[test]
fn failed_snapshot_restoration_returns_terminal_error_before_warning_import() {
    let limits = Limits {
        max_text_bytes: Some(0),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.close_inline()
        .push_text(TextSource::Generated { node: NodeId(1) }, "failed");
    let source = RubyContent::from_builder(b);
    reset();
    let Err(error) = ParagraphBuilder::from_ruby_content(&source.0, NodeId(9)) else {
        panic!("failed input must stay failed")
    };
    assert_eq!(error, source.0.error.unwrap());
    assert_eq!(error.kind, LimitKind::TextBytes);
    assert_eq!(BORROWED_WARNING_CLONES.with(|n| n.get()), 0);
    assert_eq!(BORROWED_WARNING_VISITS.with(|n| n.get()), 0);
}
