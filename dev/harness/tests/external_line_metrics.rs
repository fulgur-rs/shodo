use shodo::geometry::WritingMode;
use shodo::node::{NodeId, TextSource};
use shodo::style::LineHeight;
use shodo::{AtomicSizes, ExternalLineMetrics, LayoutContext, LineConstraint, LineResult};

#[test]
fn visible_reading_without_a_base_receives_external_metrics() {
    use shodo::limits::Limits;
    use shodo::ruby::{
        Ruby, RubyAnnotation, RubyContent, RubyLevel, RubySpan, RubyStyle, RubyVisibility,
    };
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    for (visibility, nested) in [
        (RubyVisibility::Visible, false),
        (RubyVisibility::Hidden, false),
        (RubyVisibility::Collapse, false),
        (RubyVisibility::Visible, true),
        (RubyVisibility::Hidden, true),
        (RubyVisibility::Collapse, true),
    ] {
        let root = shodo::style::ParagraphStyle::default();
        let mut content = RubyContent::text(
            TextSource::Generated { node: NodeId(3) },
            "b",
            &root.root,
            &limits,
        );
        if nested {
            let inner = Ruby::new(
                vec![],
                vec![RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(4),
                        content,
                        span: RubySpan::Auto,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle::default(),
                }],
            )
            .unwrap();
            let mut child = shodo::ParagraphBuilder::new(&root, &limits);
            child.push_ruby(NodeId(5), &root.root, inner);
            content = RubyContent::from_builder(child);
        }
        let ruby = Ruby::new(
            vec![],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(2),
                    content,
                    span: RubySpan::Auto,
                    visibility,
                }],
                // A nested ruby-only reading has no direct text; merge keeps
                // it from auto-hiding against the anonymous empty base.
                style: RubyStyle {
                    merge: shodo::ruby::RubyMerge::Merge,
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        let mut builder = shodo::ParagraphBuilder::new(&root, &limits);
        builder.push_ruby(NodeId(1), &root.root, ruby);
        let p = builder
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let mut c = LineConstraint::new(1000.);
        let LineResult::Line(normal) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("normal")
        };
        c.external_metrics = Some(ExternalLineMetrics {
            line_over: 300.,
            line_under: 200.,
        });
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("external")
        };
        if visibility == RubyVisibility::Visible {
            assert!(!line.is_empty());
            assert!(
                line.block_size() >= 500.,
                "visible reading: {}",
                line.block_size()
            );
            assert!(line.metrics().baseline >= 300.);
            c.max_block_size = Some(499.);
            assert!(matches!(
                p.next_line(
                    &mut LayoutContext::new(),
                    p.start_token(),
                    &Default::default(),
                    &c,
                    &AtomicSizes::EMPTY
                ),
                LineResult::BlockSizeExceeded { .. }
            ));
        } else {
            assert!(line.is_empty());
            assert_eq!(line.metrics(), normal.metrics());
            assert_eq!(line.block_size(), normal.block_size());
        }
    }
}

#[test]
fn ruby_and_emphasis_do_not_expose_external_occupancy_as_leading() {
    use shodo::limits::Limits;
    use shodo::ruby::{
        Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle,
        RubyVisibility,
    };
    use shodo::style::{TextEmphasis, TextEmphasisPosition, TextEmphasisShape};
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        let limits = Limits::default();
        let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
        let mut root = shodo::style::ParagraphStyle::default();
        root.root.font_size = 10.;
        root.root.line_height = LineHeight::Px(10.);
        root.root.font_families = vec![shodo::style::FontFamily::Named(
            shodo_fixtures::FONTS[0].family.into(),
        )];
        root.writing_mode = mode;
        root.root.text_emphasis = Some(TextEmphasis {
            shape: TextEmphasisShape::Dot,
            filled: true,
            position: TextEmphasisPosition::OverRight,
        });
        let mut reading = root.root.clone();
        reading.font_size = 100.;
        reading.line_height = LineHeight::Px(100.);
        reading.text_emphasis = None;
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(2),
                content: RubyContent::text(
                    TextSource::Generated { node: NodeId(3) },
                    "a",
                    &root.root,
                    &limits,
                ),
                align: RubyAlign::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(4),
                    content: RubyContent::text(
                        TextSource::Generated { node: NodeId(5) },
                        "b",
                        &reading,
                        &limits,
                    ),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            }],
        )
        .unwrap();
        let mut builder = shodo::ParagraphBuilder::new(&root, &limits);
        builder.push_ruby(NodeId(1), &root.root, ruby);
        let p = builder
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let mut c = LineConstraint::new(1000.);
        c.external_metrics = Some(ExternalLineMetrics {
            line_over: 30.,
            line_under: 20.,
        });
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("ruby line")
        };
        assert_eq!(
            line.ruby_annotations().count(),
            1,
            "{mode:?}: text={:?}, range={:?}, warnings={:?}, fragments={:?}",
            line.text(),
            line.text_range(),
            p.warnings(),
            line.fragments().collect::<Vec<_>>()
        );
        assert!(
            line.block_size() > 50.,
            "tall reading must exercise shifted ruby placement"
        );
        let annotation = line.annotation_metrics();
        assert_eq!(
            (annotation.space_over, annotation.space_under),
            (0., 0.),
            "{mode:?}"
        );
        assert_eq!(
            annotation.unannotated_block_end - annotation.unannotated_block_start,
            50.
        );
        assert_eq!(
            line.ruby_annotations().next().unwrap().line().block_size(),
            136.203_13
        );
    }
}
