//! One clean rejected line, transferred on acceptance without cloning.
use crate::style::LineOptions;
use crate::{AtomicSizes, BreakToken, FloatCursor, Line, LineConstraint, Paragraph};

const MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, PartialEq)]
pub(crate) struct Key {
    owner: usize,
    token: BreakToken,
    built_fonts: (u64, Option<u64>),
    live_fonts: (u64, Option<u64>),
    width: u32,
    inline_offset: u32,
    block_offset: u32,
    options: LineOptions,
    graphemes: Option<usize>,
    floats: Option<FloatCursor>,
    atomics_generation: u64,
    atomics_revision: u64,
}
impl Key {
    pub(crate) fn new(
        p: &Paragraph,
        token: BreakToken,
        options: LineOptions,
        c: &LineConstraint<'_>,
        atomics: &AtomicSizes,
    ) -> Self {
        Self {
            owner: std::sync::Arc::as_ptr(&p.data) as usize,
            token,
            built_fonts: p.data.generations,
            live_fonts: p.data.fonts.generations(),
            width: c.available_inline_size.to_bits(),
            inline_offset: c.inline_start_offset.to_bits(),
            block_offset: c.block_offset.to_bits(),
            options,
            graphemes: c.max_graphemes,
            floats: c.floats_placed_through,
            atomics_generation: atomics.generation(),
            atomics_revision: atomics.revision,
        }
    }
}
#[derive(Debug)]
pub(crate) struct CompletedLine {
    pub(crate) key: Key,
    pub(crate) line: Line,
}
impl CompletedLine {
    pub(crate) fn retain(key: Key, line: Line) -> Option<Box<Self>> {
        // Includes the entry/key/Line header and context's entry pointer.
        let header = std::mem::size_of::<Self>() + std::mem::size_of::<Option<Box<Self>>>();
        line.owned_heap_bytes(MAX_BYTES.checked_sub(header)?)?;
        // Check the bounded owner tree after the capacity guard, including
        // child-only build warnings not forwarded to the parent paragraph.
        if !clean_owners(&line, 0) {
            return None;
        }
        Some(Box::new(Self { key, line }))
    }
    pub(crate) fn same_start(&self, p: &Paragraph, token: BreakToken) -> bool {
        self.key.owner == std::sync::Arc::as_ptr(&p.data) as usize && self.key.token == token
    }
}

fn clean_owners(line: &Line, depth: usize) -> bool {
    depth < 64
        && line.data.warnings.is_empty()
        && line
            .ruby
            .iter()
            .all(|ruby| ruby.paragraph.warnings().is_empty() && clean_owners(&ruby.line, depth + 1))
}

#[cfg(test)]
mod tests;
