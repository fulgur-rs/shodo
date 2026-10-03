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
