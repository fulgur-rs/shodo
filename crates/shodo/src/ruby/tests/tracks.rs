use super::*;
use crate::font::{FontCollection, FontOptions};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::ruby::*;
use crate::style::{InlineStyle, ParagraphStyle};
use crate::{AtomicSizes, LayoutContext, Paragraph, ParagraphBuilder};

thread_local! {
    static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
pub(super) fn visit() {
    VISITS.with(|n| n.set(n.get() + 1));
}
fn unit(n: f32) -> LayoutUnit {
    LayoutUnit::from_f32_round(n, &mut Saturation::default())
}
fn build(levels: usize, empty_every: usize, mode: WritingMode) -> Paragraph {
    let limits = Limits::default();
    let style = InlineStyle::default();
    let content = |text| {
        RubyContent::text(
            TextSource::Generated { node: NodeId(1) },
            text,
            &style,
            &limits,
        )
    };
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(2),
            content: content("a"),
            align: RubyAlign::Start,
        }],
        (0..levels)
            .map(|i| RubyLevel {
                annotations: if empty_every > 0 && i % empty_every == 0 {
                    vec![]
                } else {
                    vec![RubyAnnotation {
                        node: NodeId(3 + i as u64),
                        content: content("b"),
                        span: RubySpan::All,
                        visibility: RubyVisibility::Hidden,
                    }]
                },
                style: RubyStyle {
                    position: RubyPosition::Alternate,
                    ..Default::default()
                },
            })
            .collect(),
    )
    .unwrap();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            writing_mode: mode,
            ..Default::default()
        },
        &limits,
    );
    builder.push_ruby(NodeId(0), &style, ruby);
    builder.build(&mut LayoutContext::new(), &fonts).unwrap()
}
fn measured_tracks(p: &Paragraph) -> (Tracks, usize) {
    let data = &p.data;
    let measure = crate::ruby::measure::candidate(
        data,
        0,
        data.units.len(),
        &AtomicSizes::EMPTY,
        &mut LayoutContext::new(),
        &mut Saturation::default(),
    );
    let f = &measure.fragments[0];
    let base = Bounds {
        top: unit(0.0),
        bottom: unit(20.0),
    };
    VISITS.with(|n| n.set(0));
    let result = tracks(
        data,
        &data.ruby.containers[f.container],
        f.column_start,
        &f.bases,
        &f.lanes,
        base,
        &vec![base; f.bases.len()],
        &f.right_columns,
        &vec![unit(2.0); f.lanes.len()],
        true,
        &mut Saturation::default(),
    );
    (result, VISITS.with(|n| n.get()))
}
#[test]
fn many_populated_and_empty_levels_have_linear_track_work() {
    for count in [64, 256] {
        for empty_every in [0, 3] {
            let p = build(count, empty_every, WritingMode::HorizontalTb);
            let (result, visits) = measured_tracks(&p);
            let lanes = p.data.ruby.containers[0].lanes.len();
            assert_eq!(result.lanes.len(), lanes);
            assert!(
                visits <= 3 * (count + lanes),
                "levels={count}, lanes={lanes}, visits={visits}"
            );
        }
    }
}
#[test]
fn empty_levels_still_alternate_and_hidden_lanes_keep_height() {
    // Empty levels 0 and 3 still consume alternating sides. Populated levels
    // 1,2,4,5 therefore go under,over,over,under (mirrored in vertical-lr).
    for (mode, expected) in [
        (WritingMode::HorizontalTb, [20.0, -2.0, -4.0, 22.0]),
        (WritingMode::VerticalRl, [20.0, -2.0, -4.0, 22.0]),
        (WritingMode::VerticalLr, [-2.0, 20.0, 22.0, -4.0]),
    ] {
        let (result, _) = measured_tracks(&build(6, 3, mode));
        assert_eq!(
            result
                .lanes
                .iter()
                .map(|l| l.block.to_f32())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(result.whole.top, unit(-4.0));
        assert_eq!(result.whole.bottom, unit(24.0));
    }
}
