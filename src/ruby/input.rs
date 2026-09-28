use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::builder::{ParagraphBuilder, RawItem};
use crate::limits::{LimitExceeded, Limits, Warning};
use crate::node::{NodeId, TextSource};
use crate::style::{InlineStyle, ParagraphStyle};

/// An immutable, unshaped snapshot of inline content. Clones share input.
#[derive(Clone, Debug)]
pub struct RubyContent(pub(crate) Arc<ContentInput>);

impl RubyContent {
    /// Consume a builder without shaping. Terminal errors and warnings survive.
    pub fn from_builder(builder: ParagraphBuilder) -> Self {
        Self(Arc::new(builder.into_ruby_content()))
    }

    /// Snapshot one text span with the supplied source and resolved style.
    pub fn text(source: TextSource, text: &str, style: &InlineStyle, limits: &Limits) -> Self {
        let mut builder = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style.clone(),
                ..Default::default()
            },
            limits,
        );
        builder.push_text(source, text);
        Self::from_builder(builder)
    }
}

#[derive(Clone, Debug)]
pub struct RubyBase {
    pub node: NodeId,
    pub content: RubyContent,
    pub align: RubyAlign,
}

#[derive(Clone, Debug)]
pub struct RubyAnnotation {
    pub node: NodeId,
    pub content: RubyContent,
    pub span: RubySpan,
    pub visibility: RubyVisibility,
}

#[derive(Clone, Debug)]
pub struct RubyLevel {
    pub annotations: Vec<RubyAnnotation>,
    pub style: RubyStyle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RubyAlign {
    Start,
    Center,
    SpaceBetween,
    #[default]
    SpaceAround,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RubyPosition {
    Over,
    Under,
    #[default]
    Alternate,
    AlternateUnder,
    InterCharacter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RubyOverhang {
    #[default]
    Auto,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RubyMerge {
    #[default]
    Separate,
    Merge,
    /// Currently uses the Separate policy.
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RubyStyle {
    pub align: RubyAlign,
    pub position: RubyPosition,
    pub overhang: RubyOverhang,
    pub merge: RubyMerge,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RubyVisibility {
    #[default]
    Visible,
    Hidden,
    Collapse,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RubySpan {
    #[default]
    Auto,
    All,
    Columns(Range<usize>),
}

/// Structurally validated ruby; normalization is deferred to a bounded parent.
#[derive(Clone, Debug)]
pub struct Ruby {
    pub(crate) bases: Vec<RubyBase>,
    pub(crate) levels: Vec<RubyLevel>,
    pub(crate) columns: usize,
}

/// A structural span error in one annotation level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RubyError {
    InvalidSpan {
        level: usize,
        annotation: usize,
        span: Range<usize>,
        columns: usize,
    },
    OverlappingSpans {
        level: usize,
        first: usize,
        second: usize,
    },
}

impl std::fmt::Display for RubyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid ruby pairing: {self:?}")
    }
}
impl std::error::Error for RubyError {}

impl Ruby {
    /// Validate nonempty, bounded, nonoverlapping spans without expanding columns.
    pub fn new(bases: Vec<RubyBase>, levels: Vec<RubyLevel>) -> Result<Self, RubyError> {
        let mut columns = bases.len();
        for level in &levels {
            for (i, annotation) in level.annotations.iter().enumerate() {
                match annotation.span {
                    RubySpan::Auto => columns = columns.max(i + 1),
                    RubySpan::All => columns = columns.max(1),
                    RubySpan::Columns(_) => {}
                }
            }
        }
        for (level_index, level) in levels.iter().enumerate() {
            let mut spans = Vec::with_capacity(level.annotations.len());
            for (i, annotation) in level.annotations.iter().enumerate() {
                let span = span(&annotation.span, i, columns);
                if span.start >= span.end || span.end > columns {
                    return Err(RubyError::InvalidSpan {
                        level: level_index,
                        annotation: i,
                        span,
                        columns,
                    });
                }
                spans.push((span, i));
            }
            spans.sort_unstable_by_key(|(span, _)| (span.start, span.end));
            for pair in spans.windows(2) {
                if pair[0].0.end > pair[1].0.start {
                    return Err(RubyError::OverlappingSpans {
                        level: level_index,
                        first: pair[0].1,
                        second: pair[1].1,
                    });
                }
            }
        }
        Ok(Self {
            bases,
            levels,
            columns,
        })
    }
}

pub(crate) fn span(span: &RubySpan, ordinal: usize, columns: usize) -> Range<usize> {
    match span {
        RubySpan::Auto => ordinal..ordinal + 1,
        RubySpan::All => 0..columns,
        RubySpan::Columns(range) => range.clone(),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ContentInput {
    pub(crate) style: ParagraphStyle,
    // Consumed by annotation preparation in Task 2.
    #[allow(dead_code)]
    pub(crate) limits: Limits,
    pub(crate) text: String,
    pub(crate) items: Vec<RawItem>,
    pub(crate) styles: Vec<InlineStyle>,
    pub(crate) first_line_styles: HashMap<u32, InlineStyle>,
    pub(crate) error: Option<LimitExceeded>,
    pub(crate) warnings: Vec<Warning>,
    #[allow(dead_code)]
    pub(crate) offset_mapping: bool,
    pub(crate) rubies: Vec<super::builder::RubyInput>,
    pub(crate) ruby_cost: super::builder::InputCost,
}
