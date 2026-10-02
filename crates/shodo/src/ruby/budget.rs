//! Aggregate retained resources and work across normal, first-line and nested paragraphs.
use crate::limits::{LimitExceeded, LimitKind, Limits};

#[derive(Clone, Copy, Default)]
pub(super) struct Cost {
    text: u64,
    items: u64,
    styles: u64,
    style_bytes: u64,
    glyphs: u64,
    cut_work: u64,
}

impl Cost {
    pub(super) fn get(self, kind: LimitKind) -> u64 {
        match kind {
            LimitKind::TextBytes => self.text,
            LimitKind::Items => self.items,
            LimitKind::Styles => self.styles,
            LimitKind::StyleBytes => self.style_bytes,
            LimitKind::ShapedGlyphs => self.glyphs,
            LimitKind::RubyCutWork => self.cut_work,
            _ => 0,
        }
    }
    pub(super) fn add(&mut self, kind: LimitKind, amount: u64) {
        let value = match kind {
            LimitKind::TextBytes => &mut self.text,
            LimitKind::Items => &mut self.items,
            LimitKind::Styles => &mut self.styles,
            LimitKind::StyleBytes => &mut self.style_bytes,
            LimitKind::ShapedGlyphs => &mut self.glyphs,
            LimitKind::RubyCutWork => &mut self.cut_work,
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
        LimitKind::StyleBytes => limits.max_style_bytes,
        LimitKind::ShapedGlyphs => limits.max_shaped_glyphs,
        LimitKind::RubyCutWork => limits.max_ruby_cut_work,
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
            LimitKind::StyleBytes,
            LimitKind::ShapedGlyphs,
            LimitKind::RubyCutWork,
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
            (LimitKind::StyleBytes, &mut remaining.max_style_bytes),
            (LimitKind::ShapedGlyphs, &mut remaining.max_shaped_glyphs),
            (LimitKind::RubyCutWork, &mut remaining.max_ruby_cut_work),
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
    pub(crate) fn paragraph_analysis(
        &mut self,
        text_bytes: u64,
        items: u64,
        styles: u64,
        style_bytes: u64,
    ) -> Result<(), LimitExceeded> {
        for (kind, amount) in [
            (LimitKind::TextBytes, text_bytes),
            (LimitKind::Items, items),
            (LimitKind::Styles, styles),
            (LimitKind::StyleBytes, style_bytes),
        ] {
            self.charge(kind, amount)?;
        }
        Ok(())
    }

    pub(crate) fn paragraph_shaping(&mut self, glyphs: u64) -> Result<(), LimitExceeded> {
        self.charge(LimitKind::ShapedGlyphs, glyphs)
    }
}
