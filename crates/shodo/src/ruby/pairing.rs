use std::ops::Range;

use super::input::{Ruby, RubyAlign, RubyContent, RubyMerge, RubyStyle, RubyVisibility, span};
use crate::limits::{LimitExceeded, LimitKind, Limits};
use crate::node::NodeId;

#[cfg(test)]
thread_local! {
    pub(super) static COMPARED_BYTES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    pub(super) static COMPARED_BASES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Debug)]
pub(crate) struct NormalizedBase {
    pub(crate) node: Option<NodeId>,
    /// Cleared after importing into the parent; the base is retained only once.
    pub(crate) content: Option<RubyContent>,
    pub(crate) limits: Option<Limits>,
    pub(crate) retained_styles: u64,
    pub(crate) retained_style_indices: Vec<u32>,
    pub(crate) align: RubyAlign,
}

#[derive(Clone, Debug)]
pub(crate) struct NormalizedAnnotation {
    pub(crate) node: Option<NodeId>,
    pub(crate) content: Option<RubyContent>,
    pub(crate) columns: Range<usize>,
    pub(crate) visibility: RubyVisibility,
    pub(crate) auto_hidden: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct NormalizedLevel {
    pub(crate) style: RubyStyle,
    pub(crate) annotations: Vec<NormalizedAnnotation>,
}

#[derive(Clone, Debug)]
pub(crate) struct NormalizedRuby {
    pub(crate) bases: Vec<NormalizedBase>,
    pub(crate) levels: Vec<NormalizedLevel>,
    pub(crate) metadata_items: u64,
}

pub(crate) fn normalize(ruby: &Ruby, limits: &Limits) -> Result<NormalizedRuby, LimitExceeded> {
    // Count all real and anonymous records before allocating a column table.
    // Invalid spans have already been rejected by Ruby::new.
    let mut metadata_items = 1u64
        .saturating_add(ruby.columns as u64)
        .saturating_add(ruby.levels.len() as u64);
    for level in &ruby.levels {
        let occupied = level
            .annotations
            .iter()
            .enumerate()
            .fold(0u64, |count, (i, a)| {
                let range = span(&a.span, i, ruby.columns);
                count.saturating_add((range.end - range.start) as u64)
            });
        metadata_items = metadata_items
            .saturating_add(level.annotations.len() as u64)
            .saturating_add((ruby.columns as u64).saturating_sub(occupied));
    }
    Limits::check(limits.max_items, LimitKind::Items, metadata_items)?;
    // Empty columns contribute no bytes. Index nonempty bases once so each
    // comparison visits only bases whose bytes it actually needs to consume.
    let text_bases: Vec<_> = ruby
        .bases
        .iter()
        .enumerate()
        .filter(|(_, base)| !base.content.0.text.is_empty())
        .collect();
    let mut bases = Vec::with_capacity(ruby.columns);
    for column in 0..ruby.columns {
        bases.push(match ruby.bases.get(column) {
            Some(base) => NormalizedBase {
                node: Some(base.node),
                content: Some(base.content.clone()),
                limits: Some(base.content.0.limits.clone()),
                retained_styles: 0,
                retained_style_indices: Vec::new(),
                align: base.align,
            },
            None => NormalizedBase {
                node: None,
                content: None,
                limits: None,
                retained_styles: 0,
                retained_style_indices: Vec::new(),
                align: RubyAlign::default(),
            },
        });
    }
    let mut levels = Vec::with_capacity(ruby.levels.len());
    for level in &ruby.levels {
        let mut annotations = Vec::new();
        for (ordinal, annotation) in level.annotations.iter().enumerate() {
            let columns = span(&annotation.span, ordinal, ruby.columns);
            // text is the original source stream, before collapse/transform,
            // including nested bases but excluding their annotation lanes.
            let text = annotation.content.0.text.bytes();
            #[cfg(test)]
            let mut compared = 0;
            #[cfg(test)]
            let text = text.inspect(|_| compared += 1);
            let start = text_bases.partition_point(|(column, _)| *column < columns.start);
            let same_text = text_bases[start..]
                .iter()
                .take_while(|(column, _)| *column < columns.end)
                .flat_map(|(_, b)| {
                    #[cfg(test)]
                    COMPARED_BASES.with(|count| count.set(count.get() + 1));
                    b.content.0.text.bytes()
                })
                .eq(text);
            #[cfg(test)]
            COMPARED_BYTES.with(|count| count.set(count.get() + compared));
            annotations.push(NormalizedAnnotation {
                node: Some(annotation.node),
                content: Some(annotation.content.clone()),
                columns,
                visibility: annotation.visibility,
                auto_hidden: level.style.merge != RubyMerge::Merge && same_text,
            });
        }
        annotations.sort_unstable_by_key(|a| a.columns.start);
        // Fill uncovered columns without nodes or empty paragraph allocations.
        let mut result = Vec::new();
        let mut column = 0;
        for annotation in annotations {
            while column < annotation.columns.start {
                result.push(empty(column));
                column += 1;
            }
            column = annotation.columns.end;
            result.push(annotation);
        }
        while column < ruby.columns {
            result.push(empty(column));
            column += 1;
        }
        levels.push(NormalizedLevel {
            style: level.style,
            annotations: result,
        });
    }
    Ok(NormalizedRuby {
        bases,
        levels,
        metadata_items,
    })
}

fn empty(column: usize) -> NormalizedAnnotation {
    NormalizedAnnotation {
        node: None,
        content: None,
        columns: column..column + 1,
        visibility: RubyVisibility::Collapse,
        auto_hidden: false,
    }
}
