use super::*;
use crate::geometry::WritingMode;
use crate::limits::{Limits, WarningSink};
use crate::node::{NodeId, TextSource};
use crate::style::{
    FontFamily, LineHeight, LineOptions, ParagraphStyle, TextCombineUpright, TextEmphasis,
    TextEmphasisPosition, TextEmphasisShape,
};
use crate::{
    AtomicSizes, LayoutContext, Line, LineConstraint, LineResult, ParagraphBuilder, Ruby,
    RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyVisibility,
};

fn accepted(mode: WritingMode, marked: bool, grouped: bool, combined: bool, ruby: bool) -> Line {
    let limits = Limits::unlimited();
    let fonts = crate::test_support::fonts::collection(&limits);
    fonts
        .register(crate::test_support::fonts::LATIN.to_vec())
        .unwrap();
    fonts
        .register(crate::test_support::fonts::CJK.to_vec())
        .unwrap();
    let root = InlineStyle {
        font_families: vec![
            FontFamily::Named("Shodo Fixture Latin".into()),
            FontFamily::Named("Shodo Fixture CJK".into()),
        ],
        font_size: 16.0,
        line_height: LineHeight::Px(18.0),
        text_emphasis: marked.then_some(TextEmphasis {
            shape: TextEmphasisShape::Dot,
            filled: true,
            position: TextEmphasisPosition::OverRight,
        }),
        ..Default::default()
    };
    let style = ParagraphStyle {
        root,
        writing_mode: mode,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    for (n, size, align) in [
        (0, 31.0, VerticalAlign::Top),
        (1, 11.0, VerticalAlign::Bottom),
        (2, 22.0, VerticalAlign::Length(2.125)),
    ] {
        let child = InlineStyle {
            font_size: size,
            line_height: LineHeight::Px(size + 3.0),
            vertical_align: if grouped {
                align
            } else {
                VerticalAlign::Length(2.125)
            },
            text_combine_upright: if combined {
                TextCombineUpright::All
            } else {
                TextCombineUpright::None
            },
            ..style.root.clone()
        };
        b.open_inline(NodeId(10 + n), &child, Default::default());
        b.push_text(
            TextSource::Generated {
                node: NodeId(20 + n),
            },
            "12",
        );
        b.close_inline();
    }
    if ruby {
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(30),
                align: Default::default(),
                content: RubyContent::text(
                    TextSource::Generated { node: NodeId(31) },
                    "日",
                    &style.root,
                    &limits,
                ),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(32),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                    content: RubyContent::text(
                        TextSource::Generated { node: NodeId(33) },
                        "日日",
                        &InlineStyle {
                            font_size: 9.0,
                            ..style.root.clone()
                        },
                        &limits,
                    ),
                }],
                style: Default::default(),
            }],
        )
        .unwrap();
        b.push_ruby(NodeId(34), &style.root, ruby);
    }
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    line
}

fn compare(line: &Line) -> Saturation {
    let range = line.units.start as usize..line.units.end as usize;
    let mut full_sat = Saturation {
        saturated: 3,
        non_finite: 2,
    };
    let mut extents_sat = full_sat;
    // The full-output specialization is the retained reference: it performs
    // every combination/record conversion and produces the discarded arrays.
    let full = measure_profile::<true>(
        &line.data,
        range.clone(),
        &line.fragments,
        &line.overlay_runs,
        true,
        &mut full_sat,
    );
    let bare = measure_unannotated(
        &line.data,
        range,
        &line.fragments,
        &line.overlay_runs,
        &mut extents_sat,
    );
    assert_eq!(
        (bare.baseline, bare.block_size),
        (full.baseline, full.block_size)
    );
    assert_eq!(
        extents_sat, full_sat,
        "discarding shifts must still convert every value"
    );
    for cap in [None, Some(0), Some(1)] {
        let mut full_warnings = WarningSink::new(cap);
        let mut extents_warnings = WarningSink::new(cap);
        full_warnings.record_saturation(&full_sat);
        extents_warnings.record_saturation(&extents_sat);
        assert_eq!(full_warnings.take(), extents_warnings.take());
    }
    full_sat
}

#[test]
fn auxiliary_extents_match_full_output_across_emphasis_groups_ruby_and_combination() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        for marked in [false, true] {
            for grouped in [false, true] {
                for combined in [false, true] {
                    for ruby in [false, true] {
                        let line = accepted(mode, marked, grouped, combined, ruby);
                        assert_eq!(line.block_shifts.len(), line.fragments.len());
                        assert_eq!(line.ruby_annotations().count() > 0, ruby);
                        if combined && mode != WritingMode::HorizontalTb {
                            assert!(!line.data.combine_spans.is_empty());
                        }
                        assert_eq!(
                            compare(&line),
                            Saturation {
                                saturated: 3,
                                non_finite: 2
                            }
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn discarded_shift_conversions_preserve_overflow_nonfinite_counts_and_warning_order() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        for combined in [false, true] {
            for value in [1.0e9, -1.0e9, f32::MAX, -f32::MAX, f32::NAN, f32::INFINITY] {
                let mut line = accepted(mode, true, true, combined, false);
                // Exercise the numeric solver directly, including values
                // normally sanitized by the public builder.
                let data = std::sync::Arc::get_mut(&mut line.data).unwrap();
                for style in &mut data.styles[1..] {
                    style.vertical_align = VerticalAlign::Length(value);
                }
                let sat = compare(&line);
                assert!(
                    sat.saturated > 3 || sat.non_finite > 2,
                    "fixture must trigger lossy conversions"
                );
            }
        }
    }
}
