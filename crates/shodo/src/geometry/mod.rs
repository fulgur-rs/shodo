//! Geometry: fixed-point units, writing modes, and logical/physical conversion.
//!
//! The [vertical output guide] shows how to compose glyph transforms, baselines
//! and physical conversion for vertical, sideways and combined text.
//!
//! [vertical output guide]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/vertical-layout.md

mod unit;

pub(crate) use unit::{LayoutUnit, Saturation};

/// CSS `writing-mode` (CSS Writing Modes 4 §3.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WritingMode {
    #[default]
    HorizontalTb,
    VerticalRl,
    VerticalLr,
    SidewaysRl,
    SidewaysLr,
}

impl WritingMode {
    pub fn is_vertical(self) -> bool {
        !matches!(self, Self::HorizontalTb)
    }

    /// `VerticalRl` or `VerticalLr`, excluding the sideways modes.
    ///
    /// Only these modes honor `text-orientation`; unlike [`Self::is_vertical`],
    /// `SidewaysRl` and `SidewaysLr` lay every glyph out sideways.
    pub(crate) fn is_typographic_vertical(self) -> bool {
        matches!(self, Self::VerticalRl | Self::VerticalLr)
    }

    /// Whether line-over lies toward logical block-end. Sideways-lr's
    /// line-over is physically left, so it remains toward block-start.
    pub(crate) fn line_over_is_block_end(self) -> bool {
        matches!(self, Self::VerticalLr)
    }

    /// Map line-over/under values to logical block-start/end. The same
    /// swap maps back; sideways-lr keeps line-over at block-start.
    pub(crate) fn over_under_to_block<T>(self, (over, under): (T, T)) -> (T, T) {
        if self.line_over_is_block_end() {
            (under, over)
        } else {
            (over, under)
        }
    }

    /// Convert the distance from line-over to a block-start baseline. Keep
    /// the subtraction conditional: even an unused conversion can saturate.
    pub(crate) fn baseline_from_over(
        self,
        over: LayoutUnit,
        block_size: LayoutUnit,
        sat: &mut Saturation,
    ) -> LayoutUnit {
        if self.line_over_is_block_end() {
            block_size.sub(over, sat)
        } else {
            over
        }
    }

    /// Logical block displacement toward line-over.
    pub(crate) fn line_over_sign(self) -> f32 {
        if self.line_over_is_block_end() {
            1.0
        } else {
            -1.0
        }
    }

    /// Whether text in `orientation` uses the vertical (upright) em box: a
    /// typographic vertical mode with any `text-orientation` but `sideways`.
    pub(crate) fn is_upright(self, orientation: crate::style::TextOrientation) -> bool {
        self.is_typographic_vertical() && orientation != crate::style::TextOrientation::Sideways
    }
}

/// CSS `direction`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

/// Baseline types used for alignment (CSS Inline 3 §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BaselineKind {
    Alphabetic,
    Central,
    Ideographic,
    Hanging,
}

/// A rectangle in logical coordinates (inline / block axes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LogicalRect {
    pub inline_start: f32,
    pub block_start: f32,
    pub inline_size: f32,
    pub block_size: f32,
}

/// A rectangle in physical coordinates (x grows right, y grows down).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicalRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicalSize {
    pub width: f32,
    pub height: f32,
}

/// Converts logical rectangles inside a container to physical ones
/// (CSS Writing Modes 4 §6).
#[derive(Clone, Copy, Debug)]
pub struct PhysicalConverter {
    writing_mode: WritingMode,
    direction: Direction,
    container: PhysicalSize,
}

impl PhysicalConverter {
    pub fn new(writing_mode: WritingMode, direction: Direction, container: PhysicalSize) -> Self {
        Self {
            writing_mode,
            direction,
            container,
        }
    }

    /// Map a logical point; unlike `rect`, no glyph or box extent is known.
    pub fn point(&self, inline: f32, block: f32) -> (f32, f32) {
        let ltr = self.direction == Direction::Ltr;
        match self.writing_mode {
            WritingMode::HorizontalTb => (
                if ltr {
                    inline
                } else {
                    self.container.width - inline
                },
                block,
            ),
            WritingMode::VerticalRl | WritingMode::SidewaysRl => (
                self.container.width - block,
                if ltr {
                    inline
                } else {
                    self.container.height - inline
                },
            ),
            WritingMode::VerticalLr => (
                block,
                if ltr {
                    inline
                } else {
                    self.container.height - inline
                },
            ),
            WritingMode::SidewaysLr => (
                block,
                if ltr {
                    self.container.height - inline
                } else {
                    inline
                },
            ),
        }
    }

    /// Map a displacement, without applying the container's translated origin.
    pub fn vector(&self, inline: f32, block: f32) -> (f32, f32) {
        let ltr = self.direction == Direction::Ltr;
        match self.writing_mode {
            WritingMode::HorizontalTb => (if ltr { inline } else { -inline }, block),
            WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                (-block, if ltr { inline } else { -inline })
            }
            WritingMode::VerticalLr => (block, if ltr { inline } else { -inline }),
            WritingMode::SidewaysLr => (block, if ltr { -inline } else { inline }),
        }
    }

    /// Inverse of `point`, used for caret/hit coordinates from a painter.
    pub fn logical_point(&self, x: f32, y: f32) -> (f32, f32) {
        let origin = self.point(0.0, 0.0);
        let dx = x - origin.0;
        let dy = y - origin.1;
        let ltr = self.direction == Direction::Ltr;
        match self.writing_mode {
            WritingMode::HorizontalTb => (if ltr { dx } else { -dx }, dy),
            WritingMode::VerticalRl | WritingMode::SidewaysRl => (if ltr { dy } else { -dy }, -dx),
            WritingMode::VerticalLr => (if ltr { dy } else { -dy }, dx),
            WritingMode::SidewaysLr => (if ltr { -dy } else { dy }, dx),
        }
    }

    pub fn rect(&self, r: LogicalRect) -> PhysicalRect {
        let PhysicalSize {
            width: w,
            height: h,
        } = self.container;
        let ltr = self.direction == Direction::Ltr;
        match self.writing_mode {
            WritingMode::HorizontalTb => PhysicalRect {
                x: if ltr {
                    r.inline_start
                } else {
                    w - r.inline_start - r.inline_size
                },
                y: r.block_start,
                width: r.inline_size,
                height: r.block_size,
            },
            WritingMode::VerticalRl | WritingMode::SidewaysRl => PhysicalRect {
                x: w - r.block_start - r.block_size,
                y: if ltr {
                    r.inline_start
                } else {
                    h - r.inline_start - r.inline_size
                },
                width: r.block_size,
                height: r.inline_size,
            },
            WritingMode::VerticalLr => PhysicalRect {
                x: r.block_start,
                y: if ltr {
                    r.inline_start
                } else {
                    h - r.inline_start - r.inline_size
                },
                width: r.block_size,
                height: r.inline_size,
            },
            WritingMode::SidewaysLr => PhysicalRect {
                x: r.block_start,
                y: if ltr {
                    h - r.inline_start - r.inline_size
                } else {
                    r.inline_start
                },
                width: r.block_size,
                height: r.inline_size,
            },
        }
    }
}

#[cfg(test)]
mod writing_mode_tests {
    use super::*;

    #[test]
    fn baseline_conversion_preserves_conditional_saturation() {
        for (mode, raw, saturated) in [
            (WritingMode::HorizontalTb, i32::MIN, 0),
            (WritingMode::VerticalRl, i32::MIN, 0),
            (WritingMode::VerticalLr, i32::MAX, 1),
            (WritingMode::SidewaysRl, i32::MIN, 0),
            (WritingMode::SidewaysLr, i32::MIN, 0),
        ] {
            let mut sat = Saturation::default();
            assert_eq!(
                mode.baseline_from_over(LayoutUnit::MIN, LayoutUnit::ZERO, &mut sat)
                    .raw(),
                raw
            );
            assert_eq!(sat.saturated, saturated, "{mode:?}");
        }
    }

    #[test]
    fn over_under_mapping_distinguishes_vertical_lr_from_sideways_lr() {
        for (mode, expected) in [
            (WritingMode::HorizontalTb, (3, 7)),
            (WritingMode::VerticalRl, (3, 7)),
            (WritingMode::VerticalLr, (7, 3)),
            (WritingMode::SidewaysRl, (3, 7)),
            (WritingMode::SidewaysLr, (3, 7)),
        ] {
            assert_eq!(mode.over_under_to_block((3, 7)), expected, "{mode:?}");
        }
    }
}
