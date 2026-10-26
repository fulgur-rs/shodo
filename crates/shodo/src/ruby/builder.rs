use super::input::{ContentInput, Ruby};
use super::pairing::{NormalizedRuby, normalize};
use crate::builder::{ParagraphBuilder, RawItem};
use crate::limits::{LimitExceeded, LimitKind, Limits};
use crate::node::{InlineEdges, NodeId};
use crate::style::{InlineStyle, UnicodeBidi};

#[cfg(test)]
thread_local! {
    pub(super) static SUMMARY_ITEM_VISITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    pub(super) static PREPARED_MAPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    pub(super) static INHERITED_STYLE_COPIES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    pub(super) static WRAPPER_INHERITED_COPIES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Boundary {
    ContainerOpen,
    ContainerClose,
    BaseOpen(usize),
    BaseClose(usize),
}

/// Counts retained annotation inputs and pairing metadata outside the raw
/// parent stream. Reusing an Arc for another lane still costs another lane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
        input.cost
    }

    pub(crate) fn measure(input: &ContentInput) -> (Self, bool) {
        let mut depth = 0u64;
        let mut max_depth = input.ruby_cost.depth;
        let mut holds_object = false;
        for item in &input.items {
            #[cfg(test)]
            SUMMARY_ITEM_VISITS.with(|n| n.set(n.get() + 1));
            #[cfg(test)]
            super::pairing::OBJECT_ITEM_VISITS.with(|n| n.set(n.get() + 1));
            match item {
                RawItem::Open { .. } => {
                    depth += 1;
                    max_depth = max_depth.max(depth);
                }
                RawItem::Close => depth = depth.saturating_sub(1),
                RawItem::Atomic { .. } => holds_object = true,
                _ => {}
            }
        }
        (
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
            },
            holds_object,
        )
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
    let metadata_items = super::pairing::metadata_items(&ruby);
    Limits::check(remaining.max_items, LimitKind::Items, metadata_items).map_err(|mut error| {
        if let Some(limit) = b.limits.max_items {
            error.limit = limit;
            error.actual = error
                .actual
                .saturating_add(b.items.len() as u64)
                .saturating_add(b.ruby_cost.items);
        }
        error
    })?;
    let mut extra = InputCost {
        items: metadata_items,
        ..Default::default()
    };
    let parent_depth = b.stack.len() as u64;
    for base in &ruby.bases {
        let content = &base.content;
        // Imported base styles are interned, not charged as fresh retained copies.
        extra.add(content.0.ruby_cost);
        extra.depth = extra.depth.max(
            parent_depth
                .saturating_add(2)
                .saturating_add(content.0.cost.depth),
        );
    }
    for annotation in ruby.levels.iter().flat_map(|level| &level.annotations) {
        let cost = annotation.content.0.cost;
        extra.add(cost);
        extra.depth = extra
            .depth
            .max(parent_depth.saturating_add(2).saturating_add(cost.depth));
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
    // Include the raw items that import will copy and every emitted wrapper/
    // marker. Reject before preparing styles or allocating normalized columns.
    let raw_items = ruby.bases.iter().fold(
        4u64.saturating_add((ruby.columns as u64).saturating_mul(2))
            .saturating_add((ruby.bases.len() as u64).saturating_mul(2)),
        |total, base| total.saturating_add(base.content.0.items.len() as u64),
    );
    let projected = (b.items.len() as u64)
        .saturating_add(b.ruby_cost.items)
        .saturating_add(extra.items)
        .saturating_add(raw_items);
    if let Some(limit) = b.limits.max_items
        && projected > limit
    {
        // Preserve the first failed reserve_item value from the old streaming
        // import, after the existing metadata/retained checks above.
        return Err(LimitExceeded {
            kind: LimitKind::Items,
            limit,
            actual: limit.saturating_add(1),
        });
    }
    let mut raw_text = b.text.len() as u64;
    for base in &ruby.bases {
        raw_text = raw_text.saturating_add(base.content.0.text.len() as u64);
        Limits::check(Some(u32::MAX as u64), LimitKind::TextBytes, raw_text)?;
    }
    b.ruby_cost.add(extra);
    let prepared = prepare_styles(b, normal, first_line, &ruby)?;
    let mut normalized = normalize(&ruby, &remaining)?;
    debug_assert_eq!(normalized.metadata_items, metadata_items);
    open_prepared(b, node, prepared.container);
    let index = b.rubies.len() as u32;
    let style = prepared.container;
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
            let plan = &prepared.bases[column];
            open_prepared(b, base_node, plan.wrapper);
            import_items(b, &content.0, &plan.mapping);
            base.retained_styles = plan.retained.len() as u64;
            base.retained_style_indices = plan.retained.clone();
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

struct PreparedBase {
    wrapper: u32,
    mapping: Vec<u32>,
    retained: Vec<u32>,
}

struct PreparedStyles {
    container: u32,
    bases: Vec<std::sync::Arc<PreparedBase>>,
}

fn prediction_work(
    b: &mut ParagraphBuilder,
    pair: (&InlineStyle, Option<&InlineStyle>),
) -> Result<(), LimitExceeded> {
    if b.reserve_style_work(pair) {
        Ok(())
    } else {
        Err(b.error.expect("style work rejection records its error"))
    }
}

fn prepare_base(
    b: &mut ParagraphBuilder,
    content: &super::input::RubyContent,
) -> Result<(u32, Vec<u32>), LimitExceeded> {
    Limits::check(
        content.0.limits.max_nesting_depth,
        LimitKind::NestingDepth,
        InputCost::content(&content.0).depth.saturating_add(1),
    )?;
    prediction_work(
        b,
        (&content.0.style.root, content.0.style.first_line.as_ref()),
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
    let has_first = content.0.style.first_line.is_some() || !content.0.first_line_styles.is_empty();
    let mut wrapper_exists = false;
    for (i, normal) in content.0.styles.iter().enumerate() {
        prediction_work(b, (normal, None))?;
        if *normal != root {
            continue;
        }
        let explicit = content.0.first_line_styles.get(&(i as u32));
        let inherited = if has_first && explicit.is_none() {
            prediction_work(
                b,
                (
                    normal,
                    Some(
                        content
                            .0
                            .style
                            .first_line
                            .as_ref()
                            .unwrap_or(&content.0.style.root),
                    ),
                ),
            )?;
            #[cfg(test)]
            WRAPPER_INHERITED_COPIES.with(|n| n.set(n.get() + 1));
            Some(crate::paragraph::first_line_style(
                normal,
                &content.0.style.root,
                content
                    .0
                    .style
                    .first_line
                    .as_ref()
                    .unwrap_or(&content.0.style.root),
            ))
        } else {
            None
        };
        if let Some(explicit) = explicit {
            prediction_work(b, (explicit, None))?;
        }
        if explicit.or(inherited.as_ref()) == first.as_ref() {
            wrapper_exists = true;
            break;
        }
    }
    Limits::check(
        content.0.limits.max_styles,
        LimitKind::Styles,
        cost.styles.saturating_add(u64::from(!wrapper_exists)),
    )?;

    let wrapper = b
        .intern_styles(&root, first.as_ref())
        .ok_or_else(|| b.error.expect("intern failure records its error"))?;
    b.stack.push(wrapper);
    let mapping = import_styles(b, &content.0);
    b.stack.pop();
    if let Some(error) = b.error {
        return Err(error);
    }
    Ok((wrapper, mapping))
}

fn prepare_styles(
    b: &mut ParagraphBuilder,
    normal: &InlineStyle,
    first_line: Option<&InlineStyle>,
    ruby: &Ruby,
) -> Result<PreparedStyles, LimitExceeded> {
    let depth = b.stack.len();
    let result = (|| {
        prediction_work(b, (normal, first_line))?;
        Limits::check(
            b.limits.max_style_bytes,
            LimitKind::StyleBytes,
            crate::style::memory::inline(normal)
                .saturating_add(first_line.map_or(0, crate::style::memory::inline)),
        )?;
        let normal = isolated(normal);
        let first = first_line.map(isolated);
        let container = b
            .intern_styles(&normal, first.as_ref())
            .ok_or_else(|| b.error.expect("intern failure records its error"))?;
        b.stack.push(container);
        let mut bases = Vec::with_capacity(ruby.bases.len());
        // Keep only one durable index map per unique snapshot. Interning still
        // executes in its original order, so StyleWork is charged just once.
        let mut shared =
            crate::hashing::FastMap::<*const ContentInput, std::sync::Arc<PreparedBase>>::default();
        for base in &ruby.bases {
            let (wrapper, mapping) = prepare_base(b, &base.content)?;
            let key = std::sync::Arc::as_ptr(&base.content.0);
            let plan = match shared.get(&key) {
                Some(plan) if plan.wrapper == wrapper && plan.mapping == mapping => {
                    std::sync::Arc::clone(plan)
                }
                _ => {
                    #[cfg(test)]
                    PREPARED_MAPS.with(|n| n.set(n.get() + 1));
                    let mut retained = mapping.clone();
                    retained.push(wrapper);
                    retained.sort_unstable();
                    retained.dedup();
                    let plan = std::sync::Arc::new(PreparedBase {
                        wrapper,
                        mapping,
                        retained,
                    });
                    shared.insert(key, std::sync::Arc::clone(&plan));
                    plan
                }
            };
            let bytes = (plan.retained.len() as u64).saturating_mul(size_of::<u32>() as u64);
            Limits::check(
                b.limits.max_style_bytes,
                LimitKind::StyleBytes,
                b.style_bytes
                    .saturating_add(b.ruby_cost.style_bytes)
                    .saturating_add(bytes),
            )?;
            b.ruby_cost.style_bytes = b.ruby_cost.style_bytes.saturating_add(bytes);
            bases.push(plan);
        }
        Ok(PreparedStyles { container, bases })
    })();
    // Both success and failure restore the caller's actual open-inline stack.
    // last_interned deliberately keeps the result of the real key operations.
    b.stack.truncate(depth);
    result
}

fn open_prepared(b: &mut ParagraphBuilder, node: NodeId, style: u32) {
    if b.reserve_item() {
        b.items.push(RawItem::Open {
            node,
            style,
            edges: InlineEdges::default(),
            percents: None,
        });
        b.stack.push(style);
    }
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

fn import_styles(b: &mut ParagraphBuilder, input: &ContentInput) -> Vec<u32> {
    if b.error.is_some() {
        return Vec::new();
    }
    let mut styles = Vec::with_capacity(input.styles.len());
    let has_first_line = input.style.first_line.is_some() || !input.first_line_styles.is_empty();
    for (i, style) in input.styles.iter().enumerate() {
        // Resolve snapshot-local legacy inheritance before moving the styles
        // into a parent with a different root. Explicit pairs still win.
        let explicit = input.first_line_styles.get(&(i as u32));
        if has_first_line {
            let bytes = crate::style::memory::alternate(
                style,
                &input.style.root,
                explicit.unwrap_or(input.style.first_line.as_ref().unwrap_or(&input.style.root)),
                explicit.is_none(),
            );
            if !b.check(b.limits.max_style_bytes, LimitKind::StyleBytes, bytes) {
                return styles;
            }
        }
        let inherited = (has_first_line && explicit.is_none()).then(|| {
            #[cfg(test)]
            INHERITED_STYLE_COPIES.with(|n| n.set(n.get() + 1));
            crate::paragraph::first_line_style(
                style,
                &input.style.root,
                input.style.first_line.as_ref().unwrap_or(&input.style.root),
            )
        });
        let first = explicit.or(inherited.as_ref());
        let Some(index) = b.intern_styles(style, first) else {
            return Vec::new();
        };
        styles.push(index);
    }
    styles
}

fn import_items(b: &mut ParagraphBuilder, input: &ContentInput, styles: &[u32]) {
    if b.error.is_some() {
        return;
    }
    b.warnings.extend_from_slice(&input.warnings);
    let offset = b.text.len() as u64;
    let total = offset.saturating_add(input.text.len() as u64);
    if !b.check(Some(u64::from(u32::MAX)), LimitKind::TextBytes, total)
        || !b.check(
            b.limits.max_text_bytes,
            LimitKind::TextBytes,
            total.saturating_add(b.ruby_cost.text),
        )
    {
        return;
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
            return;
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
