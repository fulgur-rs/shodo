use crate::font::{FontCollection, FontOptions};
use crate::limits::Limits;
use crate::node::NodeId;
use crate::style::{LineOptions, ParagraphStyle};
use crate::{AtomicSizes, LayoutContext, ParagraphBuilder};

#[test]
fn trial_driver_matches_completed_text_lines() {
    let limits = Limits::default();
    let style = ParagraphStyle::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register(crate::test_support::fonts::LATIN.to_vec())
        .unwrap();
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Generated { node: NodeId(1) },
        "Alpha beta gamma delta epsilon zeta eta theta.",
    );
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    for width in [0., 16., 63.125, 240., 4096.] {
        let options = LineOptions::default();
        let mut full_cx = LayoutContext::new();
        let expected: Vec<_> = p
            .break_all(&mut full_cx, &options, width, &AtomicSizes::EMPTY)
            .into_iter()
            .map(|l| (l.break_token(), l.block_size().to_bits()))
            .collect();
        let mut trial_cx = LayoutContext::new();
        let actual: Vec<_> = p
            .fixed_width_trials(&mut trial_cx, &options, width, &AtomicSizes::EMPTY)
            .map(|l| (l.token, l.block_size.to_bits()))
            .collect();
        assert_eq!(actual, expected, "width={width}");
        assert_eq!(
            trial_cx.take_warnings(),
            full_cx.take_warnings(),
            "width={width}"
        );
    }
}

#[test]
fn trial_driver_preserves_first_line_float_and_block_progress() {
    for first_line in [false, true] {
        let limits = Limits {
            max_warnings: None,
            ..Default::default()
        };
        let mut style = ParagraphStyle::default();
        if first_line {
            style.first_line = Some(crate::style::InlineStyle {
                font_size: 32.,
                text_transform: crate::style::TextTransform::Uppercase,
                ..style.root.clone()
            });
        }
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register(crate::test_support::fonts::LATIN.to_vec())
            .unwrap();
        let mut builder = ParagraphBuilder::new(&style, &limits);
        let text = |id| crate::node::TextSource::Generated { node: NodeId(id) };
        builder
            .push_text(text(1), "Straße ffi alpha beta ")
            .push_out_of_flow(NodeId(2), crate::node::OutOfFlowKind::Float)
            .push_text(text(3), "gamma delta ")
            .push_block_in_inline(NodeId(4))
            .push_text(text(5), "epsilon ")
            .push_forced_break(NodeId(6))
            .push_text(text(7), "zeta eta theta.");
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        for width in [0., 32., 80., 240., f32::MAX] {
            let options = LineOptions::default();
            let mut full = LayoutContext::new();
            let expected: Vec<_> = p
                .break_all(&mut full, &options, width, &AtomicSizes::EMPTY)
                .into_iter()
                .map(|l| (l.break_token(), l.block_size().to_bits()))
                .collect();
            let mut trial = LayoutContext::new();
            let actual: Vec<_> = p
                .fixed_width_trials(&mut trial, &options, width, &AtomicSizes::EMPTY)
                .map(|l| (l.token, l.block_size.to_bits()))
                .collect();
            assert_eq!(actual, expected, "first_line={first_line}, width={width}");
            assert_eq!(
                trial.take_warnings(),
                full.take_warnings(),
                "first_line={first_line}, width={width}"
            );
        }
    }
}
