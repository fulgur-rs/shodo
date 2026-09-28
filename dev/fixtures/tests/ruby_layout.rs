//! Ruby layout must shape real annotation text under the parent's budget.
use shodo::limits::{LimitKind, Limits};
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{
    LayoutContext, ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent,
    RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
use shodo_fixtures::load_fonts;

fn style(size: f32) -> InlineStyle {
    InlineStyle {
        font_size: size,
        font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
        ..Default::default()
    }
}

fn ruby(visibility: RubyVisibility) -> Ruby {
    Ruby::new(
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
                visibility,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap()
}

#[test]
fn annotation_glyphs_share_parent_limit() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    // Literal corpus: one base glyph plus three annotation glyphs. Hidden
    // annotations still reserve geometry and therefore still need shaping.
    for visibility in [RubyVisibility::Visible, RubyVisibility::Hidden] {
        for cap in [2, 4] {
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle::default(),
                &Limits {
                    max_shaped_glyphs: Some(cap),
                    ..Default::default()
                },
            );
            b.push_ruby(NodeId(8), &style(24.0), ruby(visibility));
            let result = b.build(&mut LayoutContext::new(), &fonts.collection);
            if cap == 2 {
                let error = result.expect_err("annotation glyphs must be charged to the parent");
                assert_eq!(error.kind, LimitKind::ShapedGlyphs);
                assert_eq!(error.limit, 2);
                assert!(error.actual > 2);
            } else {
                assert!(
                    result.is_ok(),
                    "the exact four-glyph cap must fit: {result:?}"
                );
            }
        }
    }
}

#[test]
fn shared_ruby_input_is_charged_for_each_prepared_container() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let shared = ruby(RubyVisibility::Visible);
    for cap in [7, 8] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle::default(),
            &Limits {
                max_shaped_glyphs: Some(cap),
                ..Default::default()
            },
        );
        b.push_ruby(NodeId(8), &style(24.0), shared.clone())
            .push_ruby(NodeId(9), &style(24.0), shared.clone());
        let result = b.build(&mut LayoutContext::new(), &fonts.collection);
        if cap == 7 {
            let error = result
                .expect_err("shared raw snapshots must not bypass per-container shaped storage");
            assert_eq!(error.kind, LimitKind::ShapedGlyphs);
            assert_eq!(error.limit, 7);
            assert!(error.actual > 7);
        } else {
            assert!(
                result.is_ok(),
                "eight retained glyphs fit exactly: {result:?}"
            );
        }
    }
}
