//! Construction and finalization of retained paragraph data.

use super::*;

/// Context-free work that can be completed before a shaping context is
/// available.
pub(crate) struct BuildAnalysis {
    normal: PreparedBuild,
    alternate: Option<PreparedBuild>,
    source_cuts: Option<Vec<std::ops::RangeInclusive<u32>>>,
    rubies: Vec<crate::ruby::builder::RubyInput>,
    bases: crate::ruby::base_budget::BaseScopes,
    alternate_bases: Option<crate::ruby::base_budget::BaseScopes>,
    alternate_warnings: Option<WarningSink>,
    warnings: WarningSink,
}

struct PreparedBuild {
    style: ParagraphStyle,
    limits: Limits,
    glyph_budget: Option<u64>,
    processed: crate::analysis::whitespace::Processed,
    styles: Vec<InlineStyle>,
    id: u64,
    combine_spans: Vec<crate::analysis::combine::CombineSpan>,
    breaks: crate::analysis::breaks::BreakAnalysis,
    bidi: crate::analysis::bidi::BidiAnalysis,
    used_direction: Direction,
}

impl ParagraphAnalysis {
    pub(crate) fn from_builder(b: ParagraphBuilder) -> Result<Self, LimitExceeded> {
        if let Some(error) = b.error {
            return Err(error);
        }
        let mut budget = crate::ruby::prepare::RubyBudget::new(!b.rubies.is_empty());
        budget.enter(&b.limits);
        match Paragraph::analyze_builder_with_ruby_budget(b, &mut budget) {
            Ok(state) => Ok(Self { state, budget }),
            Err(error) => {
                budget.leave();
                Err(error)
            }
        }
    }

    /// Shape this analysis using the supplied scratch context and font layers.
    pub fn shape(
        self,
        cx: &mut crate::LayoutContext,
        fonts: &FontCollection,
    ) -> Result<Paragraph, LimitExceeded> {
        // A build can replace shaping/edge caches used by a retained trial.
        cx.completed = None;
        let Self { state, mut budget } = self;
        let result = Paragraph::shape_analysis(state, cx, fonts, &mut budget);
        budget.leave();
        result
    }
}

impl Paragraph {
    pub(crate) fn from_builder_with_ruby_budget(
        b: ParagraphBuilder,
        cx: &mut crate::LayoutContext,
        fonts: &FontCollection,
        budget: &mut crate::ruby::prepare::RubyBudget,
    ) -> Result<Paragraph, LimitExceeded> {
        if let Some(error) = b.error {
            return Err(error);
        }
        budget.enter(&b.limits);
        let result = Self::analyze_builder_with_ruby_budget(b, budget)
            .and_then(|state| Self::shape_analysis(state, cx, fonts, budget));
        budget.leave();
        result
    }

    fn analyze_builder_with_ruby_budget(
        b: ParagraphBuilder,
        budget: &mut crate::ruby::prepare::RubyBudget,
    ) -> Result<BuildAnalysis, LimitExceeded> {
        let ruby_first_line = b.has_ruby_first_line();
        let mut bases = crate::ruby::base_budget::BaseScopes::new(&b.rubies);
        let ParagraphBuilder {
            mut style,
            limits,
            text,
            mut items,
            mut styles,
            mut first_line_styles,
            mut warnings,
            offset_mapping,
            line_break_override,
            rubies,
            ruby_annotation,
            ..
        } = b;
        for s in &mut styles {
            s.font_size = sanitize_font_size(s.font_size, &mut warnings);
            sanitize::style(s, &mut warnings);
        }
        // The root inline box's interned style (index 0) is the one layout
        // reads; keep the stored paragraph style consistent with it.
        if let Some(root) = styles.first() {
            style.root = root.clone();
        }
        sanitize::items(&mut items, &mut warnings);
        let id = NEXT_PARAGRAPH_ID.fetch_add(1, Ordering::Relaxed);
        let has_first_line =
            style.first_line.is_some() || !first_line_styles.is_empty() || ruby_first_line;
        bases.check_style_sets(has_first_line)?;
        if budget.enabled() {
            budget.check(
                LimitKind::Styles,
                styles.len() as u64 * if has_first_line { 2 } else { 1 },
            )?;
        }
        if has_first_line {
            Limits::check(
                limits.max_styles,
                LimitKind::Styles,
                styles.len() as u64 * 2,
            )?;
        }
        let normal_bytes = crate::style::memory::paragraph(&style)
            .saturating_add(crate::style::memory::styles(&styles));
        let alternate_sizes = has_first_line.then(|| {
            let first = style.first_line.as_ref().unwrap_or(&style.root);
            styles
                .iter()
                .enumerate()
                .map(|(i, original)| match first_line_styles.get(&(i as u32)) {
                    Some(resolved) => {
                        crate::style::memory::alternate(original, &style.root, resolved, false)
                    }
                    None => crate::style::memory::alternate(original, &style.root, first, true),
                })
                .collect::<Vec<_>>()
        });
        let alternate_bytes = alternate_sizes.as_ref().map_or(0, |sizes| {
            sizes
                .iter()
                .copied()
                .fold(0u64, u64::saturating_add)
                .saturating_add(sizes[0]) // alternate ParagraphStyle root
        });
        let style_bytes = normal_bytes.saturating_add(alternate_bytes);
        Limits::check(limits.max_style_bytes, LimitKind::StyleBytes, style_bytes)?;
        budget.check(LimitKind::StyleBytes, style_bytes)?;
        bases.check_style_bytes(&styles, alternate_sizes.as_deref())?;
        let alternate_styles = has_first_line.then(|| {
            let first = style.first_line.as_ref().unwrap_or(&style.root);
            styles
                .iter()
                .enumerate()
                .map(
                    |(index, original)| match first_line_styles.remove(&(index as u32)) {
                        Some(resolved) => resolved_first_line_style(original, &resolved),
                        None => first_line_style(original, &style.root, first),
                    },
                )
                .collect::<Vec<_>>()
        });
        let run_limits = budget.remaining(&limits);

        // The shared transient input is independently bounded; transformed
        // retained output uses the remaining aggregate cap, as for first-line.
        let mut input_limits = run_limits.clone();
        input_limits.max_text_bytes = limits.max_text_bytes;
        bases.start_pass(false)?;
        let processed = crate::analysis::whitespace::process_with_base_scopes(
            &text,
            &items,
            &styles,
            offset_mapping,
            &input_limits,
            ruby_annotation,
            &mut bases,
        )
        .map_err(|error| bases.translate(error, budget))?;
        let source_cuts = alternate_styles
            .as_ref()
            .map(|_| crate::analysis::breaks::source_cursor_ranges(&processed));
        let mut processed = transform_with_base_scopes(
            processed,
            &styles,
            &run_limits,
            &mut warnings,
            style.writing_mode,
            if bases.enabled() {
                Some(&mut bases)
            } else {
                None
            },
        )
        .map_err(|error| bases.translate(error, budget))?;
        if alternate_styles.is_none() {
            processed.source_spans = Vec::new();
        }
        let normal = analyze_data(
            style.clone(),
            limits.clone(),
            run_limits.max_shaped_glyphs,
            processed,
            styles,
            id,
            &mut warnings,
            true,
            line_break_override.as_deref(),
        );
        budget.paragraph_analysis(
            normal.processed.text.len() as u64,
            normal.processed.items.len() as u64,
            normal.styles.len() as u64,
            crate::style::memory::paragraph(&normal.style)
                .saturating_add(crate::style::memory::styles(&normal.styles)),
        )?;

        let mut alternate_warnings = None;
        let (alternate, alternate_bases) = if let Some(mut alternate_styles) = alternate_styles {
            let mut deferred_warnings = WarningSink::new(warnings.remaining_limit());
            for s in &mut alternate_styles {
                s.font_size = sanitize_font_size(s.font_size, &mut deferred_warnings);
                sanitize::style(s, &mut deferred_warnings);
            }
            let mut alternate_style = style.clone();
            alternate_style.root = alternate_styles[0].clone();
            alternate_style.first_line = None;
            let alternate_style_bytes = crate::style::memory::paragraph(&alternate_style)
                .saturating_add(crate::style::memory::styles(&alternate_styles));
            Limits::check(
                limits.max_style_bytes,
                LimitKind::StyleBytes,
                crate::style::memory::paragraph(&style)
                    .saturating_add(crate::style::memory::styles(&normal.styles))
                    .saturating_add(alternate_style_bytes),
            )?;
            budget.paragraph_analysis(
                0,
                0,
                alternate_styles.len() as u64,
                alternate_style_bytes,
            )?;

            let mut remaining = budget.remaining(&limits);
            if !budget.enabled() {
                remaining.max_text_bytes = limits
                    .max_text_bytes
                    .map(|max| max.saturating_sub(normal.processed.text.len() as u64));
                remaining.max_items = limits
                    .max_items
                    .map(|max| max.saturating_sub(normal.processed.items.len() as u64));
            }
            // Bound source processing independently, then enforce the aggregate
            // retained-output budget on every alternate append.
            let mut input_limits = remaining.clone();
            input_limits.max_text_bytes = limits.max_text_bytes;
            let mut alternate_bases = bases.clone();
            alternate_bases.start_pass(true)?;
            let alternate = crate::analysis::whitespace::process_with_base_scopes(
                &text,
                &items,
                &normal.styles,
                offset_mapping,
                &input_limits,
                ruby_annotation,
                &mut alternate_bases,
            )
            .and_then(|processed| {
                transform_with_base_scopes(
                    processed,
                    &alternate_styles,
                    &remaining,
                    &mut deferred_warnings,
                    style.writing_mode,
                    if alternate_bases.enabled() {
                        Some(&mut alternate_bases)
                    } else {
                        None
                    },
                )
            })
            .map_err(|mut error| {
                if budget.enabled() {
                    return alternate_bases.translate(error, budget);
                }
                if error.kind == LimitKind::TextBytes
                    && let Some(limit) = limits.max_text_bytes
                {
                    error.actual += normal.processed.text.len() as u64;
                    error.limit = limit;
                }
                if error.kind == LimitKind::Items
                    && let Some(limit) = limits.max_items
                {
                    error.actual += normal.processed.items.len() as u64;
                    error.limit = limit;
                }
                error
            })?;
            let alternate = analyze_data(
                alternate_style,
                limits.clone(),
                remaining.max_shaped_glyphs,
                alternate,
                alternate_styles,
                id,
                &mut deferred_warnings,
                false,
                line_break_override.as_deref(),
            );
            budget.paragraph_analysis(
                alternate.processed.text.len() as u64,
                alternate.processed.items.len() as u64,
                0,
                0,
            )?;
            alternate_warnings = Some(deferred_warnings);
            (Some(alternate), Some(alternate_bases))
        } else {
            (None, None)
        };
        Ok(BuildAnalysis {
            normal,
            alternate,
            source_cuts,
            rubies,
            bases,
            alternate_bases,
            alternate_warnings,
            warnings,
        })
    }

    fn shape_analysis(
        analysis: BuildAnalysis,
        cx: &mut crate::LayoutContext,
        fonts: &FontCollection,
        budget: &mut crate::ruby::prepare::RubyBudget,
    ) -> Result<Paragraph, LimitExceeded> {
        let BuildAnalysis {
            normal,
            alternate,
            source_cuts,
            rubies,
            bases: mut normal_bases,
            alternate_bases,
            alternate_warnings,
            mut warnings,
        } = analysis;
        let mut sat = Saturation::default();
        let mut data = build_data(
            normal,
            cx,
            fonts,
            &mut warnings,
            &mut sat,
            &mut normal_bases,
        )
        .map_err(|error| normal_bases.translate(error, budget))?;
        budget.paragraph_shaping(data.glyphs.len() as u64)?;
        let limits = data.limits.clone();
        let mut bases = if let Some(mut alternate) = alternate {
            let mut alternate_bases = alternate_bases.expect("first-line analysis has base scopes");
            alternate_bases.merge_shaped_glyphs_from(&normal_bases)?;
            let mut remaining = budget.remaining(&limits);
            if !budget.enabled() {
                remaining.max_shaped_glyphs = limits
                    .max_shaped_glyphs
                    .map(|max| max.saturating_sub(data.glyphs.len() as u64));
            }
            alternate.glyph_budget = remaining.max_shaped_glyphs;
            warnings.append(alternate_warnings.expect("first-line analysis has deferred warnings"));
            let mut alternate = build_data(
                alternate,
                cx,
                fonts,
                &mut warnings,
                &mut sat,
                &mut alternate_bases,
            )
            .map_err(|mut error| {
                if budget.enabled() {
                    return alternate_bases.translate(error, budget);
                }
                if error.kind == LimitKind::ShapedGlyphs
                    && let Some(limit) = limits.max_shaped_glyphs
                {
                    error.actual += data.glyphs.len() as u64;
                    error.limit = limit;
                }
                error
            })?;
            budget.paragraph_shaping(alternate.glyphs.len() as u64)?;
            finalize_data(&mut data, cx, &mut warnings, &mut sat);
            finalize_data(&mut alternate, cx, &mut warnings, &mut sat);
            let mut normal_search = 0;
            let mut normal_cursors: Vec<_> = alternate
                .units
                .iter()
                .map(|u| {
                    normal_cursor(
                        &data,
                        &alternate,
                        u,
                        source_cuts.as_ref().unwrap(),
                        &mut normal_search,
                    )
                })
                .collect();
            normal_cursors.push(Some(data.units.len() as u32));
            for i in 0..alternate.units.len() {
                if normal_cursors[i + 1].is_some() {
                    continue;
                }
                let class = alternate.units[i].break_after;
                let min_content = alternate.units[i].emergency_min_content;
                alternate.units[i].break_after = crate::analysis::units::BreakClass::Prohibited;
                alternate.units[i].emergency_min_content = false;
                if matches!(
                    class,
                    crate::analysis::units::BreakClass::Allowed
                        | crate::analysis::units::BreakClass::Emergency
                ) {
                    // Preserve the transformed opportunity beyond markers
                    // inside a source grapheme whose trailing scalar was consumed.
                    for j in i + 1..alternate.units.len() {
                        use crate::analysis::units::UnitKind;
                        if !matches!(
                            alternate.units[j].kind,
                            UnitKind::Float { .. }
                                | UnitKind::Absolute { .. }
                                | UnitKind::Open { .. }
                                | UnitKind::Close { .. }
                                | UnitKind::BidiControl
                        ) {
                            break;
                        }
                        if normal_cursors[j + 1].is_some() {
                            if alternate.units[j].break_after
                                == crate::analysis::units::BreakClass::Prohibited
                            {
                                alternate.units[j].break_after = class;
                                alternate.units[j].emergency_min_content = min_content;
                            }
                            break;
                        }
                    }
                }
            }
            data.source_spans = Vec::new();
            alternate.source_spans = Vec::new();
            data.first_line = Some(FirstLineData {
                data: Arc::new(alternate),
                alternate_cursors: normal_cursors
                    .iter()
                    .enumerate()
                    .filter_map(|(i, u)| u.map(|u| (u, i as u32)))
                    .collect(),
                normal_cursors,
            });
            alternate_bases
        } else {
            data.source_spans = Vec::new();
            finalize_data(&mut data, cx, &mut warnings, &mut sat);
            normal_bases
        };
        crate::ruby::prepare::prepare(&mut data, &rubies, cx, fonts, budget, &mut bases)?;
        if let Some(first) = &mut data.first_line {
            let alternate = Arc::get_mut(&mut first.data)
                .expect("new first-line data has one owner before publication");
            crate::ruby::prepare::prepare_alternate(
                alternate,
                &rubies,
                &data.ruby,
                budget,
                &first.normal_cursors,
                &mut bases,
            )?;
        }
        warnings.record_saturation(&sat);
        data.warnings = warnings.take();
        Ok(Paragraph {
            data: Arc::new(data),
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn analyze_data(
    style: ParagraphStyle,
    limits: Limits,
    glyph_budget: Option<u64>,
    processed: crate::analysis::whitespace::Processed,
    styles: Vec<InlineStyle>,
    id: u64,
    warnings: &mut WarningSink,
    report_combine_rejections: bool,
    line_break_override: Option<&crate::analysis::breaks::OverrideCallback>,
) -> PreparedBuild {
    let (combine_spans, combine_rejected) = crate::analysis::combine::prepare_with_rejections(
        &processed.text,
        &processed.items,
        &styles,
        style.writing_mode,
    );
    // First-line builds the same combine scopes twice with alternate paint
    // and shaping styles. Report each rejected boundary only on the normal
    // pass, so a duplicate does not consume the caller's warning budget.
    if report_combine_rejections {
        for range in combine_rejected {
            warnings.push(
                crate::limits::WarningKind::Unsupported,
                format!(
                    "text-combine-upright: all was not applied to processed text bytes {}..{}: \
                     a box boundary separates it from an adjacent candidate in the same combine \
                     scope, so it is laid out as normal text",
                    range.start, range.end
                ),
            );
        }
    }
    let mut breaks = crate::analysis::breaks::analyze_breaks(&processed, &styles, warnings);
    for opportunity in &mut breaks.opportunities {
        let index = combine_spans.partition_point(|span| span.text.end <= opportunity.offset);
        if let Some(span) = combine_spans.get(index)
            && span.text.start < opportunity.offset
        {
            opportunity.class = crate::analysis::units::BreakClass::Prohibited;
            opportunity.min_content = false;
        }
    }
    if let Some(callback) = line_break_override {
        breaks.apply_override(&processed, &styles, &combine_spans, callback);
    }
    let used_direction = crate::analysis::bidi::used_root_direction(&style, &styles[0]);
    let bidi_text = crate::analysis::bidi::upright_analysis_text(
        &processed,
        &styles,
        style.writing_mode,
        &combine_spans,
    );
    let bidi = analyze_bidi(
        bidi_text.as_deref().unwrap_or(&processed.text),
        &style,
        &styles,
        used_direction,
    );

    PreparedBuild {
        style,
        limits,
        glyph_budget,
        processed,
        styles,
        id,
        combine_spans,
        breaks,
        bidi,
        used_direction,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_data(
    prepared: PreparedBuild,
    cx: &mut crate::LayoutContext,
    fonts: &FontCollection,
    warnings: &mut WarningSink,
    sat: &mut Saturation,
    bases: &mut crate::ruby::base_budget::BaseScopes,
) -> Result<ParagraphData, LimitExceeded> {
    let PreparedBuild {
        style,
        limits,
        glyph_budget,
        processed,
        styles,
        id,
        mut combine_spans,
        breaks,
        bidi,
        used_direction,
    } = prepared;
    let mut shape_limits = limits.clone();
    shape_limits.max_shaped_glyphs = glyph_budget;
    let mut shape_items_input = crate::analysis::itemize::itemize(
        &processed,
        &styles,
        &bidi,
        &breaks,
        fonts,
        style.writing_mode,
        &combine_spans,
    );
    crate::shape::select_combined_widths(
        cx,
        &mut shape_items_input,
        &styles,
        fonts,
        style.writing_mode,
        &shape_limits,
    );
    let (glyphs, runs) = shape_items_with_base_scopes(
        cx,
        &shape_items_input,
        &styles,
        fonts,
        style.writing_mode,
        &shape_limits,
        warnings,
        sat,
        if bases.enabled() { Some(bases) } else { None },
    )?;
    let base_level = u8::from(used_direction == Direction::Rtl);
    let style_metrics: Vec<_> = styles
        .iter()
        .map(|s| crate::line::font_metrics::resolve(fonts, s, warnings))
        .collect();
    let combine_geometry = crate::analysis::combine::geometry(
        &processed.text,
        &processed.items,
        &styles,
        &combine_spans,
        &glyphs,
        &runs,
        fonts,
        &style_metrics,
        sat,
    );
    let UnitList {
        mut units,
        mut boxes,
        float_count,
    } = build_units(
        &processed.text,
        &processed.items,
        &runs,
        &glyphs,
        &bidi.levels,
        base_level,
        &breaks,
    );
    // Boxes are emitted in parent-before-child order. Keep only the jump to
    // the nearest Clone; width still rounds and adds each edge inside-out.
    for i in 0..boxes.len() {
        boxes[i].nearest_clone = if styles[boxes[i].style as usize].box_decoration_break
            == crate::style::BoxDecorationBreak::Clone
        {
            Some(i as u32)
        } else {
            boxes[i]
                .parent
                .and_then(|parent| boxes[parent as usize].nearest_clone)
        };
    }
    for (i, unit) in units.iter_mut().enumerate() {
        let index = combine_spans.partition_point(|span| span.text.end <= unit.text.start);
        if let Some(span) = combine_spans.get_mut(index)
            && span.text.start <= unit.text.start
        {
            if matches!(
                unit.kind,
                crate::analysis::units::UnitKind::Cluster { .. }
                    | crate::analysis::units::UnitKind::Tab
            ) {
                unit.combine = Some(index as u32);
                if span.units.is_empty() {
                    span.units.start = i;
                }
                span.units.end = i + 1;
                unit.level = bidi.levels[span.text.start as usize];
            }
            if unit.text.end < span.text.end {
                unit.break_after = crate::analysis::units::BreakClass::Prohibited;
                unit.emergency_min_content = false;
            }
        }
    }
    for span in &combine_spans {
        for unit in &mut units[span.units.clone()] {
            if unit.combine.is_some() {
                unit.slice_advance = crate::geometry::LayoutUnit::ZERO;
            }
        }
        if !span.units.is_empty() {
            units[span.units.end - 1].slice_advance =
                crate::geometry::LayoutUnit::from_f32_round(span.em, sat);
        }
    }
    let mut baselines = HashMap::new();
    for item in &processed.items {
        if let ItemKind::Atomic { parent_style, .. } = item.kind
            && let Some(node) = item.node
        {
            baselines.entry(node).or_insert_with(|| {
                baseline_kind(style.writing_mode, &styles[parent_style as usize])
            });
        }
    }
    let data = ParagraphData {
        ruby: Default::default(),
        #[cfg(test)]
        spacing_setup_visits: Default::default(),
        #[cfg(test)]
        baseline_queries: Default::default(),
        #[cfg(test)]
        cluster_queries: Default::default(),
        #[cfg(test)]
        cursor_queries: Default::default(),
        #[cfg(test)]
        window_queries: Default::default(),
        #[cfg(test)]
        edge_shape_calls: Default::default(),
        #[cfg(test)]
        edge_shape_bytes: Default::default(),
        first_line: None,
        source_spans: processed.source_spans,
        id,
        style,
        limits: limits.clone(),
        text: processed.text,
        items: processed.items,
        styles,
        combine_spans,
        combine_geometry,
        style_metrics,
        unit_spacing: Vec::new(),
        punctuation: Vec::new(),
        last_content_unit: None,
        internal_autospace_gaps: Vec::new(),
        needs_spacing: false,
        spacing_tree: Default::default(),
        glyphs,
        clusters: Vec::new(),
        glyph_clusters: Vec::new(),
        selectable_clusters: Vec::new(),
        shaping_barriers: Vec::new(),
        floats: Vec::new(),
        runs,
        shape_items: shape_items_input,
        breaks,
        units,
        boxes,
        float_count,
        base_level,
        bidi_paragraphs: bidi.paragraphs,
        mapping: processed.mapping,
        fonts: fonts.clone(),
        generations: fonts.generations(),
        font_layer: fonts.layer_handle(),
        warnings: Vec::new(),
        baselines,
    };
    Ok(data)
}

fn finalize_data(
    data: &mut ParagraphData,
    cx: &mut crate::LayoutContext,
    warnings: &mut WarningSink,
    sat: &mut Saturation,
) {
    crate::line::reshape::initialize_slices(data, cx, warnings, sat);
    data.spacing_tree = crate::line::autospace::Tree::build(data);
    data.punctuation = crate::line::punctuation::build(data, sat);
    (data.unit_spacing, data.internal_autospace_gaps) = crate::line::spacing::build(data, sat);
    data.last_content_unit = crate::line::spacing::last_content(data);
    data.needs_spacing = crate::line::spacing::needed(data);
    let mut clusters = Vec::new();
    let mut glyph_clusters = vec![0; data.glyphs.len()];
    let mut floats = Vec::new();
    let mut selectable_clusters = Vec::new();
    let mut shaping_barriers = Vec::new();
    let mut previous_cluster = None;
    for (i, u) in data.units.iter().enumerate() {
        match &u.kind {
            crate::analysis::units::UnitKind::Cluster { glyphs, .. } => {
                selectable_clusters.push(i as u32);
                let same = previous_cluster
                    .is_some_and(|previous: usize| u.shares_cluster(&data.units[previous]));
                previous_cluster = Some(i);
                if same {
                    continue;
                }
                let cluster = clusters.len() as u32;
                clusters.push(i as u32);
                glyph_clusters[glyphs.start as usize..glyphs.end as usize].fill(cluster);
            }
            crate::analysis::units::UnitKind::Float { node, .. } => floats.push((i as u32, *node)),
            crate::analysis::units::UnitKind::Atomic { .. }
            | crate::analysis::units::UnitKind::ForcedBreak
            | crate::analysis::units::UnitKind::BlockInInline { .. }
            | crate::analysis::units::UnitKind::Tab
            | crate::analysis::units::UnitKind::BidiControl => shaping_barriers.push(i as u32),
            crate::analysis::units::UnitKind::Open { box_index }
            | crate::analysis::units::UnitKind::Close { box_index } => {
                let b = &data.boxes[*box_index as usize];
                let start = matches!(u.kind, crate::analysis::units::UnitKind::Open { .. });
                if crate::analysis::itemize::inline_boundary_breaks_shaping(
                    &data.styles[b.style as usize],
                    &b.edges,
                    start,
                ) {
                    shaping_barriers.push(i as u32);
                }
            }
            _ => {}
        }
    }
    data.shaping_barriers = shaping_barriers;
    data.clusters = clusters;
    data.selectable_clusters = selectable_clusters;
    data.glyph_clusters = glyph_clusters;
    data.floats = floats;
}

fn normal_cursor(
    normal: &ParagraphData,
    alternate: &ParagraphData,
    u: &Unit,
    source_cuts: &[std::ops::RangeInclusive<u32>],
    search: &mut usize,
) -> Option<u32> {
    use crate::analysis::units::UnitKind;
    use crate::mapping::TransformSpan;
    let source = TransformSpan::source_position(&alternate.source_spans, u.text.start);
    let cut = source_cuts.partition_point(|cut| *cut.end() < source);
    if !source_cuts
        .get(cut)
        .is_some_and(|cut| cut.contains(&source))
    {
        return None;
    }
    if TransformSpan::map_position(&alternate.source_spans, source) != u.text.start {
        return None;
    }
    let pos = TransformSpan::map_position(&normal.source_spans, source);
    if TransformSpan::source_position(&normal.source_spans, pos) != source {
        return None;
    }
    // Both sets preserve source/item order. Exact mapped cuts are monotone,
    // including empty markers sharing a source offset, so never restart a group.
    while let Some(n) = normal.units.get(*search) {
        #[cfg(test)]
        normal.cursor_queries.fetch_add(1, Ordering::Relaxed);
        if n.text.start > pos {
            return None;
        }
        if n.text.start == pos
            && match (&n.kind, &u.kind) {
                (UnitKind::Cluster { .. }, UnitKind::Cluster { .. }) => true,
                _ => {
                    n.item == u.item
                        && std::mem::discriminant(&n.kind) == std::mem::discriminant(&u.kind)
                }
            }
        {
            return Some(*search as u32);
        }
        *search += 1;
    }
    None
}

/// The dominant baseline of a parent inline box (CSS Writing Modes 4 §4.2):
/// central in vertical typographic modes unless the text is set sideways.
fn baseline_kind(writing_mode: WritingMode, parent: &InlineStyle) -> BaselineKind {
    match writing_mode {
        WritingMode::VerticalRl | WritingMode::VerticalLr
            if parent.text_orientation != TextOrientation::Sideways =>
        {
            BaselineKind::Central
        }
        _ => BaselineKind::Alphabetic,
    }
}

fn sanitize_font_size(size: f32, warnings: &mut crate::limits::WarningSink) -> f32 {
    if !size.is_finite() {
        warnings.push(
            WarningKind::NonFiniteInput,
            "non-finite font-size replaced with 0",
        );
        0.0
    } else if size < 0.0 {
        warnings.push(
            WarningKind::NegativeInput,
            "negative font-size replaced with 0",
        );
        0.0
    } else if size > MAX_FONT_SIZE {
        warnings.push(WarningKind::Saturated, "font-size clamped to 1e6 px");
        MAX_FONT_SIZE
    } else {
        size
    }
}
