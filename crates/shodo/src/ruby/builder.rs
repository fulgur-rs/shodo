use super::input::{ContentInput, Ruby};
use super::pairing::{NormalizedRuby, normalize};
use crate::builder::{ParagraphBuilder, RawItem};
use crate::limits::{LimitExceeded, LimitKind, Limits};
use crate::node::{InlineEdges, NodeId};
use crate::style::{InlineStyle, UnicodeBidi};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Boundary {
    ContainerOpen,
    ContainerClose,
    BaseOpen(usize),
    BaseClose(usize),
}

/// Counts retained annotation inputs and pairing metadata outside the raw
/// parent stream. Reusing an Arc for another lane still costs another lane.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct InputCost {
    pub(crate) text: u64,
    pub(crate) items: u64,
    pub(crate) styles: u64,
    pub(crate) style_bytes: u64,
    pub(crate) depth: u64,
}

impl InputCost {
    fn add(&mut self, other: Self) {
        self.text = self.text.saturating_add(other.text);
        self.items = self.items.saturating_add(other.items);
        self.styles = self.styles.saturating_add(other.styles);
        self.style_bytes = self.style_bytes.saturating_add(other.style_bytes);
        self.depth = self.depth.max(other.depth);
    }

    pub(crate) fn content(input: &ContentInput) -> Self {
        let mut depth = 0u64;
        let mut max_depth = input.ruby_cost.depth;
        for item in &input.items {
            match item {
                RawItem::Open { .. } => {
                    depth += 1;
                    max_depth = max_depth.max(depth);
                }
                RawItem::Close => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        Self {
            text: (input.text.len() as u64).saturating_add(input.ruby_cost.text),
            items: (input.items.len() as u64).saturating_add(input.ruby_cost.items),
            styles: (input.styles.len() as u64)
                .saturating_add(input.first_line_styles.len() as u64)
                .saturating_add(input.ruby_cost.styles),
            style_bytes: crate::style::memory::paragraph(&input.style)
                .saturating_add(crate::style::memory::styles(&input.styles))
                .saturating_add(crate::style::memory::styles(
                    input.first_line_styles.values(),
                ))
                .saturating_add(input.ruby_cost.style_bytes),
            depth: max_depth,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RubyInput {
    #[cfg(test)]
    pub(crate) _clone_probe: input_clone_probe::CloneProbe,
    pub(crate) node: NodeId,
    pub(crate) style: u32,
    pub(crate) normalized: NormalizedRuby,
}

/// Append balanced base streams without inserting annotations into parent text.
pub(crate) fn append(
    builder: &mut ParagraphBuilder,
    node: NodeId,
    normal: &InlineStyle,
    first_line: Option<&InlineStyle>,
    ruby: Ruby,
) {
    if builder.error.is_some() {
        return;
    }
    if let Err(error) = append_checked(builder, node, normal, first_line, ruby) {
        builder.error = Some(error);
    }
}

fn append_checked(
    b: &mut ParagraphBuilder,
    node: NodeId,
    normal: &InlineStyle,
    first_line: Option<&InlineStyle>,
    ruby: Ruby,
) -> Result<(), LimitExceeded> {
    let mut text = (b.text.len() as u64).saturating_add(b.ruby_cost.text);
    // A failed snapshot never turns into valid empty input, even when hidden.
    for content in ruby.bases.iter().map(|base| &base.content).chain(
        ruby.levels
            .iter()
            .flat_map(|level| level.annotations.iter().map(|a| &a.content)),
    ) {
        if let Some(error) = content.0.error {
            return Err(error);
        }
        // Count every occurrence, including hidden and nested annotation text,
        // before normalization can compare the original source streams.
        text = text
            .saturating_add(content.0.text.len() as u64)
            .saturating_add(content.0.ruby_cost.text);
    }
    Limits::check(b.limits.max_text_bytes, LimitKind::TextBytes, text)?;
    // Every annotation snapshot is retained, including hidden lanes and Arc
    // aliases. Imported bases keep only their nested annotation inputs.
    let mut style_bytes = b.style_bytes.saturating_add(b.ruby_cost.style_bytes);
    for base in &ruby.bases {
        let cost = InputCost::content(&base.content.0);
        Limits::check(
            b.limits.max_style_bytes,
            LimitKind::StyleBytes,
            cost.style_bytes,
        )?;
        style_bytes = style_bytes.saturating_add(base.content.0.ruby_cost.style_bytes);
    }
    for annotation in ruby.levels.iter().flat_map(|level| &level.annotations) {
        style_bytes =
            style_bytes.saturating_add(InputCost::content(&annotation.content.0).style_bytes);
    }
    Limits::check(b.limits.max_style_bytes, LimitKind::StyleBytes, style_bytes)?;
    let mut remaining = b.limits.clone();
    remaining.max_items = remaining.max_items.map(|n| {
        n.saturating_sub(b.items.len() as u64)
            .saturating_sub(b.ruby_cost.items)
    });
    let mut normalized = normalize(&ruby, &remaining).map_err(|mut error| {
        if error.kind == LimitKind::Items
            && let Some(limit) = b.limits.max_items
        {
            error.limit = limit;
            error.actual = error
                .actual
                .saturating_add(b.items.len() as u64)
                .saturating_add(b.ruby_cost.items);
        }
        error
    })?;
    let mut extra = InputCost {
        items: normalized.metadata_items,
        ..Default::default()
    };
    let parent_depth = b.stack.len() as u64;
    for base in &normalized.bases {
        if let Some(content) = &base.content {
            // Its main bytes/items/styles will be imported, not retained again.
            extra.add(content.0.ruby_cost);
            extra.depth = extra.depth.max(
                parent_depth
                    .saturating_add(2)
                    .saturating_add(InputCost::content(&content.0).depth),
            );
        }
    }
    for annotation in normalized
        .levels
        .iter()
        .flat_map(|level| &level.annotations)
    {
        if let Some(content) = &annotation.content {
            let cost = InputCost::content(&content.0);
            extra.add(cost);
            extra.depth = extra
                .depth
                .max(parent_depth.saturating_add(2).saturating_add(cost.depth));
        }
    }
    extra.depth = extra.depth.max(parent_depth.saturating_add(1));
    Limits::check(
        b.limits.max_items,
        LimitKind::Items,
        (b.items.len() as u64)
            .saturating_add(b.ruby_cost.items)
            .saturating_add(extra.items),
    )?;
    Limits::check(
        b.limits.max_styles,
        LimitKind::Styles,
        (b.styles.len() as u64)
            .saturating_add(b.first_line_styles.len() as u64)
            .saturating_add(b.ruby_cost.styles)
            .saturating_add(extra.styles),
    )?;
    Limits::check(
        b.limits.max_style_bytes,
        LimitKind::StyleBytes,
        b.style_bytes
            .saturating_add(b.ruby_cost.style_bytes)
            .saturating_add(extra.style_bytes),
    )?;
    Limits::check(
        b.limits.max_nesting_depth,
        LimitKind::NestingDepth,
        extra.depth,
    )?;
    b.ruby_cost.add(extra);
    Limits::check(
        b.limits.max_style_bytes,
        LimitKind::StyleBytes,
        crate::style::memory::inline(normal)
            .saturating_add(first_line.map_or(0, crate::style::memory::inline)),
    )?;
    let normal = isolated(normal);
    let first = first_line.map(isolated);
    match first.as_ref() {
        Some(first) => {
            b.open_inline_with_first_line(node, &normal, first, InlineEdges::default());
        }
        None => {
            b.open_inline(node, &normal, InlineEdges::default());
        }
    }
    if let Some(error) = b.error {
        return Err(error);
    }
    let index = b.rubies.len() as u32;
    let style = b.current_style();
    // Reserve the outer index before importing nested ruby indices.
    b.rubies.push(RubyInput {
        #[cfg(test)]
        _clone_probe: Default::default(),
        node,
        style,
        normalized: NormalizedRuby {
            bases: Vec::new(),
            levels: Vec::new(),
            metadata_items: 0,
        },
    });
    marker(b, index, Boundary::ContainerOpen, Some(node));
    for (column, base) in normalized.bases.iter_mut().enumerate() {
        marker(b, index, Boundary::BaseOpen(column), base.node);
        if let (Some(base_node), Some(content)) = (base.node, base.content.take()) {
            Limits::check(
                content.0.limits.max_nesting_depth,
                LimitKind::NestingDepth,
                InputCost::content(&content.0).depth.saturating_add(1),
            )?;
            let root = isolated(&content.0.style.root);
            let first = content.0.style.first_line.as_ref().map(isolated);
            let cost = InputCost::content(&content.0);
            Limits::check(
                content.0.limits.max_items,
                LimitKind::Items,
                cost.items.saturating_add(2),
            )?;
            // Import resolves legacy alternatives without changing normal styles.
            // Predict the wrapper style before interning or copying any input.
            let has_first =
                content.0.style.first_line.is_some() || !content.0.first_line_styles.is_empty();
            let wrapper_exists = content.0.styles.iter().enumerate().any(|(i, normal)| {
                if *normal != root {
                    return false;
                }
                let inherited = has_first.then(|| {
                    crate::paragraph::first_line_style(
                        normal,
                        &content.0.style.root,
                        content
                            .0
                            .style
                            .first_line
                            .as_ref()
                            .unwrap_or(&content.0.style.root),
                    )
                });
                content
                    .0
                    .first_line_styles
                    .get(&(i as u32))
                    .or(inherited.as_ref())
                    == first.as_ref()
            });
            Limits::check(
                content.0.limits.max_styles,
                LimitKind::Styles,
                cost.styles.saturating_add(u64::from(!wrapper_exists)),
            )?;

            match first.as_ref() {
                Some(first) => {
                    b.open_inline_with_first_line(base_node, &root, first, InlineEdges::default());
                }
                None => {
                    b.open_inline(base_node, &root, InlineEdges::default());
                }
            }
            let wrapper = b.current_style();
            let mut imported = import(b, &content.0);
            imported.push(wrapper);
            imported.sort_unstable();
            imported.dedup();
            // Scope-index cells are temporary style metadata. Raw input
            // accounting bounds all copies until ruby preparation drops them.
            let bytes = (imported.len() as u64).saturating_mul(std::mem::size_of::<u32>() as u64);
            Limits::check(
                b.limits.max_style_bytes,
                LimitKind::StyleBytes,
                b.style_bytes
                    .saturating_add(b.ruby_cost.style_bytes)
                    .saturating_add(bytes),
            )?;
            b.ruby_cost.style_bytes = b.ruby_cost.style_bytes.saturating_add(bytes);
            base.retained_styles = imported.len() as u64;
            base.retained_style_indices = imported;
            b.close_inline();
        }
        marker(b, index, Boundary::BaseClose(column), base.node);
    }
    marker(b, index, Boundary::ContainerClose, Some(node));
    b.close_inline();
    b.rubies[index as usize].normalized = normalized;
    if let Some(error) = b.error {
        return Err(error);
    }
    Ok(())
}

pub(crate) fn isolated(style: &InlineStyle) -> InlineStyle {
    let mut style = style.clone();
    style.unicode_bidi = match style.unicode_bidi {
        UnicodeBidi::BidiOverride | UnicodeBidi::IsolateOverride => UnicodeBidi::IsolateOverride,
        UnicodeBidi::Plaintext => UnicodeBidi::Plaintext,
        _ => UnicodeBidi::Isolate,
    };
    style
}

fn marker(b: &mut ParagraphBuilder, ruby: u32, boundary: Boundary, node: Option<NodeId>) {
    if b.reserve_item() {
        b.items.push(RawItem::RubyBoundary {
            ruby,
            boundary,
            node,
            style: b.current_style(),
        });
    }
}

fn import(b: &mut ParagraphBuilder, input: &ContentInput) -> Vec<u32> {
    if b.error.is_some() {
        return Vec::new();
    }
    for warning in &input.warnings {
        b.warnings.push(warning.kind, warning.message.clone());
    }
    let mut styles = Vec::with_capacity(input.styles.len());
    let has_first_line = input.style.first_line.is_some() || !input.first_line_styles.is_empty();
    for (i, style) in input.styles.iter().enumerate() {
        // Resolve snapshot-local legacy inheritance before moving the styles
        // into a parent with a different root. Explicit pairs still win.
        if has_first_line {
            let bytes = crate::style::memory::alternate(
                style,
                &input.style.root,
                input.style.first_line.as_ref().unwrap_or(&input.style.root),
                true,
            );
            if !b.check(b.limits.max_style_bytes, LimitKind::StyleBytes, bytes) {
                return styles;
            }
        }
        let inherited = has_first_line.then(|| {
            crate::paragraph::first_line_style(
                style,
                &input.style.root,
                input.style.first_line.as_ref().unwrap_or(&input.style.root),
            )
        });
        let first = input
            .first_line_styles
            .get(&(i as u32))
            .or(inherited.as_ref());
        let Some(index) = b.intern_styles(style, first) else {
            return Vec::new();
        };
        styles.push(index);
    }
    let offset = b.text.len() as u64;
    let total = offset.saturating_add(input.text.len() as u64);
    if !b.check(Some(u64::from(u32::MAX)), LimitKind::TextBytes, total)
        || !b.check(
            b.limits.max_text_bytes,
            LimitKind::TextBytes,
            total.saturating_add(b.ruby_cost.text),
        )
    {
        return Vec::new();
    }
    b.text.push_str(&input.text);
    let nested = b.rubies.len() as u32;
    for ruby in &input.rubies {
        let mut ruby = ruby.clone();
        ruby.style = styles[ruby.style as usize];
        for base in &mut ruby.normalized.bases {
            for index in &mut base.retained_style_indices {
                *index = styles[*index as usize];
            }
            base.retained_style_indices.sort_unstable();
            base.retained_style_indices.dedup();
        }
        b.rubies.push(ruby);
    }
    for item in &input.items {
        if !b.reserve_item() {
            return Vec::new();
        }
        let mut item = item.clone();
        match &mut item {
            RawItem::Text { range, style, .. } => {
                range.start += offset as u32;
                range.end += offset as u32;
                *style = styles[*style as usize];
            }
            RawItem::Atomic {
                style,
                parent_style,
                ..
            } => {
                *style = styles[*style as usize];
                *parent_style = styles[*parent_style as usize];
            }
            RawItem::RubyBoundary { ruby, style, .. } => {
                *ruby += nested;
                *style = styles[*style as usize];
            }
            RawItem::Open { style, .. }
            | RawItem::OutOfFlow { style, .. }
            | RawItem::BlockInInline { style, .. }
            | RawItem::ForcedBreak { style, .. } => *style = styles[*style as usize],
            RawItem::Close => {}
        }
        b.items.push(item);
    }
    styles
}

// Observe real metadata clones without a shipping field or counter.
#[cfg(test)]
pub(crate) mod input_clone_probe {
    use std::cell::Cell;
    thread_local! { static COUNT: Cell<usize> = const { Cell::new(0) }; }
    #[derive(Debug, Default)]
    pub(crate) struct CloneProbe;
    impl Clone for CloneProbe {
        fn clone(&self) -> Self {
            COUNT.with(|count| count.set(count.get() + 1));
            Self
        }
    }
    pub(crate) fn reset() {
        COUNT.with(|count| count.set(0));
    }
    pub(crate) fn count() -> usize {
        COUNT.with(Cell::get)
    }
}
