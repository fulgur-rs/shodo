mod common;
use common::*;
use shodo::geometry::{Direction, LogicalRect, PhysicalConverter, PhysicalSize, WritingMode};
use shodo::node::{NodeId, TextSource};
use shodo::style::{LineOptions, TextAlign};
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult};

#[test]
fn physical_insets_produce_the_same_available_strip_in_every_flow() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let c = LineConstraint::from_physical_insets(100.0, 20.0, 10.0, mode, direction);
            let converter = PhysicalConverter::new(
                mode,
                direction,
                PhysicalSize {
                    width: 100.0,
                    height: 100.0,
                },
            );
            let r = converter.rect(LogicalRect {
                inline_start: c.inline_start_offset,
                inline_size: c.available_inline_size,
                ..Default::default()
            });
            let (start, size) = if mode.is_vertical() {
                (r.y, r.height)
            } else {
                (r.x, r.width)
            };
            assert_eq!((start, size), (20.0, 70.0), "{mode:?} {direction:?}");
        }
    }
}

#[test]
fn horizontal_physical_insets_match_manual_wrapping_and_alignment() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        let mut root = style();
        root.direction = direction;
        let p = build(&root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "אב גד הו זח");
        });
        let c = LineConstraint::from_physical_insets(
            100.0,
            20.0,
            10.0,
            WritingMode::HorizontalTb,
            direction,
        );
        let manual = LineConstraint {
            inline_start_offset: if direction == Direction::Ltr {
                20.0
            } else {
                10.0
            },
            ..LineConstraint::new(70.0)
        };
        for align in [
            TextAlign::Start,
            TextAlign::End,
            TextAlign::Center,
            TextAlign::Justify,
        ] {
            let options = LineOptions {
                text_align: align,
                ..Default::default()
            };
            let make_line = |constraint| {
                let LineResult::Line(line) = p.next_line(
                    &mut LayoutContext::new(),
                    p.start_token(),
                    &options,
                    constraint,
                    &AtomicSizes::EMPTY,
                ) else {
                    panic!("expected line")
                };
                line
            };
            let actual = make_line(&c);
            let expected = make_line(&manual);
            assert!(actual.text_range().end < p.text().len(), "line must wrap");
            assert_eq!(actual.text_range(), expected.text_range());
            assert_eq!(actual.inline_size(), expected.inline_size());
            assert_eq!(actual.hang_end(), expected.hang_end());
            assert_eq!(
                glyphs(&actual),
                glyphs(&expected),
                "{direction:?} {align:?}"
            );
        }
    }
}

#[test]
fn physical_insets_normalize_invalid_insets_and_overlapping_available_width() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        for (left, right, start, width) in [
            (-10.0, 15.0, 0.0, 85.0),
            (f32::NAN, 15.0, 0.0, 85.0),
            (10.0, -15.0, 10.0, 90.0),
            (10.0, f32::NAN, 10.0, 90.0),
        ] {
            let c = LineConstraint::from_physical_insets(
                100.0,
                left,
                right,
                WritingMode::HorizontalTb,
                direction,
            );
            let converter = PhysicalConverter::new(
                WritingMode::HorizontalTb,
                direction,
                PhysicalSize {
                    width: 100.0,
                    height: 100.0,
                },
            );
            let r = converter.rect(LogicalRect {
                inline_start: c.inline_start_offset,
                inline_size: c.available_inline_size,
                ..Default::default()
            });
            assert_eq!((r.x, r.width), (start, width));
        }
        let c = LineConstraint::from_physical_insets(
            100.0,
            80.0,
            50.0,
            WritingMode::HorizontalTb,
            direction,
        );
        assert_eq!(c.available_inline_size, 0.0);
        let p = paragraph("ab");
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &LineOptions::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("layout must progress at zero width")
        };
        assert_ne!(line.break_token(), p.start_token());
    }
}
