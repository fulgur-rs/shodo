//! Development-only caller protocol. Checkpoints own all mutable layout state.
// Each example/test uses a different subset of the shared support API.
#![allow(dead_code)]
use shodo::{
    AtomicSizes, BreakToken, FloatCursor, LayoutContext, Line, LineConstraint, LineResult,
    Paragraph,
};
use shodo::{node::NodeId, style::LineOptions};
use taffy::compute::FloatContext;
pub use taffy::{Clear, FloatDirection as Side};

#[derive(Clone, Copy, Debug)]
pub struct FloatSpec {
    pub node: NodeId,
    pub side: Side,
    pub clear: Clear,
    pub width: f32,
    pub height: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub inline_start: f32,
    pub block_start: f32,
    pub inline_size: f32,
    pub block_size: f32,
}
#[derive(Clone, Debug)]
pub struct Placement {
    pub node: NodeId,
    pub rect: Rect,
    epoch: u64,
    cursor: FloatCursor,
    spec: FloatSpec,
    min_y: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Report {
    node: NodeId,
    cursor: FloatCursor,
}
#[derive(Clone, Debug)]
pub struct Checkpoint {
    paragraph: u64,
    epoch: u64,
    token: BreakToken,
    cursor: Option<FloatCursor>,
    bfc: FloatContext,
    placed: Vec<Placement>,
    pending: Vec<Report>,
    withdrawn: Vec<Report>,
    block: f32,
    width: f32,
    fragment: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowError {
    InvalidGeometry,
    MissingFloat,
    DuplicateFloat,
    InvalidCheckpoint,
    RetryLimit,
    InvalidToken,
    UnknownResult,
}
impl std::fmt::Display for FlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "float flow: {self:?}")
    }
}
impl std::error::Error for FlowError {}
fn nonnegative(v: f32) -> bool {
    v.is_finite() && v >= 0.0
}
impl Checkpoint {
    pub fn new(p: &Paragraph, width: f32) -> Result<Self, FlowError> {
        if !width.is_finite() || width <= 0.0 {
            return Err(FlowError::InvalidGeometry);
        }
        let mut bfc = FloatContext::new();
        bfc.set_width(width);
        Ok(Self {
            paragraph: p.id(),
            epoch: 0,
            token: p.start_token(),
            cursor: None,
            bfc,
            placed: Vec::new(),
            pending: Vec::new(),
            withdrawn: Vec::new(),
            block: 0.0,
            width,
            fragment: 0,
        })
    }
    pub fn token(&self) -> BreakToken {
        self.token
    }
    pub fn cursor(&self) -> Option<FloatCursor> {
        self.cursor
    }
    pub fn block(&self) -> f32 {
        self.block
    }
    pub fn fragment(&self) -> usize {
        self.fragment
    }
    pub fn placed(&self) -> &[Placement] {
        &self.placed
    }
    pub fn pending_nodes(&self) -> Vec<NodeId> {
        self.pending.iter().map(|r| r.node).collect()
    }
    pub fn withdrawn_nodes(&self) -> Vec<NodeId> {
        self.withdrawn.iter().map(|r| r.node).collect()
    }
    pub fn slot(&self, height: f32) -> (f32, f32) {
        let mut left: f32 = 0.0;
        let mut right = self.width;
        let bottom = self.block + height;
        for p in &self.placed {
            let r = p.rect;
            if (if height > 0.0 {
                r.block_start < bottom
            } else {
                r.block_start <= self.block
            }) && r.block_start + r.block_size > self.block
                && r.inline_size > 0.0
            {
                match p.spec.side {
                    Side::Left => left = left.max(r.inline_start + r.inline_size),
                    Side::Right => right = right.min(r.inline_start),
                }
            }
        }
        (left, (right - left).max(0.0))
    }

    pub fn clearance(&self, clear: Clear) -> Option<f32> {
        self.bfc.cleared_threshold(clear)
    }
    pub fn next_fragment(&self, consumed_height: f32, width: f32) -> Result<Self, FlowError> {
        if !nonnegative(consumed_height)
            || consumed_height < self.block
            || !width.is_finite()
            || width <= 0.0
        {
            return Err(FlowError::InvalidGeometry);
        }
        let mut next = self.clone();
        next.block = 0.0;
        next.width = width;
        next.fragment = next
            .fragment
            .checked_add(1)
            .ok_or(FlowError::InvalidGeometry)?;
        next.bfc = FloatContext::new();
        next.bfc.set_width(width);
        next.placed.clear();
        for p in &self.placed {
            let end = p.rect.block_start + p.rect.block_size;
            if end <= consumed_height {
                continue;
            }
            let mut carry = p.clone();
            carry.min_y = (p.rect.block_start - consumed_height).max(0.0);
            carry.spec.height = end - consumed_height - carry.min_y;
            // Prior clearance has already been resolved in the old fragment.
            carry.spec.clear = Clear::None;
            let xy = place_checked(&mut next.bfc, &next.placed, width, carry.spec, carry.min_y)?;
            carry.rect = Rect {
                inline_start: xy.x,
                block_start: xy.y,
                inline_size: carry.spec.width,
                block_size: carry.spec.height,
            };
            next.placed.push(carry);
        }
        Ok(next)
    }
    pub fn begin_paragraph(
        &self,
        previous: &Paragraph,
        next: &Paragraph,
        cx: &mut LayoutContext,
        options: &LineOptions,
        atomics: &AtomicSizes,
    ) -> Result<Self, FlowError> {
        if previous.id() != self.paragraph || !self.pending.is_empty() || !self.withdrawn.is_empty()
        {
            return Err(FlowError::InvalidCheckpoint);
        }
        let mut c = LineConstraint::new(self.width);
        c.floats_placed_through = self.cursor;
        if !matches!(
            previous.next_line(cx, self.token, options, &c, atomics),
            LineResult::Done
        ) {
            return Err(FlowError::InvalidCheckpoint);
        }
        let mut result = self.clone();
        result.paragraph = next.id();
        result.epoch = result
            .epoch
            .checked_add(1)
            .ok_or(FlowError::InvalidGeometry)?;
        result.token = next.start_token();
        result.cursor = None;
        Ok(result)
    }
    fn place(&mut self, spec: FloatSpec, report: Report, min_y: f32) -> Result<Rect, FlowError> {
        let xy = place_checked(&mut self.bfc, &self.placed, self.width, spec, min_y)?;
        let rect = Rect {
            inline_start: xy.x,
            block_start: xy.y,
            inline_size: spec.width,
            block_size: spec.height,
        };
        if !xy.x.is_finite() || !nonnegative(xy.y) || !(xy.y + spec.height).is_finite() {
            return Err(FlowError::InvalidGeometry);
        }
        self.placed.push(Placement {
            node: spec.node,
            rect,
            epoch: self.epoch,
            cursor: report.cursor,
            spec,
            min_y,
        });
        Ok(rect)
    }
    fn withdraw(&mut self, report: Report) -> Result<(), FlowError> {
        if self.cursor != Some(report.cursor) {
            return Err(FlowError::InvalidCheckpoint);
        }
        self.placed
            .retain(|p| p.epoch != self.epoch || p.cursor != report.cursor);
        self.pending.retain(|r| *r != report);
        self.cursor = report.cursor.before();
        if self.withdrawn.contains(&report) {
            return Err(FlowError::RetryLimit);
        }
        self.withdrawn.push(report);
        self.replay()
    }
    fn replay(&mut self) -> Result<(), FlowError> {
        self.bfc = FloatContext::new();
        self.bfc.set_width(self.width);
        let old = std::mem::take(&mut self.placed);
        for mut p in old {
            let xy = place_checked(&mut self.bfc, &self.placed, self.width, p.spec, p.min_y)?;
            p.rect.inline_start = xy.x;
            p.rect.block_start = xy.y;
            self.placed.push(p);
        }
        Ok(())
    }
    fn next_bottom(&self, height: f32) -> Option<f32> {
        self.placed
            .iter()
            .filter_map(|p| {
                let r = p.rect;
                let end = r.block_start + r.block_size;
                ((if height > 0.0 {
                    r.block_start < self.block + height
                } else {
                    r.block_start <= self.block
                }) && end > self.block
                    && r.inline_size > 0.0)
                    .then_some(end)
            })
            .min_by(f32::total_cmp)
    }
}
// Taffy 0.14 may lose an opposite-side inset when a taller float spans
// beyond an earlier segment. Supply independently retained rectangle insets
// for the requested band. Advance only over actual float bottoms if needed.
fn place_checked(
    bfc: &mut FloatContext,
    placed: &[Placement],
    width: f32,
    spec: FloatSpec,
    min_y: f32,
) -> Result<taffy::Point<f32>, FlowError> {
    let mut y = min_y.max(bfc.cleared_threshold(spec.clear).unwrap_or(0.0));
    if let Some(last) = placed.last() {
        y = y.max(last.rect.block_start);
    }
    for _ in 0..=placed.len() {
        let mut insets = [0.0_f32; 2];
        let mut bottom: Option<f32> = None;
        for p in placed {
            let r = p.rect;
            let end = r.block_start + r.block_size;
            if (if spec.height > 0.0 {
                r.block_start < y + spec.height
            } else {
                r.block_start <= y
            }) && end > y
                && r.inline_size > 0.0
            {
                let slot = match p.spec.side {
                    Side::Left => 0,
                    Side::Right => 1,
                };
                let inset = if slot == 0 {
                    r.inline_start + r.inline_size
                } else {
                    width - r.inline_start
                };
                insets[slot] = insets[slot].max(inset);
                bottom = Some(bottom.map_or(end, |v| v.min(end)));
            }
        }
        let lead = match spec.side {
            Side::Left => 0,
            Side::Right => 1,
        };
        let fits = spec.width <= width - insets[0] - insets[1];
        let overflow = insets[lead] == 0.0 && insets[1 - lead] == 0.0;
        if fits || overflow {
            let xy = bfc.place_floated_box(
                taffy::Size {
                    width: spec.width,
                    height: spec.height,
                },
                y,
                insets,
                spec.side,
                Clear::None,
            );
            if !xy.x.is_finite() || !nonnegative(xy.y) || !(xy.y + spec.height).is_finite() {
                return Err(FlowError::InvalidGeometry);
            }
            return Ok(xy);
        }
        y = bottom.ok_or(FlowError::InvalidGeometry)?;
    }
    Err(FlowError::RetryLimit)
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Outcome {
    Line(Line),
    Done,
    HeightRejected {
        needed: f32,
    },
    BlockBoundary {
        node: NodeId,
        token_after: BreakToken,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Reported {
        node: NodeId,
        cursor: FloatCursor,
        token: BreakToken,
        position: f32,
    },
    Placed {
        node: NodeId,
        rect: Rect,
    },
    Deferred {
        node: NodeId,
    },
    Withdrawn {
        node: NodeId,
    },
    Accepted,
    HeightRejected,
    BandRetry {
        height: f32,
    },
    PositionRetry {
        block: f32,
    },
}
#[derive(Debug)]
pub struct Trial {
    pub outcome: Outcome,
    pub state: Checkpoint,
    pub events: Vec<Event>,
    pub calls: usize,
    pub attempts: Vec<Checkpoint>,
}
pub struct Driver {
    specs: Vec<FloatSpec>,
}
impl Driver {
    pub fn new(specs: Vec<FloatSpec>) -> Result<Self, FlowError> {
        for (i, s) in specs.iter().enumerate() {
            if !nonnegative(s.width) || !nonnegative(s.height) {
                return Err(FlowError::InvalidGeometry);
            }
            if specs[..i].iter().any(|v| v.node == s.node) {
                return Err(FlowError::DuplicateFloat);
            }
        }
        Ok(Self { specs })
    }
    fn spec(&self, node: NodeId) -> Result<FloatSpec, FlowError> {
        self.specs
            .iter()
            .find(|s| s.node == node)
            .copied()
            .ok_or(FlowError::MissingFloat)
    }
    pub fn trial(
        &self,
        p: &Paragraph,
        cx: &mut LayoutContext,
        start: &Checkpoint,
        options: &LineOptions,
        atomics: &AtomicSizes,
        max_height: Option<f32>,
    ) -> Result<Trial, FlowError> {
        if start.paragraph != p.id() {
            return Err(FlowError::InvalidCheckpoint);
        }
        if max_height.is_some_and(|h| !nonnegative(h)) {
            return Err(FlowError::InvalidGeometry);
        }
        let mut state = start.clone();
        let mut events = Vec::new();
        let mut attempts = Vec::new();
        let mut calls = 0;
        let mut band: f32 = 0.0;
        // Band growth consumes distinct line metrics; position retries consume
        // strictly increasing float bottoms. Float retries have their own bound.
        let limit = (self.specs.len().saturating_mul(3).saturating_add(1)).saturating_mul(
            p.text()
                .len()
                .saturating_add(self.specs.len())
                .saturating_add(2),
        );
        loop {
            calls += 1;
            if calls > limit {
                return Err(FlowError::RetryLimit);
            }
            attempts.push(state.clone());
            let (inset, width) = state.slot(band);
            let mut c = LineConstraint::new(width);
            c.inline_start_offset = inset;
            c.block_offset = state.block;
            c.max_block_size = max_height.map(|h| (h - (state.block - start.block)).max(0.0));
            c.floats_placed_through = state.cursor;
            match p.next_line(cx, start.token, options, &c, atomics) {
                LineResult::FloatEncountered {
                    node,
                    line_start,
                    inline_position,
                    float_cursor,
                } => {
                    if line_start != start.token {
                        return Err(FlowError::InvalidToken);
                    }
                    let report = Report {
                        node,
                        cursor: float_cursor,
                    };
                    let spec = self.spec(node)?;
                    events.push(Event::Reported {
                        node,
                        cursor: float_cursor,
                        token: line_start,
                        position: inline_position,
                    });
                    if !state.pending.is_empty()
                        || state.withdrawn.contains(&report)
                        || spec.width > width - inline_position
                    {
                        state.pending.push(report);
                        events.push(Event::Deferred { node });
                    } else {
                        let rect = state.place(spec, report, state.block)?;
                        events.push(Event::Placed { node, rect });
                    }
                    state.cursor = Some(float_cursor);
                }
                LineResult::Line(line) => {
                    if let Some(&(node, cursor)) = line.displaced_floats().last() {
                        state.withdraw(Report { node, cursor })?;
                        events.push(Event::Withdrawn { node });
                        continue;
                    }
                    if line.block_size() > band && state.slot(line.block_size()) != (inset, width) {
                        band = line.block_size();
                        events.push(Event::BandRetry { height: band });
                        continue;
                    }
                    band = band.max(line.block_size());
                    if line.inline_size() > width
                        && let Some(y) = state.next_bottom(band)
                    {
                        state.block = y;
                        events.push(Event::PositionRetry { block: y });
                        continue;
                    }
                    state.token = line.break_token();
                    state.block += line.block_size();
                    if !nonnegative(state.block) {
                        return Err(FlowError::InvalidGeometry);
                    }
                    for r in std::mem::take(&mut state.pending) {
                        let rect = state.place(self.spec(r.node)?, r, state.block)?;
                        events.push(Event::Placed { node: r.node, rect });
                    }
                    state.withdrawn.clear();
                    events.push(Event::Accepted);
                    return Ok(Trial {
                        outcome: Outcome::Line(line),
                        state,
                        events,
                        calls,
                        attempts,
                    });
                }
                LineResult::BlockSizeExceeded { needed_block_size } => {
                    events.push(Event::HeightRejected);
                    return Ok(Trial {
                        outcome: Outcome::HeightRejected {
                            needed: needed_block_size,
                        },
                        state: start.clone(),
                        events,
                        calls,
                        attempts,
                    });
                }
                LineResult::Done => {
                    return Ok(Trial {
                        outcome: Outcome::Done,
                        state,
                        events,
                        calls,
                        attempts,
                    });
                }
                LineResult::BlockInInline { node, token_after } => {
                    return Ok(Trial {
                        outcome: Outcome::BlockBoundary { node, token_after },
                        state,
                        events,
                        calls,
                        attempts,
                    });
                }
                LineResult::InvalidToken => return Err(FlowError::InvalidToken),
                _ => return Err(FlowError::UnknownResult),
            }
        }
    }
    /// Commit independently measured block layout at a reported boundary.
    pub fn commit_block(
        &self,
        p: &Paragraph,
        cx: &mut LayoutContext,
        start: &Checkpoint,
        options: &LineOptions,
        atomics: &AtomicSizes,
        height: f32,
    ) -> Result<Checkpoint, FlowError> {
        if !nonnegative(height) {
            return Err(FlowError::InvalidGeometry);
        }
        let t = self.trial(p, cx, start, options, atomics, None)?;
        let Outcome::BlockBoundary { token_after, .. } = t.outcome else {
            return Err(FlowError::InvalidCheckpoint);
        };
        let mut s = t.state;
        for r in std::mem::take(&mut s.pending) {
            s.place(self.spec(r.node)?, r, s.block)?;
        }
        s.token = token_after;
        s.block += height;
        if !nonnegative(s.block) {
            return Err(FlowError::InvalidGeometry);
        }
        s.withdrawn.clear();
        Ok(s)
    }
    pub fn preview(
        &self,
        p: &Paragraph,
        cx: &mut LayoutContext,
        start: &Checkpoint,
        options: &LineOptions,
        atomics: &AtomicSizes,
        limit: PreviewLimit,
    ) -> Result<Preview, FlowError> {
        if start.paragraph != p.id() {
            return Err(FlowError::InvalidCheckpoint);
        }
        if limit.height.is_some_and(|h| !nonnegative(h)) {
            return Err(FlowError::InvalidGeometry);
        }
        let mut preview = Preview {
            lines: Vec::new(),
            checkpoints: vec![start.clone()],
            events: Vec::new(),
            stop: None,
        };
        for _ in 0..limit.lines {
            let current = preview
                .checkpoints
                .last()
                .ok_or(FlowError::InvalidCheckpoint)?;
            let remaining = limit
                .height
                .map(|h| (h - (current.block - start.block)).max(0.0));
            let t = self.trial(p, cx, current, options, atomics, remaining)?;
            preview.events.extend(t.events);
            match t.outcome {
                Outcome::Line(line) => {
                    preview.lines.push(line);
                    preview.checkpoints.push(t.state);
                }
                Outcome::Done => {
                    preview.stop = Some(Stop::Done);
                    break;
                }
                Outcome::HeightRejected { needed } => {
                    preview.stop = Some(Stop::Height { needed });
                    break;
                }
                Outcome::BlockBoundary { node, token_after } => {
                    preview.stop = Some(Stop::Block { node, token_after });
                    break;
                }
            }
        }
        Ok(preview)
    }
}

#[derive(Debug)]
pub enum Stop {
    Done,
    Height {
        needed: f32,
    },
    Block {
        node: NodeId,
        token_after: BreakToken,
    },
}
pub struct Preview {
    pub lines: Vec<Line>,
    checkpoints: Vec<Checkpoint>,
    pub events: Vec<Event>,
    pub stop: Option<Stop>,
}
impl Preview {
    pub fn select(&self, accepted: usize) -> Result<Checkpoint, FlowError> {
        self.checkpoints
            .get(accepted)
            .cloned()
            .ok_or(FlowError::InvalidCheckpoint)
    }
}
/// `total` must come from actual lookahead, not an estimated line count.
/// A fresh page may relax impossible limits so that at least one line advances.
pub fn widow_prefix(
    capacity: usize,
    total: usize,
    orphans: usize,
    widows: usize,
    fresh_page: bool,
) -> Result<usize, FlowError> {
    if orphans == 0 || widows == 0 {
        return Err(FlowError::InvalidGeometry);
    }
    let capacity = capacity.min(total);
    if capacity == total {
        return Ok(total);
    }
    let mut keep = capacity;
    if total - keep < widows {
        keep = total.saturating_sub(widows).min(keep);
    }
    if keep < orphans {
        keep = 0;
    }
    if keep == 0 && fresh_page {
        keep = capacity;
    }
    Ok(keep)
}

#[derive(Clone, Copy, Debug)]
pub struct PreviewLimit {
    pub lines: usize,
    pub height: Option<f32>,
}
