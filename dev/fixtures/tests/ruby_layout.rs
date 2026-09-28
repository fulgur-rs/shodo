//! Ruby layout must shape real annotation text under the parent's budget.
use shodo::limits::{LimitKind, Limits};
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle, TextWrapStyle};
use shodo::{
    AtomicIntrinsics, AtomicSizes, LayoutContext, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel,
    RubyOverhang, RubySpan, RubyStyle, RubyVisibility,
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
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
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

fn short_pair() -> Paragraph {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_ruby(NodeId(8), &style(24.0), ruby(RubyVisibility::Visible));
    b.build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
}

#[test]
fn short_pair_overflows_whole() {
    let p = short_pair();
    // FontTools independently confirms1000-unit advances at1000 UPEM for
    // 日/に/ほ/ん: base24px, complete annotation3×12px=36px. A one-character
    // base has no interior parallel cut, including at a12px available width.
    for width in [1000.0, 12.0] {
        let mut cx = LayoutContext::new();
        let lines = p.break_all(&mut cx, &LineOptions::default(), width, &AtomicSizes::EMPTY);
        assert_eq!(lines.len(), 1, "one indivisible base/reading pair");
        assert!(
            (lines[0].inline_size() - 36.0).abs() < 0.001,
            "the actual reading reserves36px, width={width}, got{}",
            lines[0].inline_size()
        );
    }
}

#[test]
fn intrinsic_and_plans_measure_ruby() {
    let p = short_pair();
    let intrinsic = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        &AtomicIntrinsics::EMPTY,
    );
    assert_eq!(
        intrinsic.min_content, 36.0,
        "the pair cannot break internally"
    );
    assert_eq!(
        intrinsic.max_content, 36.0,
        "No overhang reserves the whole reading"
    );
    for wrap in [TextWrapStyle::Balance, TextWrapStyle::Pretty] {
        let options = LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let plan = p.plan_breaks(
            &mut LayoutContext::new(),
            &options,
            12.0,
            &AtomicSizes::EMPTY,
        );
        let mut constraint = LineConstraint::new(12.0);
        constraint.break_plan = Some(&plan);
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("an indivisible pair still makes progress under {wrap:?}");
        };
        assert_eq!(line.inline_size(), 36.0);
        assert!(line.is_last());
    }
}

#[path = "ruby/continuation.rs"]
mod continuation;
