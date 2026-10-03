//! Resource limits (fail-closed) and warnings.

use std::fmt;

use crate::geometry::Saturation;

/// Upper bounds on resources consumed by untrusted input. `None` disables a
/// limit. Limits are checked before allocating; exceeding one is an error,
/// never a silent truncation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Raw and processed UTF-8 bytes at build time (16 MiB). Independent of
    /// the glyph budget: ASCII may reach `max_shaped_glyphs` first.
    pub max_text_bytes: Option<u64>,
    /// Raw and processed items at build time (2^20).
    pub max_items: Option<u64>,
    /// Interned inline styles per builder (2^16).
    pub max_styles: Option<u64>,
    /// Logical owned inline-style data and builder interning keys (64 MiB). Includes
    /// normal/first-line styles and every retained ruby input occurrence.
    /// Counts cloned payload lengths and style slots, excluding allocator
    /// overhead, spare capacity and bounded transient copies.
    pub max_style_bytes: Option<u64>,
    /// Simultaneously open inline boxes at build time (512).
    pub max_nesting_depth: Option<u64>,
    /// Glyphs per paragraph build and reshape output (2^22).
    pub max_shaped_glyphs: Option<u64>,
    /// Ruby cut-search work reserved before counting and building candidate
    /// correspondences (2^23). Shared by normal/first-line and nested ruby.
    /// Reserves twice (base cuts * (lanes + 1) + total lane cuts), including
    /// rejected candidates. These are work units, not CPU instructions;
    /// `None` disables this work limit.
    pub max_ruby_cut_work: Option<u64>,
    /// UTF-8 bytes per line-edge reshape window (4096); exceeding it warns
    /// and preserves shared glyphs rather than failing line layout.
    pub max_reshape_window_bytes: Option<u64>,
    /// UTF-8 bytes per shaping run (64 KiB); giant graphemes warn and
    /// split shaping work at scalar boundaries without creating layout breaks.
    pub max_shaping_run_bytes: Option<u64>,
    /// Binary-search iterations for balance plans (16). Zero uses greedy.
    pub max_balance_iterations: Option<u64>,
    /// Lines per Pretty optimization window (4). Zero uses greedy.
    pub max_pretty_window_lines: Option<u64>,
    /// Retained warnings per build or layout context (1024), plus one
    /// suppression marker. `take_warnings` clears the context's count.
    pub max_warnings: Option<u64>,
    /// Bytes per registered font blob (32 MiB), checked before retention.
    pub max_font_blob_bytes: Option<u64>,
    /// Faces in one registered font collection (64).
    pub max_ttc_faces: Option<u64>,
    /// Variation axes in a registered font's fvar table (64).
    pub max_font_axes: Option<u64>,
    /// Layout lookups in each registered GSUB/GPOS table (4096).
    pub max_layout_lookups: Option<u64>,
    /// Layout subtables in each registered GSUB/GPOS table (65536).
    pub max_layout_subtables: Option<u64>,
    /// Expanded coverage/class glyph visits, GDEF mark sets, and AAT cache
    /// items per face (2^23).
    pub max_font_cache_items: Option<u64>,
    /// Explicitly registered faces in a font layer (256), including the shared
    /// built-in stub. Lazily materialized platform faces are exempt.
    pub max_faces_per_layer: Option<u64>,
    /// Total registered font-blob bytes retained per layer (256 MiB).
    /// Native-only blobs are exempt; explicit registration of a native blob
    /// charges it once per layer, including CSS `local()` sources.
    pub max_layer_blob_bytes: Option<u64>,
    /// Shared harfrust ShaperData cache entries per layer (64).
    pub max_shaper_cache_entries: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        const MIB: u64 = 1024 * 1024;
        Self {
            max_text_bytes: Some(16 * MIB),
            max_items: Some(1 << 20),
            max_styles: Some(1 << 16),
            max_style_bytes: Some(64 * MIB),
            max_nesting_depth: Some(512),
            max_shaped_glyphs: Some(1 << 22),
            max_ruby_cut_work: Some(1 << 23),
            max_reshape_window_bytes: Some(4096),
            max_shaping_run_bytes: Some(64 * 1024),
            max_balance_iterations: Some(16),
            max_pretty_window_lines: Some(4),
            max_warnings: Some(1024),
            max_font_blob_bytes: Some(32 * MIB),
            max_ttc_faces: Some(64),
            max_font_axes: Some(64),
            max_layout_lookups: Some(4096),
            max_layout_subtables: Some(65_536),
            max_font_cache_items: Some(1 << 23),
            max_faces_per_layer: Some(256),
            max_layer_blob_bytes: Some(256 * MIB),
            max_shaper_cache_entries: Some(64),
        }
    }
}

impl Limits {
    /// Every limit disabled. Only for trusted input.
    pub fn unlimited() -> Self {
        Self {
            max_text_bytes: None,
            max_items: None,
            max_styles: None,
            max_style_bytes: None,
            max_nesting_depth: None,
            max_shaped_glyphs: None,
            max_ruby_cut_work: None,
            max_reshape_window_bytes: None,
            max_shaping_run_bytes: None,
            max_balance_iterations: None,
            max_pretty_window_lines: None,
            max_warnings: None,
            max_font_blob_bytes: None,
            max_ttc_faces: None,
            max_font_axes: None,
            max_layout_lookups: None,
            max_layout_subtables: None,
            max_font_cache_items: None,
            max_faces_per_layer: None,
            max_layer_blob_bytes: None,
            max_shaper_cache_entries: None,
        }
    }

    pub(crate) fn check(
        limit: Option<u64>,
        kind: LimitKind,
        actual: u64,
    ) -> Result<(), LimitExceeded> {
        match limit {
            Some(limit) if actual > limit => Err(LimitExceeded {
                kind,
                limit,
                actual,
            }),
            _ => Ok(()),
        }
    }
}

/// Which limit was exceeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LimitKind {
    TextBytes,
    Items,
    Styles,
    StyleBytes,
    NestingDepth,
    ShapedGlyphs,
    RubyCutWork,
    FontBlobBytes,
    TtcFaces,
    FontAxes,
    LayoutLookups,
    LayoutSubtables,
    FontCacheItems,
    FacesPerLayer,
    LayerBlobBytes,
}

/// A resource limit was exceeded; the operation allocated nothing further.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitExceeded {
    pub kind: LimitKind,
    pub limit: u64,
    pub actual: u64,
}

impl fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "limit exceeded: {:?} (limit {}, actual {})",
            self.kind, self.limit, self.actual
        )
    }
}

impl std::error::Error for LimitExceeded {}

/// Category of a non-fatal problem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WarningKind {
    NonFiniteInput,
    NegativeInput,
    Saturated,
    UnbalancedInline,
    MissingAtomicSize,
    Unsupported,
    /// Further warnings were dropped because `Limits::max_warnings` was reached.
    Suppressed,
}

/// A non-fatal problem: the output is still produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub kind: WarningKind,
    pub message: String,
}

/// Bounded warning collector. After `max` warnings it records a single
/// `Suppressed` marker and drops the rest, so hostile input cannot grow it.
#[derive(Clone, Debug, Default)]
pub(crate) struct WarningSink {
    warnings: Vec<Warning>,
    max: Option<u64>,
    suppressed: bool,
}

impl WarningSink {
    pub(crate) fn new(max: Option<u64>) -> Self {
        Self {
            warnings: Vec::new(),
            max,
            suppressed: false,
        }
    }

    pub(crate) fn set_max(&mut self, max: Option<u64>) {
        self.max = max;
    }

    pub(crate) fn is_suppressed(&self) -> bool {
        self.suppressed
    }

    pub(crate) fn remaining_limit(&self) -> Option<u64> {
        self.max
            .map(|max| max.saturating_sub(self.warnings.len() as u64))
    }

    /// A suppressed sink cannot distinguish a clean operation from dropped
    /// warnings, so its checkpoint must not qualify a result for caching.
    pub(crate) fn checkpoint(&self) -> Option<usize> {
        (!self.suppressed).then_some(self.warnings.len())
    }

    pub(crate) fn push(&mut self, kind: WarningKind, message: impl Into<String>) {
        if self.suppressed {
            return;
        }
        if let Some(max) = self.max
            && self.warnings.len() as u64 >= max
        {
            self.warnings.push(Warning {
                kind: WarningKind::Suppressed,
                message: "further warnings suppressed".into(),
            });
            self.suppressed = true;
            return;
        }
        self.warnings.push(Warning {
            kind,
            message: message.into(),
        });
    }

    /// Push a dynamically formatted message without building it when the sink
    /// already suppressed further warnings or only needs the suppression
    /// marker. `sanitize` uses this for per-value `format!` messages so
    /// hostile input with `max_warnings=0` or a small cap does not allocate
    /// a `String` per dropped warning.
    pub(crate) fn push_lazy(&mut self, kind: WarningKind, message: impl FnOnce() -> String) {
        if self.suppressed {
            return;
        }
        if let Some(max) = self.max
            && self.warnings.len() as u64 >= max
        {
            self.warnings.push(Warning {
                kind: WarningKind::Suppressed,
                message: "further warnings suppressed".into(),
            });
            self.suppressed = true;
            return;
        }
        self.warnings.push(Warning {
            kind,
            message: message(),
        });
    }

    pub(crate) fn record_saturation(&mut self, sat: &Saturation) {
        if sat.non_finite > 0 {
            self.push(
                WarningKind::NonFiniteInput,
                format!("{} non-finite values replaced with 0", sat.non_finite),
            );
        }
        if sat.saturated > 0 {
            self.push(
                WarningKind::Saturated,
                format!("{} values saturated", sat.saturated),
            );
        }
    }

    pub(crate) fn append(&mut self, other: Self) {
        let was_suppressed = other.suppressed;
        for warning in other.warnings {
            self.push(warning.kind, warning.message);
        }
        self.suppressed |= was_suppressed;
    }

    #[cfg(test)]
    pub(crate) fn as_slice(&self) -> &[Warning] {
        &self.warnings
    }

    pub(crate) fn take(&mut self) -> Vec<Warning> {
        self.suppressed = false;
        std::mem::take(&mut self.warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Saturation;

    #[test]
    fn check_reports_kind_limit_and_actual() {
        assert_eq!(Limits::check(Some(10), LimitKind::Items, 10), Ok(()));
        assert_eq!(
            Limits::check(Some(10), LimitKind::Items, 11),
            Err(LimitExceeded {
                kind: LimitKind::Items,
                limit: 10,
                actual: 11
            })
        );
        assert_eq!(Limits::check(None, LimitKind::Items, u64::MAX), Ok(()));
    }

    #[test]
    fn defaults_are_bounded_and_unlimited_is_not() {
        let d = Limits::default();
        assert!(d.max_text_bytes.is_some() && d.max_layout_subtables.is_some());
        assert_eq!(Limits::unlimited().max_text_bytes, None);
    }

    #[test]
    fn warning_sink_caps_and_adds_one_suppression_marker() {
        let mut sink = WarningSink::new(Some(2));
        for _ in 0..5 {
            sink.push(WarningKind::NegativeInput, "x");
        }
        let kinds: Vec<_> = sink.as_slice().iter().map(|w| w.kind).collect();
        assert_eq!(
            kinds,
            vec![
                WarningKind::NegativeInput,
                WarningKind::NegativeInput,
                WarningKind::Suppressed
            ]
        );
        // take() resets the sink for the next operation.
        assert_eq!(sink.take().len(), 3);
        sink.push(WarningKind::NegativeInput, "y");
        assert_eq!(sink.as_slice().len(), 1);
    }

    #[test]
    fn saturation_becomes_at_most_two_warnings() {
        let mut sink = WarningSink::new(None);
        sink.record_saturation(&Saturation {
            saturated: 3,
            non_finite: 2,
        });
        sink.record_saturation(&Saturation::default());
        let kinds: Vec<_> = sink.as_slice().iter().map(|w| w.kind).collect();
        assert_eq!(
            kinds,
            vec![WarningKind::NonFiniteInput, WarningKind::Saturated]
        );
    }

    #[test]
    fn limit_exceeded_displays() {
        let e = LimitExceeded {
            kind: LimitKind::TextBytes,
            limit: 1,
            actual: 2,
        };
        assert_eq!(
            e.to_string(),
            "limit exceeded: TextBytes (limit 1, actual 2)"
        );
    }

    #[test]
    fn push_lazy_skips_message_after_suppression() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = AtomicUsize::new(0);
        let mut sink = WarningSink::new(Some(0));
        for _ in 0..10 {
            sink.push_lazy(WarningKind::NonFiniteInput, || {
                calls.fetch_add(1, Ordering::Relaxed);
                "non-finite margin replaced with 0".to_string()
            });
        }
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(sink.as_slice().len(), 1);
        assert_eq!(sink.as_slice()[0].kind, WarningKind::Suppressed);
        assert!(sink.is_suppressed());
        assert_eq!(sink.checkpoint(), None);
    }

    #[test]
    fn push_lazy_matches_push_for_small_cap_order_and_checkpoint() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let lazy_calls = AtomicUsize::new(0);
        let mut lazy = WarningSink::new(Some(5));
        for i in 0..20 {
            lazy.push_lazy(WarningKind::NegativeInput, || {
                lazy_calls.fetch_add(1, Ordering::Relaxed);
                format!("negative value {i} replaced with 0")
            });
        }
        // Only the first `max` messages are built; the rest only add the
        // single suppression marker.
        assert_eq!(lazy_calls.load(Ordering::Relaxed), 5);
        let mut eager = WarningSink::new(Some(5));
        for i in 0..20 {
            eager.push(
                WarningKind::NegativeInput,
                format!("negative value {i} replaced with 0"),
            );
        }
        assert_eq!(lazy.as_slice(), eager.as_slice());
        assert!(lazy.is_suppressed() && eager.is_suppressed());
        assert_eq!(lazy.checkpoint(), None);
        assert_eq!(eager.checkpoint(), None);
        assert_eq!(lazy.remaining_limit(), eager.remaining_limit());
    }
}
