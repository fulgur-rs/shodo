//! Aggregate retained resources across normal, first-line and nested paragraphs.
use crate::limits::{LimitExceeded, LimitKind, Limits};
use crate::paragraph::ParagraphData;

#[derive(Clone, Copy, Default)]
pub(super) struct Cost {
    text: u64,
    items: u64,
    styles: u64,
    glyphs: u64,
}

impl Cost {
    pub(super) fn get(self, kind: LimitKind) -> u64 {
        match kind {
            LimitKind::TextBytes => self.text,
            LimitKind::Items => self.items,
            LimitKind::Styles => self.styles,
            LimitKind::ShapedGlyphs => self.glyphs,
            _ => 0,
        }
    }
    pub(super) fn add(&mut self, kind: LimitKind, amount: u64) {
        let value = match kind {
            LimitKind::TextBytes => &mut self.text,
            LimitKind::Items => &mut self.items,
            LimitKind::Styles => &mut self.styles,
            LimitKind::ShapedGlyphs => &mut self.glyphs,
            _ => return,
        };
        *value = value.saturating_add(amount);
    }
}

struct Scope {
    limits: Limits,
    start: Cost,
    initial: Cost,
}

impl Scope {
    fn used(&self, spent: Cost, kind: LimitKind) -> u64 {
        spent
            .get(kind)
            .saturating_sub(self.start.get(kind))
            .saturating_add(self.initial.get(kind))
    }
}

/// Each nested input keeps its own limits while sharing every ancestor's cap.
pub(crate) struct RubyBudget {
    enabled: bool,
    spent: Cost,
    scopes: Vec<Scope>,
}

pub(super) fn limit(limits: &Limits, kind: LimitKind) -> Option<u64> {
    match kind {
        LimitKind::TextBytes => limits.max_text_bytes,
        LimitKind::Items => limits.max_items,
        LimitKind::Styles => limits.max_styles,
        LimitKind::ShapedGlyphs => limits.max_shaped_glyphs,
        _ => None,
    }
}

impl RubyBudget {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            spent: Cost::default(),
            scopes: Vec::new(),
        }
    }
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }
    pub(crate) fn enter(&mut self, limits: &Limits) {
        if self.enabled {
            self.scopes.push(Scope {
                limits: limits.clone(),
                start: self.spent,
                initial: Cost::default(),
            });
        }
    }
    pub(crate) fn leave(&mut self) {
        if self.enabled {
            self.scopes.pop();
        }
    }

    pub(super) fn resume(&mut self, limits: &Limits, initial: Cost) {
        debug_assert!(self.enabled);
        self.scopes.push(Scope {
            limits: limits.clone(),
            start: self.spent,
            initial,
        });
    }
    pub(super) fn finish_scope(&mut self) -> Cost {
        let scope = self.scopes.pop().expect("resumed base scope");
        let mut used = Cost::default();
        for kind in [
            LimitKind::TextBytes,
            LimitKind::Items,
            LimitKind::Styles,
            LimitKind::ShapedGlyphs,
        ] {
            used.add(kind, scope.used(self.spent, kind));
        }
        used
    }

    pub(crate) fn remaining(&self, original: &Limits) -> Limits {
        let mut remaining = original.clone();
        for (kind, value) in [
            (LimitKind::TextBytes, &mut remaining.max_text_bytes),
            (LimitKind::Items, &mut remaining.max_items),
            (LimitKind::Styles, &mut remaining.max_styles),
            (LimitKind::ShapedGlyphs, &mut remaining.max_shaped_glyphs),
        ] {
            for scope in &self.scopes {
                if let Some(cap) = limit(&scope.limits, kind) {
                    let available = cap.saturating_sub(scope.used(self.spent, kind));
                    *value = Some(value.map_or(available, |current| current.min(available)));
                }
            }
        }
        remaining
    }

    pub(crate) fn translate(&self, mut error: LimitExceeded) -> LimitExceeded {
        for scope in self.scopes.iter().rev() {
            if let Some(cap) = limit(&scope.limits, error.kind) {
                let used = scope.used(self.spent, error.kind);
                if cap.saturating_sub(used) == error.limit {
                    error.limit = cap;
                    error.actual = error.actual.saturating_add(used);
                    break;
                }
            }
        }
        error
    }

    pub(crate) fn check(&self, kind: LimitKind, amount: u64) -> Result<(), LimitExceeded> {
        for scope in self.scopes.iter().rev() {
            let used = scope.used(self.spent, kind);
            Limits::check(
                limit(&scope.limits, kind),
                kind,
                used.saturating_add(amount),
            )?;
        }
        Ok(())
    }
    pub(crate) fn charge(&mut self, kind: LimitKind, amount: u64) -> Result<(), LimitExceeded> {
        if !self.enabled {
            return Ok(());
        }
        self.check(kind, amount)?;
        self.spent.add(kind, amount);
        Ok(())
    }
    pub(crate) fn paragraph(&mut self, data: &ParagraphData) -> Result<(), LimitExceeded> {
        for (kind, amount) in [
            (LimitKind::TextBytes, data.text.len() as u64),
            (LimitKind::Items, data.items.len() as u64),
            (LimitKind::Styles, data.styles.len() as u64),
            (LimitKind::ShapedGlyphs, data.glyphs.len() as u64),
        ] {
            self.charge(kind, amount)?;
        }
        Ok(())
    }
}
