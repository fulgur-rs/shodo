//! Imported bases stay in the parent stream, but keep occurrence-local caps.
use super::budget::{Cost, RubyBudget, limit};
use super::builder::{Boundary, RubyInput};
use crate::builder::RawItem;
use crate::limits::{LimitExceeded, LimitKind, Limits};

#[derive(Clone)]
struct BaseScope {
    limits: Limits,
    spent: Cost,
    parent: Option<usize>,
    styles: u64,
    style_indices: Vec<u32>,
    style_bytes: [u64; 2],
    transient_text: u64,
}

#[derive(Clone, Default)]
pub(crate) struct BaseScopes {
    scopes: Vec<BaseScope>,
    bases: Vec<Vec<Option<usize>>>,
    containers: Vec<Option<usize>>,
    items: Vec<Option<usize>>,
    stack: Vec<Option<usize>>,
    current: Option<usize>,
    failure: Option<LimitExceeded>,
    /// Reusable per-scope totals for `can_charge_shaped_glyphs`; only the
    /// entries listed in `preflight_touched` are non-default between calls.
    preflight: Vec<(bool, u64)>,
    preflight_touched: Vec<usize>,
}

/// Work done by one `can_charge_shaped_glyphs` call: charges inspected, the
/// deepest owning chain walked, and charge iterations plus scope visits.
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct PreflightOps {
    pub(crate) charges: usize,
    pub(crate) depth: usize,
    pub(crate) steps: usize,
}

impl PreflightOps {
    fn add_walk(&mut self, visited: usize) {
        self.steps += visited;
        self.depth = self.depth.max(visited);
    }

    #[cfg(test)]
    fn record(self) {
        if PREFLIGHT_OPS_ENABLED.with(std::cell::Cell::get) {
            PREFLIGHT_OPS.with(|log| log.borrow_mut().push(self));
        }
    }

    #[cfg(not(test))]
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

impl BaseScopes {
    pub(crate) fn new(inputs: &[RubyInput]) -> Self {
        if !inputs
            .iter()
            .any(|r| r.normalized.bases.iter().any(|b| b.limits.is_some()))
        {
            return Self::default();
        }
        let mut result = Self::default();
        for input in inputs {
            let mut bases = Vec::new();
            for base in &input.normalized.bases {
                bases.push(base.limits.as_ref().map(|limits| {
                    let index = result.scopes.len();
                    result.scopes.push(BaseScope {
                        limits: limits.clone(),
                        spent: Cost::default(),
                        parent: None,
                        styles: base.retained_styles,
                        style_indices: base.retained_style_indices.clone(),
                        style_bytes: [0; 2],
                        transient_text: 0,
                    });
                    index
                }));
            }
            result.bases.push(bases);
        }
        result.containers.resize(inputs.len(), None);
        result
    }

    pub(crate) fn enabled(&self) -> bool {
        !self.scopes.is_empty()
    }

    pub(crate) fn start_pass(&mut self, alternate: bool) -> Result<(), LimitExceeded> {
        self.items.clear();
        self.stack.clear();
        self.current = None;
        self.failure = None;
        for scope in &mut self.scopes {
            // Each dataset retains its own style array. Nested base styles
            // were already included in its containing snapshot's import.
            Limits::check(
                scope.limits.max_styles,
                LimitKind::Styles,
                scope
                    .spent
                    .get(LimitKind::Styles)
                    .saturating_add(scope.styles),
            )?;
            scope.spent.add(LimitKind::Styles, scope.styles);
            let bytes = scope.style_bytes[usize::from(alternate)];
            Limits::check(
                scope.limits.max_style_bytes,
                LimitKind::StyleBytes,
                scope.spent.get(LimitKind::StyleBytes).saturating_add(bytes),
            )?;
            scope.spent.add(LimitKind::StyleBytes, bytes);
            scope.transient_text = 0;
        }
        Ok(())
    }

    pub(crate) fn before_raw(&mut self, item: &RawItem) {
        if self.scopes.is_empty() {
            return;
        }
        if let RawItem::RubyBoundary { ruby, boundary, .. } = item {
            match boundary {
                Boundary::BaseClose(_) => {
                    self.current = self.stack.pop().expect("balanced imported base")
                }
                Boundary::ContainerOpen => self.containers[*ruby as usize] = self.current,
                _ => {}
            }
        }
    }
    pub(crate) fn after_raw(&mut self, item: &RawItem) {
        if self.scopes.is_empty() {
            return;
        }
        if let RawItem::RubyBoundary {
            ruby,
            boundary: Boundary::BaseOpen(column),
            ..
        } = item
        {
            self.stack.push(self.current);
            if let Some(scope) = self.bases[*ruby as usize][*column] {
                self.scopes[scope].parent = self.current;
                self.current = Some(scope);
            }
        }
    }

    pub(crate) fn check_style_sets(&self, alternate: bool) -> Result<(), LimitExceeded> {
        for scope in &self.scopes {
            Limits::check(
                scope.limits.max_styles,
                LimitKind::Styles,
                scope.styles.saturating_mul(if alternate { 2 } else { 1 }),
            )?;
        }
        Ok(())
    }
    pub(crate) fn check_style_bytes(
        &mut self,
        normal: &[crate::style::InlineStyle],
        alternate_sizes: Option<&[u64]>,
    ) -> Result<(), LimitExceeded> {
        for scope in &mut self.scopes {
            let mut bytes = [0u64; 2];
            for &i in &scope.style_indices {
                bytes[0] =
                    bytes[0].saturating_add(crate::style::memory::inline(&normal[i as usize]));
                if let Some(sizes) = alternate_sizes {
                    bytes[1] = bytes[1].saturating_add(sizes[i as usize]);
                }
            }
            Limits::check(
                scope.limits.max_style_bytes,
                LimitKind::StyleBytes,
                bytes[0].saturating_add(bytes[1]),
            )?;
            scope.style_bytes = bytes;
        }
        Ok(())
    }

    /// Add normal-pass shaping work to an alternate pass prepared in advance.
    pub(crate) fn merge_shaped_glyphs_from(&mut self, normal: &Self) -> Result<(), LimitExceeded> {
        debug_assert_eq!(self.scopes.len(), normal.scopes.len());
        for (alternate, normal) in self.scopes.iter().zip(&normal.scopes) {
            Limits::check(
                alternate.limits.max_shaped_glyphs,
                LimitKind::ShapedGlyphs,
                alternate
                    .spent
                    .get(LimitKind::ShapedGlyphs)
                    .saturating_add(normal.spent.get(LimitKind::ShapedGlyphs)),
            )?;
        }
        for (alternate, normal) in self.scopes.iter_mut().zip(&normal.scopes) {
            alternate.spent.add(
                LimitKind::ShapedGlyphs,
                normal.spent.get(LimitKind::ShapedGlyphs),
            );
        }
        Ok(())
    }

    pub(crate) fn check_current_item(&mut self) -> Result<(), LimitExceeded> {
        let mut cursor = self.current;
        while let Some(i) = cursor {
            let scope = &self.scopes[i];
            if let Err(error) = Limits::check(
                scope.limits.max_items,
                LimitKind::Items,
                scope.spent.get(LimitKind::Items).saturating_add(1),
            ) {
                self.failure = Some(error);
                return Err(error);
            }
            cursor = scope.parent;
        }
        Ok(())
    }

    pub(crate) fn record_item(&mut self) -> Result<(), LimitExceeded> {
        if self.scopes.is_empty() {
            return Ok(());
        }
        self.charge(self.current, LimitKind::Items, 1)?;
        self.items.push(self.current);
        Ok(())
    }
    pub(crate) fn transient_text(&mut self, bytes: u64) -> Result<(), LimitExceeded> {
        let mut cursor = self.current;
        while let Some(i) = cursor {
            let scope = &mut self.scopes[i];
            let amount = scope.transient_text.saturating_add(bytes);
            if let Err(error) =
                Limits::check(scope.limits.max_text_bytes, LimitKind::TextBytes, amount)
            {
                self.failure = Some(error);
                return Err(error);
            }
            scope.transient_text = amount;
            cursor = scope.parent;
        }
        Ok(())
    }
    pub(crate) fn shaping_run_bytes(&self, item: usize, parent: u64) -> u64 {
        let mut budget = parent;
        let mut cursor = self.items.get(item).copied().flatten();
        while let Some(i) = cursor {
            let scope = &self.scopes[i];
            budget = budget.min(scope.limits.max_shaping_run_bytes.unwrap_or(u64::MAX));
            cursor = scope.parent;
        }
        budget
    }

    pub(crate) fn item(
        &mut self,
        item: usize,
        kind: LimitKind,
        amount: u64,
    ) -> Result<(), LimitExceeded> {
        self.charge(self.items.get(item).copied().flatten(), kind, amount)
    }

    /// Whether charging `charges` (item index, glyph count) in order would
    /// stay within every owning scope chain, without touching `spent` or
    /// `failure`. Charges are non-negative and saturating addition is
    /// monotone and order-independent, so every prefix of the sequential
    /// `charge` replay fits iff each touched scope's `spent` plus the
    /// saturated sum of the charges reaching it fits. A run of charges to one
    /// owner is summed before its chain is walked once, so the check costs
    /// O(charges + depth) for a single owner and O(charges * depth) at worst.
    pub(crate) fn can_charge_shaped_glyphs(
        &mut self,
        charges: impl IntoIterator<Item = (usize, u64)>,
    ) -> bool {
        let mut charges = charges.into_iter();
        let mut ops = PreflightOps::default();
        let fits = 'check: {
            let Some((item, amount)) = charges.next() else {
                break 'check true;
            };
            ops.charges += 1;
            let first = self.owner(item);
            let mut total = amount;
            let mut next = None;
            for (item, amount) in charges.by_ref() {
                ops.charges += 1;
                let owner = self.owner(item);
                if owner != first {
                    next = Some((owner, amount));
                    break;
                }
                total = total.saturating_add(amount);
            }
            ops.steps += ops.charges;
            let Some((owner, amount)) = next else {
                // Single owner: one saturated total against one chain walk.
                let mut cursor = first;
                while let Some(i) = cursor {
                    ops.steps += 1;
                    ops.depth += 1;
                    let scope = &self.scopes[i];
                    if Self::exceeds_glyphs(scope, total) {
                        break 'check false;
                    }
                    cursor = scope.parent;
                }
                break 'check true;
            };
            if self.preflight.len() < self.scopes.len() {
                self.preflight.resize(self.scopes.len(), (false, 0));
            }
            ops.add_walk(self.accumulate_preflight(first, total));
            ops.add_walk(self.accumulate_preflight(owner, amount));
            for (item, amount) in charges {
                ops.charges += 1;
                ops.steps += 1;
                let owner = self.owner(item);
                ops.add_walk(self.accumulate_preflight(owner, amount));
            }
            // Check every touched total and reset all of them, so the scratch
            // is clean for the next call whatever the outcome.
            let mut fits = true;
            for i in self.preflight_touched.drain(..) {
                let (_, added) = std::mem::take(&mut self.preflight[i]);
                fits &= !Self::exceeds_glyphs(&self.scopes[i], added);
            }
            fits
        };
        ops.record();
        fits
    }

    fn owner(&self, item: usize) -> Option<usize> {
        self.items.get(item).copied().flatten()
    }

    fn exceeds_glyphs(scope: &BaseScope, added: u64) -> bool {
        Limits::check(
            limit(&scope.limits, LimitKind::ShapedGlyphs),
            LimitKind::ShapedGlyphs,
            scope
                .spent
                .get(LimitKind::ShapedGlyphs)
                .saturating_add(added),
        )
        .is_err()
    }

    /// Add `amount` to the preflight total of every scope on `owner`'s chain;
    /// returns the number of scopes visited.
    fn accumulate_preflight(&mut self, owner: Option<usize>, amount: u64) -> usize {
        let mut visited = 0;
        let mut cursor = owner;
        while let Some(i) = cursor {
            visited += 1;
            let (touched, added) = &mut self.preflight[i];
            if !*touched {
                *touched = true;
                self.preflight_touched.push(i);
            }
            *added = added.saturating_add(amount);
            cursor = self.scopes[i].parent;
        }
        visited
    }
    pub(crate) fn container(&mut self, container: usize, amount: u64) -> Result<(), LimitExceeded> {
        self.container_cost(container, LimitKind::Items, amount)
    }
    pub(crate) fn container_cost(
        &mut self,
        container: usize,
        kind: LimitKind,
        amount: u64,
    ) -> Result<(), LimitExceeded> {
        self.charge(
            self.containers.get(container).copied().flatten(),
            kind,
            amount,
        )
    }
    fn charge(
        &mut self,
        owner: Option<usize>,
        kind: LimitKind,
        amount: u64,
    ) -> Result<(), LimitExceeded> {
        let mut cursor = owner;
        while let Some(i) = cursor {
            let scope = &self.scopes[i];
            if let Err(error) = Limits::check(
                limit(&scope.limits, kind),
                kind,
                scope.spent.get(kind).saturating_add(amount),
            ) {
                self.failure = Some(error);
                return Err(error);
            }
            cursor = scope.parent;
        }
        let mut cursor = owner;
        while let Some(i) = cursor {
            let scope = &mut self.scopes[i];
            scope.spent.add(kind, amount);
            cursor = scope.parent;
        }
        Ok(())
    }
    pub(crate) fn translate(&self, error: LimitExceeded, budget: &RubyBudget) -> LimitExceeded {
        if self.failure == Some(error) {
            error
        } else {
            budget.translate(error)
        }
    }

    pub(crate) fn enter_container(&self, container: usize, budget: &mut RubyBudget) -> Vec<usize> {
        let mut chain = Vec::new();
        let mut cursor = self.containers.get(container).copied().flatten();
        while let Some(i) = cursor {
            chain.push(i);
            cursor = self.scopes[i].parent;
        }
        for &i in chain.iter().rev() {
            budget.resume(&self.scopes[i].limits, self.scopes[i].spent);
        }
        chain
    }
    pub(crate) fn leave_container(&mut self, chain: Vec<usize>, budget: &mut RubyBudget) {
        for i in chain {
            self.scopes[i].spent = budget.finish_scope();
        }
    }

    /// Charge only index cells whose real leaves share an owning base. Cells
    /// shared with unrelated containers remain solely in the parent budget.
    pub(crate) fn index(&mut self, containers: usize, leaves: usize) -> Result<(), LimitExceeded> {
        if self.scopes.is_empty() {
            return Ok(());
        }
        let owner = self.index_node(0, leaves, containers)?;
        self.charge(owner, LimitKind::Items, 1) // retained sentinel cell
    }
    fn index_node(
        &mut self,
        start: usize,
        end: usize,
        count: usize,
    ) -> Result<Option<usize>, LimitExceeded> {
        if start >= count {
            return Ok(None);
        }
        let owner = if end - start == 1 {
            self.containers[start]
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
        self.charge(owner, LimitKind::Items, 1)?;
        Ok(owner)
    }
    fn common_owner(&self, a: Option<usize>, b: Option<usize>) -> Option<usize> {
        let mut a = a?;
        loop {
            let mut cursor = b;
            while let Some(i) = cursor {
                if i == a {
                    return Some(a);
                }
                cursor = self.scopes[i].parent;
            }
            a = self.scopes[a].parent?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(limit: Option<u64>, parent: Option<usize>) -> BaseScope {
        BaseScope {
            limits: Limits {
                max_shaped_glyphs: limit,
                ..Limits::default()
            },
            spent: Cost::default(),
            parent,
            styles: 0,
            style_indices: Vec::new(),
            style_bytes: [0; 2],
            transient_text: 0,
        }
    }

    /// Item 0 and 1 belong to the inner scope 1, whose parent is scope 0.
    fn nested(outer: Option<u64>, inner: Option<u64>) -> BaseScopes {
        BaseScopes {
            scopes: vec![scope(outer, None), scope(inner, Some(0))],
            items: vec![Some(1), Some(1), None],
            ..BaseScopes::default()
        }
    }

    #[test]
    fn shaped_glyph_preflight_accumulates_shared_ancestor_charges() {
        let mut scopes = nested(Some(3), None);
        assert!(scopes.can_charge_shaped_glyphs([(0, 2), (2, 9)]));
        assert!(!scopes.can_charge_shaped_glyphs([(0, 2), (1, 2)]));
        let mut reference = scopes.clone();
        reference.item(0, LimitKind::ShapedGlyphs, 2).unwrap();
        let error = reference.item(1, LimitKind::ShapedGlyphs, 2).unwrap_err();
        assert_eq!(
            (error.kind, error.limit, error.actual),
            (LimitKind::ShapedGlyphs, 3, 4)
        );
    }

    #[test]
    fn shaped_glyph_preflight_checks_inner_and_spent_without_mutating() {
        let mut scopes = nested(None, Some(2));
        scopes.item(0, LimitKind::ShapedGlyphs, 1).unwrap();
        assert!(scopes.can_charge_shaped_glyphs([(1, 1)]));
        assert!(!scopes.can_charge_shaped_glyphs([(1, 1), (0, 1)]));
        assert_eq!(scopes.scopes[1].spent.get(LimitKind::ShapedGlyphs), 1);
        assert_eq!(scopes.scopes[0].spent.get(LimitKind::ShapedGlyphs), 1);
        assert_eq!(scopes.failure, None);
    }

    #[test]
    fn shaped_glyph_preflight_saturates_like_charge() {
        let mut scopes = nested(None, None);
        scopes.item(0, LimitKind::ShapedGlyphs, u64::MAX).unwrap();
        assert!(scopes.can_charge_shaped_glyphs([(0, 1), (1, u64::MAX)]));
    }

    /// Scopes 1 and 2 are siblings under scope 0; item 0 belongs to scope 1,
    /// item 1 to scope 2 and item 2 to no scope.
    fn siblings(outer: Option<u64>, left: Option<u64>, right: Option<u64>) -> BaseScopes {
        BaseScopes {
            scopes: vec![
                scope(outer, None),
                scope(left, Some(0)),
                scope(right, Some(0)),
            ],
            items: vec![Some(1), Some(2), None],
            ..BaseScopes::default()
        }
    }

    fn replay_fits(scopes: &BaseScopes, charges: &[(usize, u64)]) -> bool {
        let mut reference = scopes.clone();
        charges.iter().all(|&(item, amount)| {
            reference
                .item(item, LimitKind::ShapedGlyphs, amount)
                .is_ok()
        })
    }

    #[test]
    fn shaped_glyph_preflight_sums_sibling_owners_into_shared_ancestor() {
        let mut scopes = siblings(Some(5), Some(3), Some(3));
        for charges in [
            &[(0, 3), (1, 3)][..],
            &[(0, 3), (1, 2)],
            &[(0, 4), (1, 1)],
            &[(0, 2), (2, 100), (1, 2), (0, 1)],
            &[(0, 2), (1, 2), (0, 2)],
        ] {
            assert_eq!(
                scopes.can_charge_shaped_glyphs(charges.iter().copied()),
                replay_fits(&scopes, charges),
                "{charges:?}"
            );
        }
        // Each sibling stays under its own cap; only the ancestor total fails.
        assert!(!scopes.can_charge_shaped_glyphs([(0, 3), (1, 3)]));
        assert!(scopes.can_charge_shaped_glyphs([(0, 3), (1, 2)]));
    }

    #[test]
    fn shaped_glyph_preflight_resets_scratch_after_failure() {
        let mut scopes = siblings(Some(5), Some(3), Some(3));
        scopes.item(1, LimitKind::ShapedGlyphs, 1).unwrap();
        assert!(!scopes.can_charge_shaped_glyphs([(0, 3), (1, 2)]));
        // A dirty scratch would still carry the failed call's totals.
        assert!(scopes.can_charge_shaped_glyphs([(0, 2), (1, 2)]));
        assert!(scopes.can_charge_shaped_glyphs([(0, 2), (1, 2)]));
        assert!(scopes.preflight_touched.is_empty());
        assert!(scopes.preflight.iter().all(|&entry| entry == (false, 0)));
        assert_eq!(scopes.scopes[0].spent.get(LimitKind::ShapedGlyphs), 1);
        assert_eq!(scopes.scopes[1].spent.get(LimitKind::ShapedGlyphs), 0);
        assert_eq!(scopes.scopes[2].spent.get(LimitKind::ShapedGlyphs), 1);
        assert_eq!(scopes.failure, None);
    }

    #[test]
    fn shaped_glyph_preflight_saturates_against_finite_limits() {
        let limit = u64::MAX - 1;
        let mut scopes = siblings(Some(limit), None, None);
        scopes.item(0, LimitKind::ShapedGlyphs, limit - 1).unwrap();
        for (charges, fits) in [
            (&[(0, 1)][..], true),
            (&[(0, 1), (1, 1)], false),
            (&[(0, 1), (0, 1)], false),
            // Wrapping sums would land back under the limit here.
            (&[(0, u64::MAX), (1, 2)], false),
            (&[(0, u64::MAX), (0, 2)], false),
        ] {
            assert_eq!(
                scopes.can_charge_shaped_glyphs(charges.iter().copied()),
                fits,
                "{charges:?}"
            );
            assert_eq!(replay_fits(&scopes, charges), fits, "{charges:?}");
        }
        let mut fresh = siblings(Some(5), None, None);
        assert!(!fresh.can_charge_shaped_glyphs([(0, u64::MAX), (1, 2)]));
        assert!(!fresh.can_charge_shaped_glyphs([(0, u64::MAX), (0, 2)]));
        assert_eq!(fresh.scopes[0].spent.get(LimitKind::ShapedGlyphs), 0);
        assert_eq!(fresh.failure, None);
    }

    #[test]
    fn shaped_glyph_preflight_matches_sequential_replay_exhaustively() {
        let limits = [None, Some(0), Some(1), Some(2), Some(3), Some(5)];
        let amounts = [0, 1, 2, 3];
        for outer in limits {
            for left in limits {
                for right in limits {
                    let mut scopes = siblings(outer, left, right);
                    scopes.item(0, LimitKind::ShapedGlyphs, 0).unwrap();
                    for len in 1..=3usize {
                        let total = (3 * amounts.len()).pow(len as u32);
                        for code in 0..total {
                            let mut code = code;
                            let charges: Vec<(usize, u64)> = (0..len)
                                .map(|_| {
                                    let digit = code % (3 * amounts.len());
                                    code /= 3 * amounts.len();
                                    (digit % 3, amounts[digit / 3])
                                })
                                .collect();
                            assert_eq!(
                                scopes.can_charge_shaped_glyphs(charges.iter().copied()),
                                replay_fits(&scopes, &charges),
                                "{outer:?} {left:?} {right:?} {charges:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
