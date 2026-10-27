//! One cumulative ledger for paragraph, first-line and nested ruby producers.

use super::{LimitExceeded, LimitKind, Limits};
use crate::builder::RawItem;
use crate::ruby::builder::{Boundary, RubyInput};

pub(crate) type ScopeId = usize;
pub(crate) type FrameId = usize;

#[derive(Clone, Copy, Default)]
pub(crate) struct Cost {
    text: u64,
    items: u64,
    shape_items: u64,
    styles: u64,
    style_bytes: u64,
    mapping_bytes: u64,
    analysis_bytes: u64,
    decoration_work: u64,
    glyphs: u64,
    cut_work: u64,
}

impl Cost {
    pub(crate) fn get(self, kind: LimitKind) -> u64 {
        match kind {
            LimitKind::TextBytes => self.text,
            LimitKind::Items => self.items,
            LimitKind::Styles => self.styles,
            LimitKind::StyleBytes => self.style_bytes,
            LimitKind::MappingBytes => self.mapping_bytes,
            LimitKind::AnalysisBytes => self.analysis_bytes,
            LimitKind::DecorationWork => self.decoration_work,
            LimitKind::ShapedGlyphs => self.glyphs,
            LimitKind::RubyCutWork => self.cut_work,
            _ => 0,
        }
    }
    pub(crate) fn add(&mut self, kind: LimitKind, amount: u64) {
        let value = match kind {
            LimitKind::TextBytes => &mut self.text,
            LimitKind::Items => &mut self.items,
            LimitKind::Styles => &mut self.styles,
            LimitKind::StyleBytes => &mut self.style_bytes,
            LimitKind::MappingBytes => &mut self.mapping_bytes,
            LimitKind::AnalysisBytes => &mut self.analysis_bytes,
            LimitKind::DecorationWork => &mut self.decoration_work,
            LimitKind::ShapedGlyphs => &mut self.glyphs,
            LimitKind::RubyCutWork => &mut self.cut_work,
            _ => return,
        };
        *value = value.saturating_add(amount);
    }

    pub(crate) fn add_shape_items(&mut self, amount: u64) {
        self.shape_items = self.shape_items.saturating_add(amount);
    }

    pub(crate) fn shape_items(self) -> u64 {
        self.shape_items
    }
}

pub(crate) fn limit(limits: &Limits, kind: LimitKind) -> Option<u64> {
    match kind {
        LimitKind::TextBytes => limits.max_text_bytes,
        LimitKind::Items => limits.max_items,
        LimitKind::Styles => limits.max_styles,
        LimitKind::StyleBytes => limits.max_style_bytes,
        LimitKind::MappingBytes => limits.max_mapping_bytes,
        LimitKind::AnalysisBytes => limits.max_analysis_bytes,
        LimitKind::DecorationWork => limits.max_decoration_work,
        LimitKind::ShapedGlyphs => limits.max_shaped_glyphs,
        LimitKind::RubyCutWork => limits.max_ruby_cut_work,
        _ => None,
    }
}

struct Scope {
    limits: Limits,
    spent: Cost,
    parent: Option<ScopeId>,
    transient_text: u64,
    snapshot: Option<usize>,
}

#[derive(Clone)]
struct Base {
    scope: ScopeId,
    styles: u64,
    indices: Vec<u32>,
    style_bytes: [u64; 2],
}

#[derive(Clone)]
struct Frame {
    paragraph: ScopeId,
    bases: Vec<Vec<Option<ScopeId>>>,
    local: Vec<Base>,
    containers: Vec<ScopeId>,
    items: Vec<(usize, ScopeId)>,
    item_count: usize,
    stack: Vec<ScopeId>,
    current: ScopeId,
    shaping_start: Vec<(ScopeId, u64)>,
    trial_peak: u64,
}

#[derive(Clone, Copy)]
enum Reach {
    Chain,
    Local,
    Direct,
}

#[derive(Clone, Copy)]
struct Charge {
    owner: ScopeId,
    kind: LimitKind,
    amount: u64,
    shape_items: bool,
    reach: Reach,
}

struct Snapshot {
    owner: ScopeId,
    previous: Cost,
    previous_snapshot: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Checkpoint {
    charges: usize,
    snapshots: usize,
}

/// Detached producer reservations from an unselected or reusable trial.
pub(crate) struct Reservations(Vec<Charge>);

pub(crate) struct Capture(Vec<(ScopeId, Cost)>);

/// A single owner-indexed admission ledger; paragraph frames contain no budgets.
pub(crate) struct LimitsLedger {
    scopes: Vec<Scope>,
    frames: Vec<Frame>,
    frame: FrameId,
    journal: Vec<Charge>,
    snapshots: Vec<Snapshot>,
    checkpoints: Vec<Checkpoint>,
    replay: bool,
    capture_omissions: crate::hashing::FastMap<ScopeId, Cost>,
}

/// Work done by one `can_charge_shaped_glyphs` call: charges inspected, the
/// deepest owning chain walked, and charge iterations plus scope visits.
#[derive(Clone, Copy, Debug, Default)]
#[cfg(test)]
pub(crate) struct PreflightOps {
    pub(crate) charges: usize,
    pub(crate) depth: usize,
    pub(crate) steps: usize,
}

#[cfg(not(test))]
#[derive(Default)]
struct PreflightOps {}

#[cfg(test)]
impl PreflightOps {
    fn charge(&mut self) {
        self.charges += 1;
        self.steps += 1;
    }

    fn scope(&mut self) {
        self.steps += 1;
        self.depth += 1;
    }

    fn record(self) {
        if PREFLIGHT_OPS_ENABLED.with(std::cell::Cell::get) {
            PREFLIGHT_OPS.with(|log| log.borrow_mut().push(self));
        }
    }
}

#[cfg(not(test))]
impl PreflightOps {
    fn charge(&mut self) {}
    fn scope(&mut self) {}
    fn record(self) {}
}

#[cfg(test)]
std::thread_local! {
    pub(crate) static PREFLIGHT_OPS: std::cell::RefCell<Vec<PreflightOps>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Tests that inspect `PREFLIGHT_OPS` enable recording explicitly.
    pub(crate) static PREFLIGHT_OPS_ENABLED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

impl LimitsLedger {
    pub(crate) fn new(limits: &Limits) -> Self {
        let mut result = Self {
            scopes: Vec::new(),
            frames: Vec::new(),
            frame: 0,
            journal: Vec::new(),
            snapshots: Vec::new(),
            checkpoints: Vec::new(),
            replay: true,
            capture_omissions: crate::hashing::FastMap::default(),
        };
        let root = result.add_scope(limits, None);
        result.add_frame(root, &[]);
        result
    }

    pub(crate) fn add_scope(&mut self, limits: &Limits, parent: Option<ScopeId>) -> ScopeId {
        let id = self.scopes.len();
        self.scopes.push(Scope {
            limits: limits.clone(),
            spent: Cost::default(),
            parent,
            transient_text: 0,
            snapshot: None,
        });
        id
    }

    fn add_frame(&mut self, paragraph: ScopeId, inputs: &[RubyInput]) -> FrameId {
        let mut bases = Vec::new();
        let mut local = Vec::new();
        for input in inputs {
            let mut columns = Vec::new();
            for base in &input.normalized.bases {
                columns.push(base.limits.as_ref().map(|limits| {
                    let scope = self.add_scope(limits, Some(paragraph));
                    local.push(Base {
                        scope,
                        styles: base.retained_styles,
                        indices: base.retained_style_indices.clone(),
                        style_bytes: [0; 2],
                    });
                    scope
                }));
            }
            bases.push(columns);
        }
        let frame = self.frames.len();
        self.frames.push(Frame {
            paragraph,
            bases,
            local,
            containers: vec![paragraph; inputs.len()],
            items: Vec::new(),
            item_count: 0,
            stack: Vec::new(),
            current: paragraph,
            shaping_start: Vec::new(),
            trial_peak: 0,
        });
        frame
    }

    pub(crate) fn register_bases(&mut self, inputs: &[RubyInput]) {
        let paragraph = self.frames[self.frame].paragraph;
        let new = self.add_frame(paragraph, inputs);
        let frame = self.frames.pop().expect("new frame");
        debug_assert_eq!(new, self.frames.len());
        self.frames[self.frame] = frame;
    }

    pub(crate) fn paragraph(
        &mut self,
        limits: &Limits,
        inputs: &[RubyInput],
        parent: ScopeId,
    ) -> FrameId {
        let paragraph = self.add_scope(limits, Some(parent));
        let id = self.add_frame(paragraph, inputs);
        self.frame = id;
        id
    }

    pub(crate) fn frame(&self) -> FrameId {
        self.frame
    }
    pub(crate) fn select_frame(&mut self, frame: FrameId) {
        self.frame = frame;
    }
    pub(crate) fn clone_item_owners(&mut self, normal: FrameId) {
        self.frames[self.frame].items = self.frames[normal].items.clone();
        self.frames[self.frame].item_count = self.frames[normal].item_count;
    }
    pub(crate) fn alternate_frame(&mut self) -> FrameId {
        let mut frame = self.frames[self.frame].clone();
        frame.items.clear();
        frame.item_count = 0;
        frame.stack.clear();
        frame.current = frame.paragraph;
        let id = self.frames.len();
        self.frames.push(frame);
        self.frame = id;
        id
    }

    #[cfg(test)]
    pub(crate) fn used(&self, scope: ScopeId, kind: LimitKind) -> u64 {
        self.scopes[scope].spent.get(kind)
    }

    pub(crate) fn check_scope(
        &self,
        owner: ScopeId,
        kind: LimitKind,
        amount: u64,
    ) -> Result<(), LimitExceeded> {
        self.check_charge(Charge {
            owner,
            kind,
            amount,
            shape_items: false,
            reach: Reach::Chain,
        })
    }

    fn check_charge(&self, charge: Charge) -> Result<(), LimitExceeded> {
        let mut cursor = Some(charge.owner);
        while let Some(i) = cursor {
            let scope = &self.scopes[i];
            let used = if charge.shape_items {
                scope.spent.shape_items()
            } else {
                scope.spent.get(charge.kind)
            };
            Limits::check(
                limit(&scope.limits, charge.kind),
                charge.kind,
                used.saturating_add(charge.amount),
            )?;
            cursor = self.next_owner(i, charge.reach);
        }
        Ok(())
    }

    fn next_owner(&self, owner: ScopeId, reach: Reach) -> Option<ScopeId> {
        match reach {
            Reach::Chain => self.scopes[owner].parent,
            Reach::Direct => None,
            Reach::Local => self.scopes[owner]
                .parent
                .filter(|&i| i != self.frames[self.frame].paragraph),
        }
    }

    fn apply(&mut self, charge: Charge) -> Result<(), LimitExceeded> {
        self.check_charge(charge)?;
        self.apply_unchecked(charge);
        Ok(())
    }

    fn apply_unchecked(&mut self, charge: Charge) {
        let checkpoint = self.checkpoints.last().copied();
        let mut cursor = Some(charge.owner);
        while let Some(i) = cursor {
            let scope = &mut self.scopes[i];
            if let Some(checkpoint) = checkpoint
                && scope
                    .snapshot
                    .is_none_or(|index| index < checkpoint.snapshots)
            {
                let index = self.snapshots.len();
                self.snapshots.push(Snapshot {
                    owner: i,
                    previous: scope.spent,
                    previous_snapshot: scope.snapshot,
                });
                scope.snapshot = Some(index);
            }
            if charge.shape_items {
                scope.spent.add_shape_items(charge.amount);
            } else {
                scope.spent.add(charge.kind, charge.amount);
            }
            if !self.replay {
                let omitted = self.capture_omissions.entry(i).or_default();
                if charge.shape_items {
                    omitted.add_shape_items(charge.amount);
                } else {
                    omitted.add(charge.kind, charge.amount);
                }
            }
            cursor = self.next_owner(i, charge.reach);
        }
        if checkpoint.is_some() {
            self.journal.push(charge);
        }
    }

    pub(crate) fn charge_scope(
        &mut self,
        owner: ScopeId,
        kind: LimitKind,
        amount: u64,
    ) -> Result<(), LimitExceeded> {
        self.apply(Charge {
            owner,
            kind,
            amount,
            shape_items: false,
            reach: Reach::Chain,
        })
    }
    pub(crate) fn check(&self, kind: LimitKind, amount: u64) -> Result<(), LimitExceeded> {
        self.check_scope(self.frames[self.frame].paragraph, kind, amount)
    }
    pub(crate) fn current(&mut self, kind: LimitKind, amount: u64) -> Result<(), LimitExceeded> {
        self.charge_scope(self.frames[self.frame].current, kind, amount)
    }
    pub(crate) fn charge(&mut self, kind: LimitKind, amount: u64) -> Result<(), LimitExceeded> {
        self.charge_scope(self.frames[self.frame].paragraph, kind, amount)
    }
    pub(crate) fn item(
        &mut self,
        item: usize,
        kind: LimitKind,
        amount: u64,
    ) -> Result<(), LimitExceeded> {
        self.charge_scope(self.item_owner(item), kind, amount)
    }
    fn item_owner(&self, item: usize) -> ScopeId {
        let frame = &self.frames[self.frame];
        if item >= frame.item_count {
            return frame.paragraph;
        }
        let through = frame.items.partition_point(|&(start, _)| start <= item);
        through
            .checked_sub(1)
            .map_or(frame.paragraph, |i| frame.items[i].1)
    }
    pub(crate) fn container_owner(&self, container: usize) -> ScopeId {
        self.frames[self.frame].containers[container]
    }
    pub(crate) fn container_cost(
        &mut self,
        container: usize,
        kind: LimitKind,
        amount: u64,
    ) -> Result<(), LimitExceeded> {
        self.charge_scope(self.container_owner(container), kind, amount)
    }
    pub(crate) fn container(&mut self, container: usize, amount: u64) -> Result<(), LimitExceeded> {
        self.container_cost(container, LimitKind::Items, amount)
    }
    pub(crate) fn shape_item(&mut self, item: usize) -> Result<(), LimitExceeded> {
        self.apply(Charge {
            owner: self.item_owner(item),
            kind: LimitKind::Items,
            amount: 1,
            shape_items: true,
            reach: Reach::Chain,
        })
    }
    pub(crate) fn check_current_item(&self) -> Result<(), LimitExceeded> {
        self.check_scope(self.frames[self.frame].current, LimitKind::Items, 1)
    }
    pub(crate) fn record_item(&mut self) -> Result<(), LimitExceeded> {
        let owner = self.frames[self.frame].current;
        self.charge_scope(owner, LimitKind::Items, 1)?;
        let frame = &mut self.frames[self.frame];
        let previous = frame
            .items
            .last()
            .map_or(frame.paragraph, |&(_, owner)| owner);
        if previous != owner {
            frame.items.push((frame.item_count, owner));
        }
        frame.item_count += 1;
        Ok(())
    }

    pub(crate) fn before_raw(&mut self, item: &RawItem) {
        if let RawItem::RubyBoundary { ruby, boundary, .. } = item {
            let frame = &mut self.frames[self.frame];
            match boundary {
                Boundary::BaseClose(_) => {
                    frame.current = frame.stack.pop().expect("balanced imported base")
                }
                Boundary::ContainerOpen => frame.containers[*ruby as usize] = frame.current,
                _ => {}
            }
        }
    }
    pub(crate) fn after_raw(&mut self, item: &RawItem) {
        if let RawItem::RubyBoundary {
            ruby,
            boundary: Boundary::BaseOpen(column),
            ..
        } = item
        {
            let frame = &mut self.frames[self.frame];
            frame.stack.push(frame.current);
            if let Some(scope) = frame.bases[*ruby as usize][*column] {
                self.scopes[scope].parent = Some(frame.current);
                frame.current = scope;
            }
        }
    }
    pub(crate) fn start_pass(&mut self, alternate: bool) -> Result<(), LimitExceeded> {
        let frame = &mut self.frames[self.frame];
        frame.items.clear();
        frame.item_count = 0;
        frame.stack.clear();
        frame.current = frame.paragraph;
        self.scopes[frame.paragraph].transient_text = 0;
        for base in &frame.local {
            let scope = &mut self.scopes[base.scope];
            scope.transient_text = 0;
            for (kind, amount) in [
                (LimitKind::Styles, base.styles),
                (
                    LimitKind::StyleBytes,
                    base.style_bytes[usize::from(alternate)],
                ),
            ] {
                Limits::check(
                    limit(&scope.limits, kind),
                    kind,
                    scope.spent.get(kind).saturating_add(amount),
                )?;
                scope.spent.add(kind, amount);
            }
        }
        Ok(())
    }
    pub(crate) fn check_style_sets(&self, alternate: bool) -> Result<(), LimitExceeded> {
        for base in &self.frames[self.frame].local {
            let scope = &self.scopes[base.scope];
            Limits::check(
                scope.limits.max_styles,
                LimitKind::Styles,
                base.styles.saturating_mul(if alternate { 2 } else { 1 }),
            )?;
        }
        Ok(())
    }
    pub(crate) fn check_style_bytes(
        &mut self,
        normal: &[crate::style::InlineStyle],
        alternate_sizes: Option<&[u64]>,
    ) -> Result<(), LimitExceeded> {
        for base in &mut self.frames[self.frame].local {
            let mut bytes = [0u64; 2];
            for &i in &base.indices {
                bytes[0] =
                    bytes[0].saturating_add(crate::style::memory::inline(&normal[i as usize]));
                if let Some(sizes) = alternate_sizes {
                    bytes[1] = bytes[1].saturating_add(sizes[i as usize]);
                }
            }
            Limits::check(
                self.scopes[base.scope].limits.max_style_bytes,
                LimitKind::StyleBytes,
                bytes[0].saturating_add(bytes[1]),
            )?;
            base.style_bytes = bytes;
        }
        Ok(())
    }
    pub(crate) fn transient_text(&mut self, bytes: u64) -> Result<(), LimitExceeded> {
        let paragraph = self.frames[self.frame].paragraph;
        let mut cursor = self.frames[self.frame].current;
        loop {
            let scope = &self.scopes[cursor];
            Limits::check(
                scope.limits.max_text_bytes,
                LimitKind::TextBytes,
                scope.transient_text.saturating_add(bytes),
            )?;
            if cursor == paragraph {
                break;
            }
            cursor = scope.parent.expect("local scope belongs to paragraph");
        }
        let mut cursor = self.frames[self.frame].current;
        loop {
            let scope = &mut self.scopes[cursor];
            scope.transient_text = scope.transient_text.saturating_add(bytes);
            if cursor == paragraph {
                break;
            }
            cursor = scope.parent.expect("local scope belongs to paragraph");
        }
        Ok(())
    }
    pub(crate) fn shaping_run_bytes(&self, item: usize, parent: u64) -> u64 {
        let mut budget = parent;
        let mut cursor = Some(self.item_owner(item));
        while let Some(i) = cursor {
            let scope = &self.scopes[i];
            budget = budget.min(scope.limits.max_shaping_run_bytes.unwrap_or(u64::MAX));
            // A run window is a local producer bound, unlike cumulative
            // retained resources. Annotation paragraphs keep their own cap.
            if i == self.frames[self.frame].paragraph {
                break;
            }
            cursor = scope.parent;
        }
        budget
    }
    pub(crate) fn begin_shaping(&mut self) {
        let mut start = Vec::new();
        let mut cursor = Some(self.frames[self.frame].paragraph);
        while let Some(i) = cursor {
            start.push((i, self.scopes[i].spent.get(LimitKind::ShapedGlyphs)));
            cursor = self.scopes[i].parent;
        }
        self.frames[self.frame].shaping_start = start;
        self.frames[self.frame].trial_peak = 0;
    }

    pub(crate) fn trials_fit(&self, frame: FrameId) -> bool {
        self.check(LimitKind::ShapedGlyphs, self.frames[frame].trial_peak)
            .is_ok()
    }

    pub(crate) fn shaped_glyphs(
        &mut self,
        item: usize,
        amount: u64,
        trial_total: Option<u64>,
    ) -> Result<(), LimitExceeded> {
        let charge = Charge {
            owner: self.item_owner(item),
            kind: LimitKind::ShapedGlyphs,
            amount,
            shape_items: false,
            reach: Reach::Chain,
        };
        if let Some(total) = trial_total {
            self.frames[self.frame].trial_peak = self.frames[self.frame].trial_peak.max(total);
        }
        // Include local caps and output committed since shaping began, and
        // preserve the ordinary owner-first error before the snapshot check.
        self.check_charge(charge)?;
        if let Some(total) = trial_total {
            for &(i, initial) in &self.frames[self.frame].shaping_start {
                let scope = &self.scopes[i];
                Limits::check(
                    scope.limits.max_shaped_glyphs,
                    LimitKind::ShapedGlyphs,
                    initial.saturating_add(total),
                )?;
            }
        }
        self.apply_unchecked(charge);
        Ok(())
    }

    pub(crate) fn mapping_record(&mut self, bytes: u64) -> Result<(), LimitExceeded> {
        self.current(LimitKind::MappingBytes, bytes)
    }

    /// Root/ancestor reservation was made by the whole-table producer; add the
    /// local inclusive part without reserving it twice at the paragraph owner.
    fn local(&mut self, owner: ScopeId, kind: LimitKind, bytes: u64) -> Result<(), LimitExceeded> {
        if owner == self.frames[self.frame].paragraph {
            return Ok(());
        }
        self.apply(Charge {
            owner,
            kind,
            amount: bytes,
            shape_items: false,
            reach: Reach::Local,
        })
    }
    pub(crate) fn analysis_raw(&mut self, bytes: u64) -> Result<(), LimitExceeded> {
        self.local(
            self.frames[self.frame].current,
            LimitKind::AnalysisBytes,
            bytes,
        )
    }
    pub(crate) fn analysis_item(&mut self, item: usize, bytes: u64) -> Result<(), LimitExceeded> {
        self.local(self.item_owner(item), LimitKind::AnalysisBytes, bytes)
    }
    pub(crate) fn analysis_styles(
        &mut self,
        styles: &[crate::style::InlineStyle],
        bytes: impl Fn(&crate::style::InlineStyle) -> u64,
    ) -> Result<(), LimitExceeded> {
        for index in 0..self.frames[self.frame].local.len() {
            let base = &self.frames[self.frame].local[index];
            let added = base.indices.iter().fold(0u64, |sum, &i| {
                sum.saturating_add(bytes(&styles[i as usize]))
            });
            self.apply(Charge {
                owner: base.scope,
                kind: LimitKind::AnalysisBytes,
                amount: added,
                shape_items: false,
                reach: Reach::Direct,
            })?;
        }
        Ok(())
    }

    /// Shared source-cut tables are admitted once and excluded from replay.
    pub(crate) fn record_replay(&mut self, record: bool) {
        self.replay = record;
    }

    pub(crate) fn capture(&mut self) -> Capture {
        self.capture_omissions.clear();
        let frame = &self.frames[self.frame];
        let mut costs = Vec::with_capacity(frame.local.len() + 1);
        let mut cursor = Some(frame.paragraph);
        while let Some(i) = cursor {
            costs.push((i, self.scopes[i].spent));
            cursor = self.scopes[i].parent;
        }
        costs.extend(
            frame
                .local
                .iter()
                .map(|base| (base.scope, self.scopes[base.scope].spent)),
        );
        Capture(costs)
    }

    pub(crate) fn captured(&mut self, capture: Capture) -> Reservations {
        let mut charges = Vec::new();
        for (owner, initial) in capture.0 {
            let scope = &self.scopes[owner];
            let omissions = self
                .capture_omissions
                .get(&owner)
                .copied()
                .unwrap_or_default();
            for kind in [
                LimitKind::TextBytes,
                LimitKind::Items,
                LimitKind::Styles,
                LimitKind::StyleBytes,
                LimitKind::MappingBytes,
                LimitKind::AnalysisBytes,
                LimitKind::ShapedGlyphs,
                LimitKind::RubyCutWork,
            ] {
                let amount = scope
                    .spent
                    .get(kind)
                    .saturating_sub(initial.get(kind))
                    .saturating_sub(omissions.get(kind));
                if amount > 0 {
                    charges.push(Charge {
                        owner,
                        kind,
                        amount,
                        shape_items: false,
                        reach: Reach::Direct,
                    });
                }
            }
            let amount = scope
                .spent
                .shape_items()
                .saturating_sub(initial.shape_items())
                .saturating_sub(omissions.shape_items());
            if amount > 0 {
                charges.push(Charge {
                    owner,
                    kind: LimitKind::Items,
                    amount,
                    shape_items: true,
                    reach: Reach::Direct,
                });
            }
        }
        self.capture_omissions.clear();
        Reservations(charges)
    }

    pub(crate) fn can_commit(&mut self, reservations: &Reservations) -> bool {
        if reservations
            .0
            .iter()
            .all(|charge| matches!(charge.reach, Reach::Direct))
        {
            return reservations
                .0
                .iter()
                .all(|&charge| self.check_charge(charge).is_ok());
        }
        let checkpoint = self.checkpoint();
        let fits = reservations
            .0
            .iter()
            .all(|&charge| self.apply(charge).is_ok());
        self.rollback(checkpoint);
        fits
    }

    pub(crate) fn checkpoint(&mut self) -> Checkpoint {
        let checkpoint = Checkpoint {
            charges: self.journal.len(),
            snapshots: self.snapshots.len(),
        };
        self.checkpoints.push(checkpoint);
        checkpoint
    }
    pub(crate) fn rollback(&mut self, checkpoint: Checkpoint) -> Reservations {
        let active = self.checkpoints.pop();
        debug_assert_eq!(active, Some(checkpoint));
        for snapshot in self.snapshots.drain(checkpoint.snapshots..).rev() {
            let scope = &mut self.scopes[snapshot.owner];
            scope.spent = snapshot.previous;
            scope.snapshot = snapshot.previous_snapshot;
        }
        Reservations(self.journal.drain(checkpoint.charges..).collect())
    }
    pub(crate) fn commit(&mut self, reservations: Reservations) -> Result<(), LimitExceeded> {
        if self.checkpoints.is_empty()
            && let Some(first) = reservations.0.first().copied()
            && first.kind == LimitKind::ShapedGlyphs
            && reservations.0.iter().all(|charge| {
                charge.owner == first.owner
                    && charge.kind == first.kind
                    && matches!(charge.reach, Reach::Chain)
            })
        {
            let mut ops = PreflightOps::default();
            let amount = reservations.0.iter().fold(0u64, |sum, charge| {
                ops.charge();
                sum.saturating_add(charge.amount)
            });
            let mut cursor = Some(first.owner);
            while let Some(i) = cursor {
                ops.scope();
                cursor = self.scopes[i].parent;
            }
            ops.record();
            let aggregate = Charge { amount, ..first };
            if self.check_charge(aggregate).is_ok() {
                self.apply_unchecked(aggregate);
                return Ok(());
            }
        }
        // On failure keep the original producer boundaries and owner-first
        // precedence; no glyph production is needed to recover the error.
        for charge in reservations.0 {
            self.apply(charge)?;
        }
        Ok(())
    }

    pub(crate) fn index(&mut self, containers: usize, leaves: usize) -> Result<(), LimitExceeded> {
        self.charge(LimitKind::Items, (leaves as u64).saturating_mul(2))?;
        let owner = self.index_node(0, leaves, containers)?;
        self.local(owner, LimitKind::Items, 1)
    }
    fn index_node(
        &mut self,
        start: usize,
        end: usize,
        count: usize,
    ) -> Result<ScopeId, LimitExceeded> {
        let paragraph = self.frames[self.frame].paragraph;
        if start >= count {
            return Ok(paragraph);
        }
        let owner = if end - start == 1 {
            self.container_owner(start)
        } else {
            let middle = start + (end - start) / 2;
            let left = self.index_node(start, middle, count)?;
            let right = self.index_node(middle, end, count)?;
            if middle >= count {
                left
            } else {
                self.common_owner(left, right)
            }
        };
        self.local(owner, LimitKind::Items, 1)?;
        Ok(owner)
    }
    fn common_owner(&self, mut a: ScopeId, b: ScopeId) -> ScopeId {
        loop {
            let mut cursor = Some(b);
            while let Some(i) = cursor {
                if i == a {
                    return a;
                }
                cursor = self.scopes[i].parent;
            }
            a = self.scopes[a].parent.expect("owners share root");
        }
    }
}

#[cfg(test)]
#[path = "ledger_tests.rs"]
mod tests;
