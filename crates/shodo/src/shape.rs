//! Shaping into structure-of-arrays glyph storage.
//!
//! Harfrust shapes matched fonts; absent fonts use deterministic .notdef
//! advances. Cluster groups are kept in logical order, with intra-cluster
//! offsets normalized so public output applies bidi reversal once.

pub(crate) mod cache;
mod features;
pub(crate) use features::FeatureSets;
mod input;
mod instance;
pub(crate) mod orientation;
pub(crate) use instance::RunInstance;
pub(crate) use instance::resolve as resolve_instance;
use std::sync::Arc;

use std::ops::Range;

#[cfg(test)]
std::thread_local! {
    pub(super) static HARFRUST_SHAPE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static CFF_ORIGIN_DELTA_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static COMBINED_WIDTH_GROUP_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static COMBINED_WIDTH_GROUP_CLONE_SCALARS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static COMBINED_WIDTH_GROUP_CLONE_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static COMBINED_WIDTH_TRIAL_STORE_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static COMBINED_WIDTH_PROBE_MODE: std::cell::Cell<CombinedWidthProbeMode> = const { std::cell::Cell::new(CombinedWidthProbeMode::ScopedReuse) };
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CombinedWidthProbeMode {
    /// Reference: select with cloned trial groups, then shape every item
    /// through the scoped path.
    CloneReference,
    /// Reference variant that borrows the trial feature override.
    FeatureView,
    /// Production: reuse the selected trial output under BaseScope replay.
    ScopedReuse,
}

#[cfg(test)]
pub(crate) fn set_combined_width_probe_mode(mode: CombinedWidthProbeMode) {
    COMBINED_WIDTH_PROBE_MODE.with(|current| current.set(mode));
}

#[cfg(test)]
pub(crate) fn combined_width_probe_mode_for_test() -> CombinedWidthProbeMode {
    COMBINED_WIDTH_PROBE_MODE.with(std::cell::Cell::get)
}

use crate::font::FontId;
use crate::geometry::{LayoutUnit, Saturation, WritingMode};
use crate::limits::{LimitExceeded, LimitKind, Limits};
use skrifa::{MetadataProvider, raw::TableProvider};

// Harfrust's vertical-origin fallback only reads glyf bounds. For a CFF face
// without VORG, OpenType instead requires the CFF outline top plus vmtx TSB.
fn cff_vertical_origin_delta(
    font: &skrifa::FontRef<'_>,
    glyph_id: u32,
    coords: &[skrifa::instance::NormalizedCoord],
) -> Option<f32> {
    #[cfg(test)]
    CFF_ORIGIN_DELTA_CALLS.with(|calls| calls.set(calls.get() + 1));
    let glyph = skrifa::GlyphId::new(glyph_id);
    let bounds = font
        .glyph_metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::new(coords),
        )
        .bounds(glyph)?;
    let tsb = f32::from(font.vmtx().ok()?.side_bearing(glyph)?);
    let vvar = font.vvar().ok();
    let origin = bounds.y_max
        + tsb
        + vvar
            .as_ref()
            .and_then(|v| v.tsb_delta(glyph, coords).ok())
            .map_or(0.0, |delta| delta.to_f32())
        + vvar
            .as_ref()
            .and_then(|v| v.v_org_delta(glyph, coords).ok())
            .map_or(0.0, |delta| delta.to_f32());
    let ascent = font
        .os2()
        .ok()
        .map(|os2| f32::from(os2.s_typo_ascender()))
        .or_else(|| {
            font.hhea()
                .ok()
                .map(|hhea| f32::from(hhea.ascender().to_i16()))
        })
        .unwrap_or(0.0);
    let ascent_delta = font
        .mvar()
        .ok()
        .and_then(|mvar| mvar.metric_delta(skrifa::Tag::new(b"hasc"), coords).ok())
        .map_or(0.0, |delta| delta.to_f32());
    Some(origin - ascent - ascent_delta)
}

/// A run is closed before its pen position would exceed this value, so
/// differences between pen positions within a run never saturate.
pub(crate) const RUN_PEN_LIMIT: i32 = 1 << 30;

#[derive(Clone, Debug, Default)]
pub(crate) struct GlyphStore {
    #[cfg(test)]
    pub(crate) _cache_clone_probe: cache_clone_probe::CloneProbe,
    pub(crate) id: Vec<u32>,
    pub(crate) advance: Vec<LayoutUnit>,
    /// Pen position before the glyph, relative to the start of its run.
    pub(crate) pen: Vec<LayoutUnit>,
    pub(crate) offset_inline: Vec<LayoutUnit>,
    pub(crate) offset_block: Vec<LayoutUnit>,
    /// Byte offset of the glyph's character in the processed text.
    pub(crate) cluster: Vec<u32>,
    /// Bits0/1: unsafe to break/concat before this shaping cluster.
    pub(crate) flags: Vec<u8>,
    /// Per-glyph layout spacing for tracked/word-spaced/justified owned windows.
    /// Shaping advances remain unchanged for public cluster measurements.
    pub(crate) spacing: Option<Vec<LayoutUnit>>,
    /// Logical leading space, kept separate from the pen shared by marks.
    pub(crate) leading: Option<Vec<LayoutUnit>>,
}

impl GlyphStore {
    pub(crate) fn len(&self) -> usize {
        self.id.len()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ShapedRun {
    pub(crate) glyphs: Range<u32>,
    pub(crate) text: Range<u32>,
    pub(crate) item: u32,
    pub(crate) orientation: orientation::RunOrientation,
    pub(crate) font: FontId,
    pub(crate) font_size: f32,
    pub(crate) instance: Arc<RunInstance>,
}

#[cfg(test)]
fn trial_storage_capacity_bytes(store: &GlyphStore, run_capacity: usize) -> usize {
    use std::mem::size_of;

    let mut bytes = store.id.capacity() * size_of::<u32>()
        + store.advance.capacity() * size_of::<LayoutUnit>()
        + store.pen.capacity() * size_of::<LayoutUnit>()
        + store.offset_inline.capacity() * size_of::<LayoutUnit>()
        + store.offset_block.capacity() * size_of::<LayoutUnit>()
        + store.cluster.capacity() * size_of::<u32>()
        + store.flags.capacity() * size_of::<u8>()
        + run_capacity * size_of::<ShapedRun>();
    if let Some(spacing) = &store.spacing {
        bytes += spacing.capacity() * size_of::<LayoutUnit>();
    }
    if let Some(leading) = &store.leading {
        bytes += leading.capacity() * size_of::<LayoutUnit>();
    }
    bytes
}

/// Prove applicable width-feature coverage through the selected font/script/
/// language/variation instance. A feature must change every visible source
/// character; otherwise keep the complete composition on its ordinary glyphs.
#[cfg(test)]
pub(crate) fn select_combined_widths(
    cx: &mut crate::LayoutContext,
    items: &mut [crate::analysis::itemize::ShapeItem],
    styles: &[crate::style::InlineStyle],
    fonts: &crate::font::FontCollection,
    mode: WritingMode,
    limits: &Limits,
) {
    let mut begin = 0;
    while begin < items.len() {
        let Some(combine) = items[begin].combine else {
            begin += 1;
            continue;
        };
        let mut end = begin + 1;
        while end < items.len() && items[end].combine == Some(combine) {
            end += 1;
        }
        let group = &items[begin..end];
        let mut starts: Vec<_> = group
            .iter()
            .flat_map(|item| item.scalars.iter())
            .filter(|scalar| scalar.grapheme_start)
            .map(|scalar| scalar.offset)
            .collect();
        starts.sort_unstable();
        starts.dedup();
        let tag = match starts.len() {
            2 => *b"hwid",
            3 => *b"twid",
            4 => *b"qwid",
            _ => {
                begin = end;
                continue;
            }
        };
        if group.iter().any(|item| item.font.is_none()) {
            begin = end;
            continue;
        }
        let mut warnings = crate::limits::WarningSink::new(limits.max_warnings);
        let mut sat = Saturation::default();
        let Ok((plain, _plain_runs)) = shape_items(
            cx,
            group,
            styles,
            fonts,
            mode,
            limits,
            &mut warnings,
            &mut sat,
        ) else {
            begin = end;
            continue;
        };
        COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| {
            bytes.set(
                bytes
                    .get()
                    .saturating_add(trial_storage_capacity_bytes(&plain, _plain_runs.capacity())),
            );
        });
        let narrow_result =
            if combined_width_probe_mode_for_test() == CombinedWidthProbeMode::CloneReference {
                use std::mem::size_of;

                COMBINED_WIDTH_GROUP_CLONE_COUNT.with(|count| count.set(count.get() + 1));
                COMBINED_WIDTH_GROUP_CLONE_SCALARS.with(|count| {
                    count.set(
                        count.get().saturating_add(
                            group.iter().map(|item| item.scalars.len()).sum::<usize>(),
                        ),
                    )
                });
                COMBINED_WIDTH_GROUP_CLONE_BYTES.with(|bytes| {
                    let cloned = std::mem::size_of_val(group)
                        + group
                            .iter()
                            .map(|item| {
                                item.scalars.len() * size_of::<crate::analysis::itemize::Scalar>()
                                    + item.before.len()
                                    + item.after.len()
                            })
                            .sum::<usize>();
                    bytes.set(bytes.get().saturating_add(cloned));
                });
                let mut candidate = group.to_vec();
                for item in &mut candidate {
                    item.width_feature = Some(tag);
                }
                shape_items(
                    cx,
                    &candidate,
                    styles,
                    fonts,
                    mode,
                    limits,
                    &mut warnings,
                    &mut sat,
                )
            } else {
                let mut trial_features = FeatureSets::new(group, styles);
                for item in group {
                    trial_features.prepare_width_feature(item, styles, tag);
                }
                shape_inputs(
                    cx,
                    group
                        .iter()
                        .map(|item| input::ShapeInput::whole(item).with_width_feature(Some(tag))),
                    styles,
                    fonts,
                    mode,
                    limits,
                    &mut warnings,
                    &mut sat,
                    None,
                    None,
                    &trial_features,
                    0,
                )
            };
        let Ok((narrow, _narrow_runs)) = narrow_result else {
            begin = end;
            continue;
        };
        COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| {
            bytes.set(bytes.get().saturating_add(trial_storage_capacity_bytes(
                &narrow,
                _narrow_runs.capacity(),
            )));
        });
        starts.push(group.last().unwrap().end);
        let covered = starts.windows(2).all(|range| {
            let a = plain.cluster.partition_point(|offset| *offset < range[0])
                ..plain.cluster.partition_point(|offset| *offset < range[1]);
            let b = narrow.cluster.partition_point(|offset| *offset < range[0])
                ..narrow.cluster.partition_point(|offset| *offset < range[1]);
            if a.is_empty() || b.is_empty() {
                return false;
            }
            let visible = plain.advance[a.clone()]
                .iter()
                .any(|advance| advance.raw() != 0);
            !visible || plain.id[a] != narrow.id[b]
        });
        if covered {
            for item in &mut items[begin..end] {
                item.width_feature = Some(tag);
            }
        }
        begin = end;
    }
}

/// Shape a paragraph, with or without Ruby base scopes, while consuming each
/// selected width-feature trial directly into its retained output. Without
/// scopes every selected trial is reused; with scopes a trial is reused only
/// when `scoped_reuse_fits`, and otherwise the group is shaped through the
/// scoped path so local budgets fail exactly as before.
#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_items_with_combined_width_reuse(
    cx: &mut crate::LayoutContext,
    items: &mut [crate::analysis::itemize::ShapeItem],
    styles: &[crate::style::InlineStyle],
    fonts: &crate::font::FontCollection,
    mode: WritingMode,
    limits: &Limits,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
    mut bases: Option<&mut crate::ruby::base_budget::BaseScopes>,
    feature_sets: &mut FeatureSets,
) -> Result<(GlyphStore, Vec<ShapedRun>), LimitExceeded> {
    let mut glyphs = GlyphStore::default();
    let mut runs = Vec::new();
    let mut cursor = 0;
    let mut traces = bases.is_some().then(|| (Vec::new(), Vec::new()));

    while cursor < items.len() {
        let Some((start, end, tag, starts)) = next_combined_width_group(items, cursor) else {
            let (rest, rest_runs) = shape_inputs(
                cx,
                items[cursor..].iter().map(input::ShapeInput::whole),
                styles,
                fonts,
                mode,
                limits,
                warnings,
                sat,
                bases.as_deref_mut(),
                None,
                feature_sets,
                glyphs.len() as u64,
            )?;
            append_shape_output(&mut glyphs, &mut runs, rest, rest_runs);
            break;
        };

        if start > cursor {
            let (prefix, prefix_runs) = shape_inputs(
                cx,
                items[cursor..start].iter().map(input::ShapeInput::whole),
                styles,
                fonts,
                mode,
                limits,
                warnings,
                sat,
                bases.as_deref_mut(),
                None,
                feature_sets,
                glyphs.len() as u64,
            )?;
            append_shape_output(&mut glyphs, &mut runs, prefix, prefix_runs);
        }

        let selection = {
            let group = &items[start..end];
            try_shape_combined_width_group(
                cx,
                group,
                &starts,
                tag,
                styles,
                fonts,
                mode,
                limits,
                feature_sets,
                traces.as_mut(),
            )
        };

        if let Some(selection) = selection {
            if selection.use_width_feature {
                for item in &mut items[start..end] {
                    item.width_feature = Some(tag);
                    feature_sets.retain_width_feature(item, tag);
                }
            } else {
                for item in &items[start..end] {
                    feature_sets.discard_unselected_width_feature(item, tag);
                }
            }

            // The independent trial limit starts at zero. If its selected
            // output would cross the paragraph limit, shape this group through
            // the ordinary global-counting path to preserve the exact failing
            // run and LimitExceeded.actual value.
            let group_exceeds_limit = limits
                .max_shaped_glyphs
                .is_some_and(|max| glyphs.len() as u64 + selection.glyphs.len() as u64 > max);
            let scoped_fits = match (bases.as_deref_mut(), traces.as_ref()) {
                (Some(bases), Some((plain, narrow))) => scoped_reuse_fits(
                    bases,
                    &items[start..end],
                    limits,
                    if selection.use_width_feature {
                        narrow
                    } else {
                        plain
                    },
                ),
                _ => true,
            };
            if group_exceeds_limit || !scoped_fits {
                let (group_glyphs, group_runs) = shape_inputs(
                    cx,
                    items[start..end].iter().map(input::ShapeInput::whole),
                    styles,
                    fonts,
                    mode,
                    limits,
                    warnings,
                    sat,
                    bases.as_deref_mut(),
                    None,
                    feature_sets,
                    glyphs.len() as u64,
                )?;
                append_shape_output(&mut glyphs, &mut runs, group_glyphs, group_runs);
            } else {
                if let (Some(bases), Some((plain, narrow))) =
                    (bases.as_deref_mut(), traces.as_ref())
                {
                    let trace = if selection.use_width_feature {
                        narrow
                    } else {
                        plain
                    };
                    for charge in trace {
                        bases.item(charge.item as usize, LimitKind::ShapedGlyphs, charge.glyphs)?;
                    }
                }
                warnings.append(selection.warnings);
                sat.saturated += selection.saturation.saturated;
                sat.non_finite += selection.saturation.non_finite;
                append_shape_output(&mut glyphs, &mut runs, selection.glyphs, selection.runs);
            }
        } else {
            let (group_glyphs, group_runs) = shape_inputs(
                cx,
                items[start..end].iter().map(input::ShapeInput::whole),
                styles,
                fonts,
                mode,
                limits,
                warnings,
                sat,
                bases.as_deref_mut(),
                None,
                feature_sets,
                glyphs.len() as u64,
            )?;
            append_shape_output(&mut glyphs, &mut runs, group_glyphs, group_runs);
        }
        cursor = end;
    }

    Ok((glyphs, runs))
}

struct CombinedWidthSelection {
    glyphs: GlyphStore,
    runs: Vec<ShapedRun>,
    warnings: crate::limits::WarningSink,
    saturation: Saturation,
    use_width_feature: bool,
}

fn next_combined_width_group(
    items: &[crate::analysis::itemize::ShapeItem],
    mut cursor: usize,
) -> Option<(usize, usize, [u8; 4], Vec<u32>)> {
    while cursor < items.len() {
        let Some(combine) = items[cursor].combine else {
            cursor += 1;
            continue;
        };
        let mut end = cursor + 1;
        while end < items.len() && items[end].combine == Some(combine) {
            end += 1;
        }
        let group = &items[cursor..end];
        if group.iter().any(|item| item.font.is_none()) {
            cursor = end;
            continue;
        }
        let mut starts: Vec<_> = group
            .iter()
            .flat_map(|item| item.scalars.iter())
            .filter(|scalar| scalar.grapheme_start)
            .map(|scalar| scalar.offset)
            .collect();
        starts.sort_unstable();
        starts.dedup();
        let tag = match starts.len() {
            2 => *b"hwid",
            3 => *b"twid",
            4 => *b"qwid",
            _ => {
                cursor = end;
                continue;
            }
        };
        return Some((cursor, end, tag, starts));
    }
    None
}

/// Whether a selected unscoped trial can stand in for scoped shaping: every
/// input must be one window under both the scoped and global run budgets (so
/// neither path splits or warns), and replaying the trial's charges must fit
/// every owning BaseScope chain.
fn scoped_reuse_fits(
    bases: &mut crate::ruby::base_budget::BaseScopes,
    group: &[crate::analysis::itemize::ShapeItem],
    limits: &Limits,
    trace: &[WindowCharge],
) -> bool {
    let global = limits.max_shaping_run_bytes.unwrap_or(u64::MAX);
    group.iter().all(|item| {
        let Some(first) = item.scalars.first() else {
            return true;
        };
        let bytes = item
            .scalars
            .iter()
            .map(|scalar| scalar.c.len_utf8() as u64)
            .fold(0u64, u64::saturating_add);
        bytes <= bases.shaping_run_bytes(first.item as usize, global)
    }) && bases.can_charge_shaped_glyphs(
        trace
            .iter()
            .map(|charge| (charge.item as usize, charge.glyphs)),
    )
}

#[allow(clippy::too_many_arguments)]
fn try_shape_combined_width_group(
    cx: &mut crate::LayoutContext,
    group: &[crate::analysis::itemize::ShapeItem],
    starts: &[u32],
    tag: [u8; 4],
    styles: &[crate::style::InlineStyle],
    fonts: &crate::font::FontCollection,
    mode: WritingMode,
    limits: &Limits,
    feature_sets: &mut FeatureSets,
    traces: Option<&mut (Vec<WindowCharge>, Vec<WindowCharge>)>,
) -> Option<CombinedWidthSelection> {
    let (plain_trace, narrow_trace) = match traces {
        Some((plain, narrow)) => {
            plain.clear();
            narrow.clear();
            (Some(plain), Some(narrow))
        }
        None => (None, None),
    };
    for item in group {
        feature_sets.prepare_width_feature(item, styles, tag);
    }

    let mut plain_warnings = crate::limits::WarningSink::new(limits.max_warnings);
    let mut plain_saturation = Saturation::default();
    let Ok((plain, plain_runs)) = shape_inputs(
        cx,
        group
            .iter()
            .map(|item| input::ShapeInput::whole(item).with_width_feature(None)),
        styles,
        fonts,
        mode,
        limits,
        &mut plain_warnings,
        &mut plain_saturation,
        None,
        plain_trace,
        feature_sets,
        0,
    ) else {
        for item in group {
            feature_sets.discard_unselected_width_feature(item, tag);
        }
        return None;
    };
    #[cfg(test)]
    COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| {
        bytes.set(
            bytes
                .get()
                .saturating_add(trial_storage_capacity_bytes(&plain, plain_runs.capacity())),
        );
    });

    let mut narrow_warnings = crate::limits::WarningSink::new(limits.max_warnings);
    let mut narrow_saturation = Saturation::default();
    let narrow_result = shape_inputs(
        cx,
        group
            .iter()
            .map(|item| input::ShapeInput::whole(item).with_width_feature(Some(tag))),
        styles,
        fonts,
        mode,
        limits,
        &mut narrow_warnings,
        &mut narrow_saturation,
        None,
        narrow_trace,
        feature_sets,
        0,
    );

    let Some((narrow, narrow_runs)) = narrow_result.ok() else {
        for item in group {
            feature_sets.discard_unselected_width_feature(item, tag);
        }
        return Some(CombinedWidthSelection {
            glyphs: plain,
            runs: plain_runs,
            warnings: plain_warnings,
            saturation: plain_saturation,
            use_width_feature: false,
        });
    };
    #[cfg(test)]
    COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| {
        bytes.set(bytes.get().saturating_add(trial_storage_capacity_bytes(
            &narrow,
            narrow_runs.capacity(),
        )));
    });

    let group_end = group.last().expect("nonempty combined group").end;
    let covered = starts.iter().enumerate().all(|(index, start)| {
        let end = starts.get(index + 1).copied().unwrap_or(group_end);
        let plain_range = plain.cluster.partition_point(|offset| *offset < *start)
            ..plain.cluster.partition_point(|offset| *offset < end);
        let narrow_range = narrow.cluster.partition_point(|offset| *offset < *start)
            ..narrow.cluster.partition_point(|offset| *offset < end);
        if plain_range.is_empty() || narrow_range.is_empty() {
            return false;
        }
        let visible = plain.advance[plain_range.clone()]
            .iter()
            .any(|advance| advance.raw() != 0);
        !visible || plain.id[plain_range] != narrow.id[narrow_range]
    });

    if covered {
        Some(CombinedWidthSelection {
            glyphs: narrow,
            runs: narrow_runs,
            warnings: narrow_warnings,
            saturation: narrow_saturation,
            use_width_feature: true,
        })
    } else {
        for item in group {
            feature_sets.discard_unselected_width_feature(item, tag);
        }
        Some(CombinedWidthSelection {
            glyphs: plain,
            runs: plain_runs,
            warnings: plain_warnings,
            saturation: plain_saturation,
            use_width_feature: false,
        })
    }
}

fn append_shape_output(
    target: &mut GlyphStore,
    target_runs: &mut Vec<ShapedRun>,
    mut source: GlyphStore,
    mut source_runs: Vec<ShapedRun>,
) {
    if target.len() == 0
        && target_runs.is_empty()
        && target.spacing.is_none()
        && target.leading.is_none()
    {
        *target = source;
        *target_runs = source_runs;
        return;
    }
    let old_len = target.len();
    let added_len = source.len();
    let glyph_offset = old_len as u32;
    for run in &mut source_runs {
        run.glyphs.start += glyph_offset;
        run.glyphs.end += glyph_offset;
    }
    target.id.append(&mut source.id);
    target.advance.append(&mut source.advance);
    target.pen.append(&mut source.pen);
    target.offset_inline.append(&mut source.offset_inline);
    target.offset_block.append(&mut source.offset_block);
    target.cluster.append(&mut source.cluster);
    target.flags.append(&mut source.flags);
    append_optional_layout_values(&mut target.spacing, source.spacing, old_len, added_len);
    append_optional_layout_values(&mut target.leading, source.leading, old_len, added_len);
    target_runs.append(&mut source_runs);
}

fn append_optional_layout_values(
    target: &mut Option<Vec<LayoutUnit>>,
    source: Option<Vec<LayoutUnit>>,
    old_len: usize,
    added_len: usize,
) {
    match (target, source) {
        (Some(target), Some(mut source)) => target.append(&mut source),
        (Some(target), None) => target.resize(old_len + added_len, LayoutUnit::ZERO),
        (target @ None, Some(mut source)) => {
            let mut combined = Vec::with_capacity(old_len + added_len);
            combined.resize(old_len, LayoutUnit::ZERO);
            combined.append(&mut source);
            *target = Some(combined);
        }
        (None, None) => {}
    }
}

/// Shapes one compatible style/font/script segment, then assigns each cluster
/// to the item supplying its first scalar. Node boundaries do not lose GSUB.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_items(
    cx: &mut crate::LayoutContext,
    items: &[crate::analysis::itemize::ShapeItem],
    styles: &[crate::style::InlineStyle],
    fonts: &crate::font::FontCollection,
    mode: WritingMode,
    limits: &Limits,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
) -> Result<(GlyphStore, Vec<ShapedRun>), LimitExceeded> {
    shape_items_with_base_scopes(
        cx,
        items,
        styles,
        fonts,
        mode,
        limits,
        warnings,
        sat,
        None,
        &FeatureSets::new(items, styles),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_items_with_base_scopes(
    cx: &mut crate::LayoutContext,
    items: &[crate::analysis::itemize::ShapeItem],
    styles: &[crate::style::InlineStyle],
    fonts: &crate::font::FontCollection,
    mode: WritingMode,
    limits: &Limits,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
    bases: Option<&mut crate::ruby::base_budget::BaseScopes>,
    feature_sets: &FeatureSets,
) -> Result<(GlyphStore, Vec<ShapedRun>), LimitExceeded> {
    shape_inputs(
        cx,
        items.iter().map(input::ShapeInput::whole),
        styles,
        fonts,
        mode,
        limits,
        warnings,
        sat,
        bases,
        None,
        feature_sets,
        0,
    )
}

/// Glyphs one harfrust call charged to its first scalar's owning item, in
/// shaping order. Trials record it so a reused output can replay BaseScope
/// charges exactly. Only harfrust calls are traced: the per-scalar charges of
/// missing-font notdef glyphs are not, which is safe because groups with a
/// missing font never take part in width selection and are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WindowCharge {
    pub(crate) item: u32,
    pub(crate) glyphs: u64,
}

#[allow(clippy::too_many_arguments)]
fn shape_inputs<'a>(
    cx: &mut crate::LayoutContext,
    items: impl IntoIterator<Item = input::ShapeInput<'a>>,
    styles: &[crate::style::InlineStyle],
    fonts: &crate::font::FontCollection,
    mode: WritingMode,
    limits: &Limits,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
    mut bases: Option<&mut crate::ruby::base_budget::BaseScopes>,
    mut trace: Option<&mut Vec<WindowCharge>>,
    feature_sets: &FeatureSets,
    glyph_offset: u64,
) -> Result<(GlyphStore, Vec<ShapedRun>), LimitExceeded> {
    cx.bound_shaping_scratch(limits);
    let mut store = GlyphStore::default();
    let mut runs: Vec<ShapedRun> = Vec::new();
    for input in items {
        let original = input.original;
        let style = &styles[original.style as usize];
        let features = feature_sets.get(original, input.width_feature);
        let font_data = original.font.as_ref().map(|found| {
            fonts
                .font_data(found.id)
                .expect("matched face remains retained")
        });
        let resolved = original
            .font
            .as_ref()
            .zip(font_data.as_ref())
            .map(|(found, data)| {
                let (shaper, mut instance, size) = instance::resolve(
                    data.data.as_ref(),
                    data.index,
                    found,
                    style,
                    original.script,
                    warnings,
                );
                let (metrics, vertical_metrics) =
                    fonts.metrics_from_data(found.id, data, size, &instance.coords);
                Arc::get_mut(&mut instance).expect("new instance").metrics = metrics;
                Arc::get_mut(&mut instance)
                    .expect("new instance")
                    .vertical_metrics = vertical_metrics;
                Arc::get_mut(&mut instance).expect("new instance").features = features.clone();
                (shaper, instance, size)
            });
        let missing_instance = if resolved.is_none() {
            Some(Arc::new(RunInstance {
                script: original.script,
                language: style.lang.clone(),
                features,
                metrics: Some(fonts.metrics(fonts.primary_font(), style.font_size)),
                ..Default::default()
            }))
        } else {
            None
        };
        // Face, shaper, vertical-origin source and language do not change
        // between budget windows of one input; prepare them once per input.
        let font = font_data.as_ref().map(|data| {
            harfrust::FontRef::from_index(data.data.as_ref(), data.index).expect("registered face")
        });
        let shared = original.font.as_ref().map(|found| {
            fonts
                .shaper_data(found.id)
                .expect("registered shaping data")
        });
        let shaper = shared
            .as_ref()
            .zip(font.as_ref())
            .zip(resolved.as_ref())
            .map(|((shared, font), (instance, _, _))| {
                shared.shaper(font).instance(Some(instance)).build()
            });
        let cff_without_vorg = font_data
            .as_ref()
            .filter(|_| original.orientation == orientation::RunOrientation::Upright)
            .and_then(|data| skrifa::FontRef::from_index(data.data.as_ref(), data.index).ok())
            .filter(|font| {
                font.vorg().is_err()
                    && (font.cff().is_ok() || font.cff2().is_ok())
                    && font.vmtx().is_ok()
            });
        // Origin deltas depend on this input's face and variation coordinates,
        // so the cache never outlives the input.
        let mut cff_origin_deltas = std::collections::HashMap::new();
        let language: Option<harfrust::Language> = style.lang.as_ref().and_then(|l| l.parse().ok());
        // Authored bidi controls start no paragraph grapheme but are always
        // their own shaping cluster (itemize/graphemes.rs), so a budget window
        // may also end right before one.
        let bidi_controls =
            icu_properties::CodePointSetData::new::<icu_properties::props::BidiControl>();
        let mut cursor = 0;
        while cursor < input.scalars.len() {
            let start = cursor;
            let budget = limits.max_shaping_run_bytes.unwrap_or(u64::MAX);
            let budget = bases.as_ref().map_or(budget, |bases| {
                bases.shaping_run_bytes(input.scalars[start].item as usize, budget)
            });
            let mut bytes = 0;
            let mut boundary = start;
            while cursor < input.scalars.len() {
                let scalar = &input.scalars[cursor];
                if (scalar.grapheme_start || bidi_controls.contains(scalar.c)) && cursor > start {
                    boundary = cursor;
                }
                let next = input.scalars[cursor].c.len_utf8() as u64;
                if bytes + next > budget && cursor > start {
                    if boundary > start {
                        cursor = boundary;
                    } else {
                        warnings.push(crate::limits::WarningKind::Unsupported,"giant grapheme exceeds shaping run budget; splitting at scalar boundary");
                    }
                    break;
                }
                bytes += next;
                cursor += 1;
                if next > budget {
                    warnings.push(
                        crate::limits::WarningKind::Unsupported,
                        "scalar exceeds shaping run budget; forcing grapheme progress",
                    );
                    break;
                }
            }
            let last = &input.scalars[cursor - 1];
            let scalars = &input.scalars[start..cursor];
            let window_end = last.end;
            let window_run_start = runs.len();
            let Some(found) = &original.font else {
                warnings.push(
                    crate::limits::WarningKind::Unsupported,
                    "missing font; using .notdef glyphs",
                );
                let run_instance = missing_instance.as_ref().expect("missing font instance");
                for scalar in scalars {
                    if let Some(bases) = &mut bases {
                        bases.item(scalar.item as usize, LimitKind::ShapedGlyphs, 1)?;
                    }
                    let glyph = store.len();
                    push_notdef_glyph(
                        &mut store,
                        scalar.c,
                        scalar.offset,
                        style.font_size,
                        limits,
                        sat,
                        glyph_offset,
                    )?;
                    // Extend the window's previous run while it is contiguous
                    // and its pen stays under the limit; otherwise start a run.
                    if runs.len() > window_run_start
                        && let Some(previous) = runs.last_mut()
                        && previous.item == scalar.item
                        && previous.text.end == scalar.offset
                        && i64::from(store.pen[glyph - 1].raw())
                            + i64::from(store.advance[glyph - 1].raw())
                            + i64::from(store.advance[glyph].raw())
                            <= i64::from(RUN_PEN_LIMIT)
                    {
                        store.pen[glyph] = store.pen[glyph - 1] + store.advance[glyph - 1];
                        previous.glyphs.end = glyph as u32 + 1;
                        previous.text.end = scalar.end;
                    } else {
                        runs.push(ShapedRun {
                            glyphs: glyph as u32..glyph as u32 + 1,
                            text: scalar.offset..scalar.end,
                            item: scalar.item,
                            orientation: original.orientation,
                            font: fonts.primary_font(),
                            font_size: style.font_size,
                            instance: Arc::clone(run_instance),
                        });
                    }
                }
                continue;
            };
            let (instance, run_instance, font_size) = resolved.as_ref().expect("matched instance");
            let font_size = *font_size;
            let shaper = shaper.as_ref().expect("matched shaper");
            let mut buffer = cx.scratch.take().unwrap_or_default();
            buffer.clear();
            for scalar in scalars {
                buffer.add(scalar.c, scalar.offset);
            }
            // Inner window edges take up to five neighbouring scalars inline;
            // only the input's outer edges use the item context.
            let pre = (start > 0).then(|| {
                input::Context::from_chars(
                    input.scalars[start.saturating_sub(5)..start]
                        .iter()
                        .map(|s| s.c),
                )
            });
            let post = (cursor < input.scalars.len())
                .then(|| input::Context::from_chars(input.scalars[cursor..].iter().map(|s| s.c)));
            buffer.set_pre_context(pre.as_ref().unwrap_or(&input.before).as_str());
            buffer.set_post_context(post.as_ref().unwrap_or(&input.after).as_str());
            let upright = original.orientation == orientation::RunOrientation::Upright;
            buffer.set_direction(if upright {
                harfrust::Direction::TopToBottom
            } else if original.level % 2 == 1 {
                harfrust::Direction::RightToLeft
            } else {
                harfrust::Direction::LeftToRight
            });
            buffer.set_script(
                harfrust::Script::from_iso15924_tag(harfrust::Tag::new(&original.script))
                    .unwrap_or(harfrust::script::UNKNOWN),
            );
            if let Some(language) = &language {
                buffer.set_language(language.clone());
            }
            // Keep controls in the shaping input for joining/substitutions, but
            // remove their residual glyphs: even a zero-width space can have ink.
            let mut flags = harfrust::BufferFlags::PRODUCE_UNSAFE_TO_CONCAT
                | harfrust::BufferFlags::REMOVE_DEFAULT_IGNORABLES;
            if !scalars[0].grapheme_start {
                flags |= harfrust::BufferFlags::DO_NOT_INSERT_DOTTED_CIRCLE;
            }
            buffer.set_flags(flags);
            let features = &run_instance.features;
            let plan = cx
                .plans
                .get(found.id, shaper, &buffer, Some(instance), features);
            let shaped = shaper.shape(
                buffer,
                harfrust::ShapeOptions::default()
                    .features(features)
                    .plan(Some(&plan)),
            );
            #[cfg(test)]
            HARFRUST_SHAPE_CALLS.with(|calls| calls.set(calls.get() + 1));
            Limits::check(
                limits.max_shaped_glyphs,
                LimitKind::ShapedGlyphs,
                glyph_offset + store.len() as u64 + shaped.len() as u64,
            )?;
            if let Some(trace) = &mut trace {
                trace.push(WindowCharge {
                    item: scalars[0].item,
                    glyphs: shaped.len() as u64,
                });
            }
            if let Some(bases) = &mut bases {
                // Ruby boundaries/isolation delimit shaping segments; transparent
                // DOM node boundaries within a base keep the same scope.
                bases.item(
                    scalars[0].item as usize,
                    LimitKind::ShapedGlyphs,
                    shaped.len() as u64,
                )?;
            }
            let scale = font_size / shaper.units_per_em() as f32;
            // Sort clusters into logical order while preserving the shaper's
            // intra-cluster order. Public output positions handle RTL groups.
            // Harfrust may merge a removed leading control into the next
            // glyph's cluster. Keep its source owner separate from the ink.
            let ignorables = icu_properties::CodePointSetData::new::<
                icu_properties::props::DefaultIgnorableCodePoint,
            >();
            let leading_end = scalars
                .iter()
                .find(|scalar| !ignorables.contains(scalar.c))
                .filter(|scalar| scalar.grapheme_start)
                .map_or(scalars[0].offset, |scalar| scalar.offset);
            let output_cluster =
                |index: usize| shaped.glyph_infos()[index].cluster.max(leading_end);
            let mut order: Vec<_> = (0..shaped.len()).collect();
            order.sort_by_key(|i| output_cluster(*i));
            // A glyph-free prefix still owns text, grapheme limits and break
            // opportunities (including a standalone discretionary hyphen).
            let first_cluster = order
                .first()
                .map_or(window_end, |index| output_cluster(*index));
            let mut absent = 0;
            while absent < scalars.len() && scalars[absent].offset < first_cluster {
                let first = &scalars[absent];
                let mut finish = absent + 1;
                while finish < scalars.len()
                    && scalars[finish].offset < first_cluster
                    && !scalars[finish].grapheme_start
                    && scalars[finish].item == first.item
                {
                    finish += 1;
                }
                runs.push(ShapedRun {
                    glyphs: store.len() as u32..store.len() as u32,
                    text: first.offset..scalars[finish - 1].end,
                    item: first.item,
                    orientation: original.orientation,
                    font: found.id,
                    font_size,
                    instance: Arc::clone(run_instance),
                });
                absent = finish;
            }
            let mut begin = 0;
            // `order` is cluster-ascending, so the cluster value examined each
            // iteration only grows; `scalar_cursor` tracks the matching position
            // in `scalars` (offset-ascending) instead of re-searching the
            // whole slice with `partition_point` on every cluster.
            let mut scalar_cursor = 0usize;
            while begin < order.len() {
                let cluster = output_cluster(order[begin]);
                let mut end = begin + 1;
                while end < order.len() && output_cluster(order[end]) == cluster {
                    end += 1;
                }
                while scalar_cursor < scalars.len() && scalars[scalar_cursor].offset <= cluster {
                    scalar_cursor += 1;
                }
                let scalar_index = scalar_cursor.saturating_sub(1);
                debug_assert_eq!(
                    scalar_cursor,
                    scalars.partition_point(|s| s.offset <= cluster),
                    "scalar_cursor must track partition_point(|s| s.offset <= cluster); \
                     scalars is not offset-ascending"
                );
                let owner = scalars[scalar_index].item;
                let next_cluster = if end < order.len() {
                    output_cluster(order[end])
                } else {
                    window_end
                };
                // Transparent anchors do not belong to the preceding cluster.
                // A cluster spanning an anchor still covers all its real scalars,
                // while a gap between clusters ends at the last actual scalar.
                while scalar_cursor < scalars.len() && scalars[scalar_cursor].offset < next_cluster
                {
                    scalar_cursor += 1;
                }
                let scalar_end = scalar_cursor;
                debug_assert_eq!(
                    scalar_end,
                    scalars.partition_point(|s| s.offset < next_cluster),
                    "scalar_cursor must track partition_point(|s| s.offset < next_cluster); \
                     scalars is not offset-ascending"
                );
                let last_scalar = &scalars[scalar_end.saturating_sub(1)];
                let cluster_end = last_scalar.end;
                let mut parts = Vec::new();
                let mut part_start = begin;
                let mut part_advance = 0i64;
                for (at, index) in order.iter().enumerate().take(end).skip(begin) {
                    let position = &shaped.glyph_positions()[*index];
                    let advance = LayoutUnit::from_f32_round(
                        if upright {
                            -position.y_advance
                        } else {
                            position.x_advance
                        } as f32
                            * scale,
                        sat,
                    )
                    .raw() as i64;
                    if at > part_start && (part_advance + advance).abs() > i64::from(RUN_PEN_LIMIT)
                    {
                        parts.push((part_start..at, part_advance));
                        part_start = at;
                        part_advance = 0;
                    }
                    part_advance += advance;
                }
                // The usual unsplit cluster keeps its sole part on the stack.
                // Only a pen-budget split above allocates the fallback Vec.
                let single = if parts.is_empty() {
                    Some((part_start..end, part_advance))
                } else {
                    parts.push((part_start..end, part_advance));
                    None
                };
                if parts.len() > 1 || part_advance.abs() > i64::from(RUN_PEN_LIMIT) {
                    warnings.push(crate::limits::WarningKind::Unsupported,
                        "glyph cluster exceeds run pen budget; splitting storage without introducing a break");
                }
                // Splitting storage does not reverse a cluster's internal visual
                // order. Bidi reverses the chunk units, so store RTL chunks in
                // reverse order while keeping each chunk's glyph order intact.
                if original.level % 2 == 1 {
                    parts.reverse();
                }
                for (part, part_advance) in single.into_iter().chain(parts) {
                    let run_start = store.len() as u32;
                    let mut pen = LayoutUnit::ZERO;
                    for index in &order[part.clone()] {
                        let info = &shaped.glyph_infos()[*index];
                        let pos = &shaped.glyph_positions()[*index];
                        let advance = LayoutUnit::from_f32_round(
                            if upright {
                                -pos.y_advance
                            } else {
                                pos.x_advance
                            } as f32
                                * scale,
                            sat,
                        );
                        store.flags.push(
                            u8::from(info.unsafe_to_break())
                                | (u8::from(info.unsafe_to_concat()) << 1),
                        );
                        store.id.push(info.glyph_id);
                        store.cluster.push(cluster);
                        store.advance.push(advance);
                        store.pen.push(pen);
                        let offset = LayoutUnit::from_f32_round(
                            (if upright { -pos.y_offset } else { pos.x_offset } as f32
                                + if upright {
                                    cff_without_vorg
                                        .as_ref()
                                        .and_then(|font| {
                                            *cff_origin_deltas.entry(info.glyph_id).or_insert_with(
                                                || {
                                                    cff_vertical_origin_delta(
                                                        font,
                                                        info.glyph_id,
                                                        instance.coords(),
                                                    )
                                                },
                                            )
                                        })
                                        .unwrap_or(0.0)
                                } else {
                                    0.0
                                })
                                * scale,
                            sat,
                        );
                        let offset = if original.level % 2 == 1 {
                            LayoutUnit::from_raw(
                                part_advance.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                            )
                            .sub(pen, sat)
                            .sub(pen, sat)
                            .sub(advance, sat)
                            .sub(offset, sat)
                        } else {
                            offset
                        };
                        store.offset_inline.push(offset);
                        store.offset_block.push(LayoutUnit::from_f32_round(
                            if upright {
                                match mode {
                                    WritingMode::VerticalLr => pos.x_offset,
                                    _ => -pos.x_offset,
                                }
                            } else {
                                -pos.y_offset
                            } as f32
                                * scale,
                            sat,
                        ));
                        pen = pen.add(advance, sat);
                    }
                    let (part_min, part_max) = store.pen[run_start as usize..].iter().fold(
                        (part_advance.min(0), part_advance.max(0)),
                        |(min, max), p| (min.min(i64::from(p.raw())), max.max(i64::from(p.raw()))),
                    );
                    let previous_end_pen =
                        runs.last().filter(|r| !r.glyphs.is_empty()).map_or(0, |r| {
                            i64::from(store.pen[r.glyphs.end as usize - 1].raw())
                                + i64::from(store.advance[r.glyphs.end as usize - 1].raw())
                        });
                    if runs.len() > window_run_start
                        && let Some(run) = runs.last_mut()
                        && !run.glyphs.is_empty()
                        && run.item == owner
                        && run.font == found.id
                        && run.text.end == cluster
                        && previous_end_pen + part_min >= -i64::from(RUN_PEN_LIMIT)
                        && previous_end_pen + part_max <= i64::from(RUN_PEN_LIMIT)
                    {
                        let old_end = run.glyphs.end as usize;
                        let old_pen = store.pen[old_end - 1] + store.advance[old_end - 1];
                        for p in &mut store.pen[run_start as usize..] {
                            *p = p.add(old_pen, sat);
                        }
                        run.glyphs.end = store.len() as u32;
                        run.text.end = cluster_end;
                    } else {
                        runs.push(ShapedRun {
                            glyphs: run_start..store.len() as u32,
                            text: cluster..cluster_end,
                            item: owner,
                            orientation: original.orientation,
                            font: found.id,
                            font_size,
                            instance: Arc::clone(run_instance),
                        });
                    }
                }
                begin = end;
            }
            cx.scratch_bytes = cx.scratch_bytes.max(
                scalars
                    .len()
                    .max(shaped.len())
                    .max(4)
                    .next_power_of_two()
                    .saturating_mul(128),
            );
            cx.scratch = Some(shaped.clear());
            cx.bound_shaping_scratch(limits);
        }
    }
    Ok((store, runs))
}

pub(crate) fn is_mark(c: char) -> bool {
    use icu_properties::{CodePointMapData, props::GeneralCategory};
    matches!(
        CodePointMapData::<GeneralCategory>::new().get(c),
        GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::EnclosingMark
    )
}

/// Pushes one `.notdef` glyph for a scalar without a matching face, at pen
/// zero: one em of advance, or zero for marks (shifted back half an em) and
/// default ignorables. The caller places it in a run.
fn push_notdef_glyph(
    store: &mut GlyphStore,
    c: char,
    cluster: u32,
    font_size: f32,
    limits: &Limits,
    sat: &mut Saturation,
    glyph_offset: u64,
) -> Result<(), LimitExceeded> {
    // Converted per glyph so saturation counts stay per scalar.
    let em = LayoutUnit::from_f32_round(font_size, sat);
    Limits::check(
        limits.max_shaped_glyphs,
        LimitKind::ShapedGlyphs,
        glyph_offset + store.len() as u64 + 1,
    )?;
    let mark = is_mark(c);
    let advance = if mark || icu_properties::CodePointSetData::new::<
        icu_properties::props::DefaultIgnorableCodePoint,
    >()
    .contains(c)
    {
        LayoutUnit::ZERO
    } else {
        em
    };
    store.flags.push(0);
    store.id.push(0);
    store.advance.push(advance);
    store.pen.push(LayoutUnit::ZERO);
    store.offset_inline.push(if mark {
        LayoutUnit::ZERO - em.div_i32(2)
    } else {
        LayoutUnit::ZERO
    });
    store.offset_block.push(LayoutUnit::ZERO);
    store.cluster.push(cluster);
    Ok(())
}

pub(crate) fn shape_line_edge(
    data: &crate::paragraph::ParagraphData,
    unit: &crate::analysis::units::Unit,
    cx: &mut crate::LayoutContext,
    sat: &mut Saturation,
) -> Option<GlyphStore> {
    let mut warnings = crate::limits::WarningSink::new(data.limits.max_warnings);
    let result = shape_window(data, unit, cx, &mut warnings, sat).map(|(store, _)| store);
    for warning in warnings.take() {
        cx.warnings.push(warning.kind, warning.message);
    }
    result
}

/// Shape only real scalars from the original itemization. Transparent controls
/// and anchors remain in source offsets, but are never submitted as glyphs.
pub(crate) fn shape_window(
    data: &crate::paragraph::ParagraphData,
    unit: &crate::analysis::units::Unit,
    cx: &mut crate::LayoutContext,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
) -> Option<(GlyphStore, Vec<ShapedRun>)> {
    shape_window_budget(data, unit, data.limits.max_shaped_glyphs, cx, warnings, sat)
}

pub(crate) fn shape_window_budget(
    data: &crate::paragraph::ParagraphData,
    unit: &crate::analysis::units::Unit,
    glyph_budget: Option<u64>,
    cx: &mut crate::LayoutContext,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
) -> Option<(GlyphStore, Vec<ShapedRun>)> {
    shape_window_edit(data, unit, glyph_budget, None, cx, warnings, sat)
}

pub(crate) struct Replacement {
    pub(crate) text: Range<u32>,
    pub(crate) c: char,
    pub(crate) font: Option<std::sync::Arc<crate::font::FontMatch>>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_window_edit(
    data: &crate::paragraph::ParagraphData,
    unit: &crate::analysis::units::Unit,
    glyph_budget: Option<u64>,
    replacement: Option<&Replacement>,
    cx: &mut crate::LayoutContext,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
) -> Option<(GlyphStore, Vec<ShapedRun>)> {
    let contains_replacement =
        replacement.is_some_and(|r| unit.text.start <= r.text.start && r.text.end <= unit.text.end);
    let synthetic_extra = replacement.filter(|_| contains_replacement).map_or(0, |r| {
        (r.c.len_utf8() as u64).saturating_sub(u64::from(r.text.end - r.text.start))
    });
    if data
        .limits
        .max_reshape_window_bytes
        .is_some_and(|max| u64::from(unit.text.end - unit.text.start) + synthetic_extra > max)
    {
        warnings.push(
            crate::limits::WarningKind::Unsupported,
            "line edge reshape window exceeded; keeping shared glyphs",
        );
        return None;
    }
    let index = data
        .shape_items
        .partition_point(|item| item.end <= unit.text.start);
    let mut limits = data.limits.clone();
    limits.max_shaped_glyphs = glyph_budget;
    let result = if !contains_replacement
        && input::can_borrow(&data.shape_items[index..], &unit.text)
    {
        shape_inputs(
            cx,
            input::clipped_items(&data.shape_items[index..], unit.text.clone()),
            &data.styles,
            &data.fonts,
            data.style.writing_mode,
            &limits,
            warnings,
            sat,
            None,
            None,
            &data.shape_features,
            0,
        )
    } else {
        let mut items: Vec<crate::analysis::itemize::ShapeItem> = Vec::new();
        for original in data.shape_items[index..]
            .iter()
            .take_while(|i| i.scalars.first().is_some_and(|s| s.offset < unit.text.end))
        {
            let begin = original
                .scalars
                .partition_point(|s| s.offset < unit.text.start);
            let end = original
                .scalars
                .partition_point(|s| s.offset < unit.text.end);
            if begin == end {
                continue;
            }
            // Font substitution can split an original item; newly identical
            // adjacent segments join before shaping so GPOS sees the hyphen.
            let mut at = begin;
            while at < end {
                let edited = replacement.filter(|r| {
                    contains_replacement && original.scalars[at].offset == r.text.start
                });
                let font = edited.map_or_else(|| original.font.clone(), |r| r.font.clone());
                let mut finish = at + 1;
                if edited.is_none() {
                    while finish < end
                        && !replacement.is_some_and(|r| {
                            contains_replacement && original.scalars[finish].offset == r.text.start
                        })
                    {
                        finish += 1;
                    }
                }
                let mut before: Vec<_> = original
                    .before
                    .chars()
                    .chain(original.scalars[..at].iter().map(|s| s.c))
                    .rev()
                    .take(5)
                    .collect();
                before.reverse();
                let mut scalars = original.scalars[at..finish].to_vec();
                if let Some(r) = edited {
                    scalars[0].c = r.c;
                    scalars[0].end = r.text.end;
                }
                let part = crate::analysis::itemize::ShapeItem {
                    segment: original.segment,
                    end: edited.map_or_else(|| scalars.last().unwrap().end, |r| r.text.end),
                    scalars,
                    style: original.style,
                    level: original.level,
                    script: original.script,
                    font,
                    orientation: original.orientation,
                    combine: original.combine,
                    width_feature: original.width_feature,
                    before: before.into_iter().collect(),
                    after: original.scalars[finish..]
                        .iter()
                        .map(|s| s.c)
                        .chain(original.after.chars())
                        .take(5)
                        .collect(),
                };
                if let Some(previous) = items.last_mut()
                    && input::compatible(previous, &part)
                {
                    previous.scalars.extend(part.scalars);
                    previous.end = part.end;
                    previous.after = part.after;
                } else {
                    items.push(part);
                }
                at = finish;
            }
        }
        shape_items_with_base_scopes(
            cx,
            &items,
            &data.styles,
            &data.fonts,
            data.style.writing_mode,
            &limits,
            warnings,
            sat,
            None,
            &data.shape_features,
        )
    };
    match result {
        Ok((store, mut runs)) => {
            if let Some(r) = replacement.filter(|_| contains_replacement) {
                for run in &mut runs {
                    if run.text.end == r.text.start + r.c.len_utf8() as u32 {
                        run.text.end = r.text.end;
                    }
                }
            }
            Some((store, runs))
        }
        Err(_) => {
            warnings.push(
                crate::limits::WarningKind::Unsupported,
                "line edge glyph budget exceeded; keeping shared glyphs",
            );
            None
        }
    }
}

#[cfg(test)]
mod tests;

// Observe the real derived GlyphStore clone without a shipping field or counter.
#[cfg(test)]
pub(crate) mod cache_clone_probe {
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
