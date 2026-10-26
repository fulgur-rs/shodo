//! Resource limits (fail-closed) and warnings.

pub(crate) mod ledger;

use std::fmt;

use crate::geometry::Saturation;

/// Upper bounds on resources consumed by input. `None` disables a limit.
/// Some limits reject the operation; layout-work limits may warn and use a
/// documented fallback. See each field for its behavior.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Raw and processed UTF-8 bytes at build time (16 MiB). Independent of
    /// the glyph budget: ASCII may reach `max_shaped_glyphs` first.
    pub max_text_bytes: Option<u64>,
    /// Transform and source-offset mapping data used by normal, first-line and
    /// nested paragraphs (64 MiB). Includes data later discarded, but not
    /// allocator overhead or unused capacity.
    pub max_mapping_bytes: Option<u64>,
    /// Analysis data and scratch space reserved for normal, first-line and
    /// nested paragraphs (128 MiB). Reservations add up even after scratch is
    /// discarded or copied for replay. Does not include allocator overhead,
    /// unused capacity or bounded temporary data. This is not a process memory
    /// limit.
    pub max_analysis_bytes: Option<u64>,
    /// Input items and shaping runs kept for layout (2^20). Also limits
    /// the structure of each ruby annotation before validation. Paragraph
    /// construction applies the limit across imported ruby annotations.
    pub max_items: Option<u64>,
    /// Interned inline styles per builder (2^16).
    pub max_styles: Option<u64>,
    /// Approximate size of style data and builder lookup keys (64 MiB).
    /// Includes normal and first-line styles and each retained ruby input.
    /// Counts copied values, but not allocator overhead, unused capacity or
    /// short-lived copies.
    pub max_style_bytes: Option<u64>,
    /// Cumulative style-key work per builder (256 Mi units). Every identity
    /// comparison and hash reserves one unit plus the incoming normal and
    /// first-line styles' variable payload bytes, including array slots.
    /// Family slots are reserved before inspecting their name lengths.
    /// Construction also reserves the root/first-line payload before sizing
    /// owned copies, even though only the root enters the initial hash.
    /// This bounds interning work, independently of retained style bytes;
    /// it is not a CPU instruction count or a budget for all ruby processing.
    /// Restoring a ruby snapshot charges the new builder's actual key work,
    /// not work already performed to create the immutable snapshot.
    pub max_style_work: Option<u64>,
    /// Simultaneously open inline boxes at build time (512).
    pub max_nesting_depth: Option<u64>,
    /// Glyphs per paragraph build and reshape output (2^22).
    pub max_shaped_glyphs: Option<u64>,
    /// Work budget for matching ruby readings to base text (2^23). Shared
    /// by normal, first-line and nested ruby. Work is reserved for both the
    /// counting and building passes, including candidates later rejected.
    /// These are work units, not CPU instruction counts; `None` disables
    /// this limit.
    pub max_ruby_cut_work: Option<u64>,
    /// Work budget for each `next_line` or `intrinsic_sizes` call. The
    /// allowance is the configured factor (default 16) times the sum of the
    /// fit-probe span and the widest ruby-container walk. For walks over
    /// 16,384 containers, only one distinct look-ahead end is allowed per
    /// start and atomic-size revision. Reaching the budget emits a warning
    /// and skips ruby adjustments for the rest of the call. Line layout
    /// continues, and accepted lines still place their ruby annotations
    /// normally. A value of 0 skips adjustments; `None` disables this limit.
    pub max_ruby_line_work: Option<u64>,
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
    /// Logical CSS descriptor payload and coverage-index allowances per face
    /// (64 KiB), checked before validation, copying and source attempts.
    pub max_font_descriptor_bytes: Option<u64>,
    /// Total admitted CSS descriptor payload/index allowances per font layer
    /// (8 MiB). Shared application and document layers have separate budgets.
    pub max_layer_descriptor_bytes: Option<u64>,
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
    /// Retained faces in a font layer (256), including the shared built-in
    /// stub and lazily materialized platform faces.
    pub max_faces_per_layer: Option<u64>,
    /// Total font-blob bytes retained per layer (256 MiB). Shared blob storage
    /// is charged once, including platform fonts and CSS `local()` sources.
    pub max_layer_blob_bytes: Option<u64>,
    /// Shared HarfRust Font cache entries per layer (64).
    pub max_shaper_cache_entries: Option<u64>,
    /// Declared sources per CSS font registration (64), checked before attempts.
    pub max_font_sources: Option<u64>,
    /// Total UTF-8 bytes of declared local source names per registration (64 KiB).
    pub max_font_source_name_bytes: Option<u64>,
    /// Approximate memory for indexing installed font names (8 MiB),
    /// including index metadata and temporary construction data. Document
    /// collections use their root collection's limit. Checking a selected
    /// system-font file during registration uses the caller's limit for
    /// decoding its name.
    pub max_local_font_index_bytes: Option<u64>,
    /// Work budget for indexing installed font names (2^24), including
    /// family names, faces, source loads and name records. Also limits checks
    /// of a selected system font during each source registration. Indexing
    /// uses the root collection's limit; checks use the caller's.
    pub max_local_font_lookup_work: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        const MIB: u64 = 1024 * 1024;
        Self {
            max_text_bytes: Some(16 * MIB),
            max_mapping_bytes: Some(64 * MIB),
            max_analysis_bytes: Some(128 * MIB),
            max_items: Some(1 << 20),
            max_styles: Some(1 << 16),
            max_style_bytes: Some(64 * MIB),
            max_style_work: Some(256 * MIB),
            max_nesting_depth: Some(512),
            max_shaped_glyphs: Some(1 << 22),
            max_ruby_cut_work: Some(1 << 23),
            max_ruby_line_work: Some(16),
            max_reshape_window_bytes: Some(4096),
            max_shaping_run_bytes: Some(64 * 1024),
            max_balance_iterations: Some(16),
            max_pretty_window_lines: Some(4),
            max_warnings: Some(1024),
            max_font_blob_bytes: Some(32 * MIB),
            max_font_descriptor_bytes: Some(64 * 1024),
            max_layer_descriptor_bytes: Some(8 * MIB),
            max_ttc_faces: Some(64),
            max_font_axes: Some(64),
            max_layout_lookups: Some(4096),
            max_layout_subtables: Some(65_536),
            max_font_cache_items: Some(1 << 23),
            max_faces_per_layer: Some(256),
            max_layer_blob_bytes: Some(256 * MIB),
            max_shaper_cache_entries: Some(64),
            max_font_sources: Some(64),
            max_font_source_name_bytes: Some(64 * 1024),
            max_local_font_index_bytes: Some(8 * MIB),
            max_local_font_lookup_work: Some(1 << 24),
        }
    }
}

impl Limits {
    /// Every limit disabled. Only for trusted input.
    pub fn unlimited() -> Self {
        Self {
            max_text_bytes: None,
            max_mapping_bytes: None,
            max_analysis_bytes: None,
            max_items: None,
            max_styles: None,
            max_style_bytes: None,
            max_style_work: None,
            max_nesting_depth: None,
            max_shaped_glyphs: None,
            max_ruby_cut_work: None,
            max_ruby_line_work: None,
            max_reshape_window_bytes: None,
            max_shaping_run_bytes: None,
            max_balance_iterations: None,
            max_pretty_window_lines: None,
            max_warnings: None,
            max_font_blob_bytes: None,
            max_font_descriptor_bytes: None,
            max_layer_descriptor_bytes: None,
            max_ttc_faces: None,
            max_font_axes: None,
            max_layout_lookups: None,
            max_layout_subtables: None,
            max_font_cache_items: None,
            max_faces_per_layer: None,
            max_layer_blob_bytes: None,
            max_shaper_cache_entries: None,
            max_font_sources: None,
            max_font_source_name_bytes: None,
            max_local_font_index_bytes: None,
            max_local_font_lookup_work: None,
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
    MappingBytes,
    AnalysisBytes,
    TextBytes,
    Items,
    Styles,
    StyleBytes,
    StyleWork,
    NestingDepth,
    ShapedGlyphs,
    RubyCutWork,
    FontBlobBytes,
    FontDescriptorBytes,
    LayerDescriptorBytes,
    TtcFaces,
    FontAxes,
    LayoutLookups,
    LayoutSubtables,
    FontCacheItems,
    FacesPerLayer,
    LayerBlobBytes,
    FontSources,
    FontSourceNameBytes,
    LocalFontIndexBytes,
    LocalFontLookupWork,
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

/// Snapshot for append-only warning operations on the same sink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Checkpoint {
    len: usize,
    suppressed: bool,
}

impl Checkpoint {
    /// Dropped warnings cannot qualify an operation for caching.
    pub(crate) fn unchanged(self, sink: &WarningSink) -> bool {
        !self.suppressed && !sink.suppressed && self.len == sink.warnings.len()
    }
}

#[cfg(test)]
thread_local! {
    pub(crate) static BORROWED_WARNING_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static BORROWED_WARNING_CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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

    pub(crate) fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            len: self.warnings.len(),
            suppressed: self.suppressed,
        }
    }

    pub(crate) fn pristine(&self) -> bool {
        self.warnings.is_empty() && !self.suppressed
    }

    /// Undo appended warnings, including a newly added suppression marker.
    /// The checkpoint must belong to this sink, with no intervening `take`.
    pub(crate) fn rollback_to(&mut self, checkpoint: Checkpoint) {
        self.warnings.truncate(checkpoint.len);
        self.suppressed = checkpoint.suppressed;
    }

    pub(crate) fn push(&mut self, kind: WarningKind, message: impl Into<String>) {
        self.push_lazy(kind, || message.into());
    }

    /// Import snapshot records, cloning only messages admitted by this sink.
    /// A source `Suppressed` record is ordinary input; only this sink's own
    /// suppression state stops imports, preserving the previous push contract.
    pub(crate) fn extend_from_slice(&mut self, warnings: &[Warning]) {
        if self.suppressed {
            return;
        }
        for warning in warnings {
            #[cfg(test)]
            BORROWED_WARNING_VISITS.with(|count| count.set(count.get() + 1));
            self.push_lazy(warning.kind, || {
                #[cfg(test)]
                BORROWED_WARNING_CLONES.with(|count| count.set(count.get() + 1));
                warning.message.clone()
            });
            if self.suppressed {
                break;
            }
        }
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
    fn warning_checkpoint_tracks_clean_operations_and_pristine_sinks() {
        let mut sink = WarningSink::new(Some(1));
        let empty = sink.checkpoint();
        assert!(sink.pristine());
        assert!(empty.unchanged(&sink));
        sink.push(WarningKind::Unsupported, "first");
        assert!(!sink.pristine());
        assert!(!empty.unchanged(&sink));
        let full = sink.checkpoint();
        assert!(full.unchanged(&sink));
        sink.push(WarningKind::Unsupported, "suppressed");
        assert!(!full.unchanged(&sink));
        assert!(!sink.checkpoint().unchanged(&sink));
        assert!(!sink.pristine());
        sink.take();
        assert!(sink.pristine());
    }

    #[test]
    fn warning_rollback_restores_prefix_and_suppression_through_nested_trials() {
        for cap in [0, 1] {
            let mut sink = WarningSink::new(Some(cap));
            let empty = sink.checkpoint();
            if cap == 1 {
                sink.push(WarningKind::Unsupported, "prefix");
            }
            let prefix = sink.checkpoint();
            sink.push(WarningKind::NegativeInput, "rejected");
            let suppressed = sink.checkpoint();
            sink.push(WarningKind::NegativeInput, "dropped");
            sink.rollback_to(suppressed);
            assert!(sink.is_suppressed());
            assert_eq!(sink.as_slice().len(), cap as usize + 1);
            sink.rollback_to(prefix);
            assert!(!sink.is_suppressed());
            assert!(prefix.unchanged(&sink));
            assert_eq!(sink.as_slice().len(), cap as usize);
            if cap == 1 {
                assert_eq!(sink.as_slice()[0].message, "prefix");
            }
            sink.rollback_to(empty);
            assert!(sink.pristine());
            sink.push(WarningKind::MissingAtomicSize, "accepted");
            assert_eq!(
                sink.as_slice()[0].kind,
                if cap == 0 {
                    WarningKind::Suppressed
                } else {
                    WarningKind::MissingAtomicSize
                }
            );
        }
    }

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
        assert!(!sink.checkpoint().unchanged(&sink));
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
        assert!(!lazy.checkpoint().unchanged(&lazy));
        assert!(!eager.checkpoint().unchanged(&eager));
        assert_eq!(lazy.remaining_limit(), eager.remaining_limit());
    }
}

#[cfg(test)]
#[path = "limits/warning_import_tests.rs"]
mod warning_import_tests;
