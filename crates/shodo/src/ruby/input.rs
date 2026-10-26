use crate::hashing::FastMap;
use std::ops::Range;
use std::sync::Arc;

use crate::builder::{ParagraphBuilder, RawItem};
use crate::limits::{LimitExceeded, LimitKind, Limits, Warning};
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
        let mut builder = ParagraphBuilder::from_inline_style(style, limits);
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

/// A constructor resource-limit or structural span error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RubyError {
    /// Structural metadata exceeded `Limits::max_items` before span validation.
    Limit(LimitExceeded),
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
        if let Self::Limit(error) = self {
            return error.fmt(f);
        }
        write!(f, "invalid ruby pairing: {self:?}")
    }
}
impl std::error::Error for RubyError {}

impl From<LimitExceeded> for RubyError {
    fn from(error: LimitExceeded) -> Self {
        Self::Limit(error)
    }
}

#[cfg(test)]
thread_local! {
    pub(super) static VALIDATED_ANNOTATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static SPAN_TABLES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Ruby {
    /// Validate spans using default resource limits, without expanding columns.
    /// Use [`Self::with_limits`] to choose an explicit structural metadata budget.
    pub fn new(bases: Vec<RubyBase>, levels: Vec<RubyLevel>) -> Result<Self, RubyError> {
        Self::with_limits(bases, levels, &Limits::default())
    }

    /// Validate nonempty, bounded, nonoverlapping spans with explicit limits.
    ///
    /// Before examining annotations or allocating/sorting span tables, check
    /// [`Limits::max_items`] against one container, every base and level, and
    /// every annotation occurrence. The first excessive count returns
    /// [`RubyError::Limit`], even if a span is also invalid. Hidden/collapsed
    /// annotations and shared content count separately.
    ///
    /// This per-constructor admission bounds validation scratch and sorting;
    /// it does not account for caller-owned vector capacity or content payload.
    /// Dropping rejected owned inputs still visits their records. The parent
    /// independently checks projected anonymous boxes and aggregate resources
    /// when the ruby is appended. `max_items: None` disables this constructor
    /// budget and should only be used for trusted input.
    pub fn with_limits(
        bases: Vec<RubyBase>,
        levels: Vec<RubyLevel>,
        limits: &Limits,
    ) -> Result<Self, RubyError> {
        // Bound the outer walk before inspecting per-level vector lengths;
        // then admit all raw metadata before touching any annotation element.
        // This is a lower bound on normalized metadata, whose anonymous columns
        // and aggregate parent cost are checked independently by normalize.
        let mut items = 1u64
            .saturating_add(bases.len() as u64)
            .saturating_add(levels.len() as u64);
        Limits::check(limits.max_items, LimitKind::Items, items)?;
        for level in &levels {
            items = items.saturating_add(level.annotations.len() as u64);
            Limits::check(limits.max_items, LimitKind::Items, items)?;
        }
        let mut columns = bases.len();
        for level in &levels {
            for (i, annotation) in level.annotations.iter().enumerate() {
                #[cfg(test)]
                VALIDATED_ANNOTATIONS.with(|count| count.set(count.get() + 1));
                match annotation.span {
                    RubySpan::Auto => columns = columns.max(i + 1),
                    RubySpan::All => columns = columns.max(1),
                    RubySpan::Columns(_) => {}
                }
            }
        }
        for (level_index, level) in levels.iter().enumerate() {
            #[cfg(test)]
            SPAN_TABLES.with(|count| count.set(count.get() + 1));
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
    pub(crate) limits: Limits,
    pub(crate) text: String,
    pub(crate) items: Vec<RawItem>,
    pub(crate) styles: Vec<InlineStyle>,
    pub(crate) first_line_styles: FastMap<u32, InlineStyle>,
    pub(crate) error: Option<LimitExceeded>,
    pub(crate) warnings: Vec<Warning>,
    pub(crate) offset_mapping: bool,
    pub(crate) has_first_line: bool,
    pub(crate) rubies: Vec<super::builder::RubyInput>,
    pub(crate) ruby_cost: super::builder::InputCost,
    /// Frozen before sharing; aliases never re-walk items or style payloads.
    /// Any future payload mutation must rebuild both cached summaries.
    pub(crate) cost: super::builder::InputCost,
    /// Only objects in this source stream, excluding annotation lanes.
    pub(crate) holds_object: bool,
}
