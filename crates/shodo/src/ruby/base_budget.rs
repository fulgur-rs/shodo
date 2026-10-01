//! Imported bases stay in the parent stream, but keep occurrence-local caps.
use super::budget::{Cost, RubyBudget, limit};
use super::builder::{Boundary, RubyInput};
use crate::builder::RawItem;
use crate::limits::{LimitExceeded, LimitKind, Limits};

struct BaseScope {
    limits: Limits,
    spent: Cost,
    parent: Option<usize>,
    styles: u64,
    style_indices: Vec<u32>,
    style_bytes: [u64; 2],
    transient_text: u64,
}

#[derive(Default)]
pub(crate) struct BaseScopes {
    scopes: Vec<BaseScope>,
    bases: Vec<Vec<Option<usize>>>,
    containers: Vec<Option<usize>>,
    items: Vec<Option<usize>>,
    stack: Vec<Option<usize>>,
    current: Option<usize>,
    failure: Option<LimitExceeded>,
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
    pub(crate) fn container(&mut self, container: usize, amount: u64) -> Result<(), LimitExceeded> {
        self.charge(
            self.containers.get(container).copied().flatten(),
            LimitKind::Items,
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
