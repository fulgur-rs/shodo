//! Development caller for real CSS first-line styles on one immutable DOM.
//! Layout and paint are shared by the fixture CLI and offline WPT probe.
use shodo_harness::glyph_paint;
use std::{collections::HashMap, ops::Range};

use raikiri_html::{ParseOptions, UncascadedDocument, parse};
use raikiri_style::{CascadeResult, ComputedValues, FirstLineStyles, MediaContext, StyleNodeId};
use raikiri_traits::{Dom, NodeId as DomId};
use shodo::hit::{LineLayout, TextPosition};
use shodo::style::{
    FontFamily, InlineStyle, LineHeight, PaintStyle, ParagraphStyle, TextDecoration, TextTransform,
    TextWrapMode, WhiteSpaceCollapse,
};
use shodo::{AtomicSizes, LayoutContext, Line, Paragraph, ParagraphBuilder};
use shodo::{font::FontCollection, geometry::LogicalRect, limits::Limits};
use shodo::{
    mapping::{Affinity, MappingKind},
    node::{NodeId, TextSource},
};

pub struct ResolvedInput {
    parsed: UncascadedDocument,
    normal: CascadeResult,
    first: Option<FirstLineStyles>,
    root: StyleNodeId,
}

/// Resolve actual CSS against the original DOM, without root overrides.
pub fn resolve_html(html: &str, root_id: &str) -> Result<ResolvedInput, String> {
    let parsed = parse(
        html.as_bytes(),
        &ParseOptions {
            extra_stylesheets: &[],
            network: None,
            base_url: None,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    let root = (0..parsed.dom.node_count())
        .find(|&id| parsed.dom.get_node(id).unwrap().attribute("id") == Some(root_id))
        .ok_or("missing IFC root")?;
    resolve_document(parsed, StyleNodeId(root as u64), &MediaContext::default())
}

/// Consume an unchanged document, including its original CSS and media.
pub fn resolve_document(
    parsed: UncascadedDocument,
    root: StyleNodeId,
    media: &MediaContext,
) -> Result<ResolvedInput, String> {
    let tree = raikiri_html::build_rule_tree(&parsed);
    let resolved = raikiri_style::cascade_with_first_line(&parsed.dom, &tree, media, root)
        .map_err(|e| format!("{e:?}"))?;
    Ok(ResolvedInput {
        parsed,
        normal: resolved.normal,
        first: resolved.first_line,
        root,
    })
}

impl ResolvedInput {
    pub fn has_first_line(&self) -> bool {
        self.first.is_some()
    }
}

#[derive(Clone, Copy)]
pub enum FontPolicy {
    FixtureLatin,
    BundledWpt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub element: NodeId,
    pub href: String,
}

#[derive(Clone, Debug)]
pub struct LinkRegion {
    pub line: usize,
    pub node: NodeId,
    pub dom: Range<u32>,
    pub text: Range<u32>,
    pub kind: MappingKind,
    pub link: Link,
    pub rect: LogicalRect,
}

pub struct Output {
    pub lines: Vec<Line>,
    pub links: Vec<LinkRegion>,
}

impl Output {
    /// Container-relative logical coordinates; excludes the PNG's 10px margin.
    pub fn link_at(&self, inline: f32, block: f32) -> Option<&Link> {
        self.links
            .iter()
            .find(|r| {
                inline >= r.rect.inline_start
                    && inline < r.rect.inline_start + r.rect.inline_size
                    && block >= r.rect.block_start
                    && block < r.rect.block_start + r.rect.block_size
            })
            .map(|r| &r.link)
    }
}

fn rgba(c: raikiri_style::property::CssColor) -> [u8; 4] {
    [c.r, c.g, c.b, c.a]
}

/// Accept the mapped inline footprint explicitly. Custom-property environments
/// are already resolved by raikiri and are intentionally not paint inputs.
fn validate_projection(cv: &ComputedValues, root: bool) -> Result<(), String> {
    let initial = ComputedValues::initial();
    macro_rules! initial_only {
        ($($field:ident),* $(,)?) => { $(
            if cv.$field != initial.$field {
                return Err(format!("unsupported computed value: {}",stringify!($field)));
            }
        )* };
    }
    initial_only!(
        background_color,
        list_style_type,
        list_style_position,
        list_style_image,
        counter_reset,
        counter_increment,
        counter_set,
        content,
        string_set,
        running_templates,
        position,
        text_align,
        hanging_punctuation,
        text_autospace,
        word_space_transform,
        text_spacing_trim,
        text_justify,
        text_align_last,
        writing_mode,
        ruby_position,
        text_indent,
        text_indent_ch_factor,
        text_indent_ch_offset,
        text_indent_ch_font,
        text_indent_ch_inherited,
        text_indent_hanging,
        text_indent_each_line,
        border,
        border_radius,
        box_shadow,
        outline,
        outline_offset,
        top,
        right,
        bottom,
        left,
        overflow,
        text_decoration_skip_ink,
        text_decoration_skip_spaces,
        text_decoration_inset,
        text_decoration_inset_start_ch,
        text_decoration_inset_end_ch,
        text_underline_offset,
        text_underline_position,
        text_emphasis_position,
        text_emphasis_style,
        text_emphasis_color,
        vertical_align,
        font_kerning,
        font_optical_sizing,
        font_variant_emoji,
        font_language_override,
        font_variant_ligatures,
        font_synthesis,
        font_variant_position,
        font_palette,
        font_variant_numeric,
        font_variant_east_asian,
        font_variation_settings,
        font_variant_caps,
        text_combine_upright,
        text_orientation,
        unicode_bidi,
        visibility,
        z_index,
        word_break,
        line_break,
        overflow_wrap,
        letter_spacing_ch_factor,
        letter_spacing_ch_offset,
        letter_spacing_ch_font,
        word_spacing_ch_factor,
        word_spacing_ch_offset,
        word_spacing_ch_font,
        tab_size,
        break_before,
        break_after,
        break_inside,
        float,
        clear,
        text_wrap_style,
        hyphens,
        hyphenate_character,
        hyphenate_limit_chars,
        flex_direction,
        flex_wrap,
        flex_grow,
        flex_shrink,
        flex_basis,
        order,
        justify_content,
        align_content,
        align_items,
        align_self,
        row_gap,
        column_gap,
        quotes,
        quotes_auto,
        text_shadow,
        grid_template_columns,
        grid_template_rows,
        grid_template_areas,
        grid_auto_columns,
        grid_auto_rows,
        grid_auto_flow,
        grid_row_start,
        grid_row_end,
        grid_column_start,
        grid_column_end,
        justify_items,
        justify_self,
        orphans,
        widows,
        background_repeat,
        background_attachment,
        background_clip,
        background_origin,
        background_size,
        background_position,
        background_image,
        object_fit,
        object_position,
        opacity,
        isolation,
        mix_blend_mode,
        mask_image,
        clip_path,
        transform,
        transform_origin,
        transform_origin_z,
        filter,
        table_layout,
        border_collapse,
        border_spacing,
        caption_side,
        empty_cells,
        column_count,
        column_width,
    );
    // The originating block's ordinary geometry belongs to the outer layout.
    // Inline descendants must not require unsupported box geometry.
    if !root {
        initial_only!(
            box_sizing,
            height,
            height_ch,
            margin,
            margin_ch,
            max_height,
            max_width,
            min_block_size,
            min_height,
            min_width,
            padding,
            padding_ch,
            width,
            width_ch,
        );
    }
    Ok(())
}

/// The core first-line projection retains these normal formatting controls.
fn validate_first_line_projection(normal: &InlineStyle, first: &InlineStyle) -> Result<(), String> {
    if normal.text_wrap_mode != first.text_wrap_mode
        || normal.white_space_collapse != first.white_space_collapse
    {
        return Err("caller cannot project first-line wrapping/whitespace changes".into());
    }
    Ok(())
}

fn reject_generated_content(input: &ResolvedInput, id: usize) -> Result<(), String> {
    for pseudo in [
        raikiri_style::PseudoElem::Before,
        raikiri_style::PseudoElem::After,
    ] {
        if let Some(cv) = input.normal.pseudo.get(&(StyleNodeId(id as u64), pseudo))
            && cv.display != raikiri_style::property::DisplayValue::None
            && cv
                .content
                .iter()
                .any(|value| !matches!(value, raikiri_style::property::ContentComponent::None))
        {
            return Err("caller does not project generated before/after content".into());
        }
    }
    Ok(())
}

fn style(
    cv: &ComputedValues,
    inherited_underline: Option<TextDecoration>,
    policy: FontPolicy,
    root: bool,
) -> Result<InlineStyle, String> {
    use raikiri_style::property as css;
    validate_projection(cv, root)?;
    if cv.opacity != 1.0
        || cv.background_color.a != 0
        || cv.background_image != css::BackgroundImage::None
    {
        return Err("caller does not paint opacity or backgrounds".into());
    }
    if cv.direction != css::Direction::Ltr
        || cv.cssom_writing_mode != css::WritingMode::HorizontalTb
    {
        return Err("representative caller requires horizontal LTR input".into());
    }
    if (matches!(policy, FontPolicy::FixtureLatin)
        && (cv.font_family.len() != 1
            || cv.font_family[0].as_str() != shodo_fixtures::FONTS[0].family))
        || cv.font_weight != 400.0
        || cv.font_style != css::FontStyle::Normal
    {
        return Err("representative caller needs its static normal Latin fixture".into());
    }
    if cv.letter_spacing_ch_factor.is_some() || cv.word_spacing_ch_factor.is_some() {
        return Err("caller spacing needs declaring-font ch resolution".into());
    }
    // This pin retains legacy white-space separately from the longhands.
    // Mixed noninitial values need declaration provenance, not a guessed win.
    let (collapse, wrap) = match cv.white_space {
        css::WhiteSpace::Normal => (WhiteSpaceCollapse::Collapse, TextWrapMode::Wrap),
        css::WhiteSpace::Pre if cv.white_space_collapse == css::WhiteSpaceCollapse::Collapse => {
            (WhiteSpaceCollapse::Preserve, TextWrapMode::NoWrap)
        }
        _ => return Err("representative caller supports normal/pre legacy whitespace".into()),
    };
    let spacing = |s| match s {
        raikiri_style::ComputedLetterSpacing::Px(v) => Ok(v),
        _ => Err("representative caller needs absolute spacing".to_owned()),
    };
    let line = cv.text_decoration_line;
    if line.overline || line.line_through || line.blink || line.spelling_error || line.grammar_error
    {
        return Err("representative caller supports solid underline only".into());
    }
    let underline = if line.underline {
        if cv.text_decoration_style != css::TextDecorationStyle::Solid {
            return Err("representative caller supports solid underline only".into());
        }
        let color = match cv.text_decoration_color {
            css::TextDecorationColor::CurrentColor => rgba(cv.color),
            css::TextDecorationColor::Resolved(c) => rgba(c),
            _ => return Err("unsupported decoration color".into()),
        };
        let thickness = match cv.text_decoration_thickness {
            raikiri_style::ComputedTextDecorationThickness::Auto
            | raikiri_style::ComputedTextDecorationThickness::FromFont => None,
            raikiri_style::ComputedTextDecorationThickness::Length(v) => Some(v.0),
        };
        if cv.text_underline_offset != raikiri_style::ComputedTextUnderlineOffset::Auto {
            return Err("representative caller uses font underline offset".into());
        }
        Some(TextDecoration {
            color: Some(color),
            thickness,
            offset: None,
        })
    } else {
        inherited_underline
    };
    Ok(InlineStyle {
        font_families: cv
            .font_family
            .iter()
            .map(|f| {
                if f.1 == css::FontFamilyKind::Named {
                    return Ok(FontFamily::Named(f.as_str().into()));
                }
                let generic = match f.as_str() {
                    "serif" => shodo::style::GenericFamily::Serif,
                    "sans-serif" => shodo::style::GenericFamily::SansSerif,
                    "monospace" => shodo::style::GenericFamily::Monospace,
                    "cursive" => shodo::style::GenericFamily::Cursive,
                    "fantasy" => shodo::style::GenericFamily::Fantasy,
                    "system-ui" => shodo::style::GenericFamily::SystemUi,
                    _ => return Err("font generic outside bundled registry".into()),
                };
                Ok(FontFamily::Generic(generic))
            })
            .collect::<Result<_, String>>()?,
        font_size: cv.font_size.0,
        font_weight: cv.font_weight,
        letter_spacing: spacing(cv.letter_spacing_computed)?,
        word_spacing: spacing(cv.word_spacing_computed)?,
        paint: PaintStyle {
            color: rgba(cv.color),
            underline,
            ..Default::default()
        },
        line_height: match cv.line_height {
            raikiri_style::ComputedLineHeight::Normal => LineHeight::Normal,
            raikiri_style::ComputedLineHeight::Number(v) => LineHeight::Number(v),
            raikiri_style::ComputedLineHeight::Length(v) => LineHeight::Px(v.0),
        },
        white_space_collapse: match cv.white_space_collapse {
            css::WhiteSpaceCollapse::Collapse => collapse,
            css::WhiteSpaceCollapse::Preserve => WhiteSpaceCollapse::Preserve,
            _ => return Err("representative caller supports collapse/preserve whitespace".into()),
        },
        text_wrap_mode: match cv.text_wrap {
            css::TextWrapMode::Wrap => wrap,
            css::TextWrapMode::Nowrap => TextWrapMode::NoWrap,
            _ => return Err("unsupported text-wrap mode".into()),
        },
        text_transform: match cv.text_transform {
            css::TextTransform::None => TextTransform::None,
            css::TextTransform::Uppercase => TextTransform::Uppercase,
            css::TextTransform::Lowercase => TextTransform::Lowercase,
            css::TextTransform::Capitalize => TextTransform::Capitalize,
            _ => return Err("unsupported text transform".into()),
        },
        ..Default::default()
    })
}

struct Walker<'a> {
    input: &'a ResolvedInput,
    builder: ParagraphBuilder,
    sources: HashMap<NodeId, Option<Link>>,
    policy: FontPolicy,
}

impl Walker<'_> {
    fn walk(
        &mut self,
        id: usize,
        link: Option<Link>,
        underline: Option<TextDecoration>,
        first_underline: Option<TextDecoration>,
    ) -> Result<(), String> {
        let node = self
            .input
            .parsed
            .dom
            .get_node(id)
            .ok_or("missing DOM node")?;
        if let Some(text) = node.text_content() {
            let source = NodeId(id as u64);
            self.sources.insert(source, link);
            self.builder.push_text(
                TextSource::Dom {
                    node: source,
                    offset: 0,
                },
                text,
            );
            return Ok(());
        }
        if self.input.normal.computed[id].display == raikiri_style::property::DisplayValue::None {
            return Ok(());
        }
        reject_generated_content(self.input, id)?;
        validate_projection(&self.input.normal.computed[id], false)?;
        if node.tag_name() == Some("br") {
            self.builder.push_forced_break(NodeId(id as u64));
            return Ok(());
        }
        if !matches!(node.tag_name(), Some("span" | "a" | "em")) {
            return Err("representative caller expects span/a/em/br descendants".into());
        }
        if self.input.normal.computed[id].display != raikiri_style::property::DisplayValue::Inline {
            return Err("representative caller expects inline descendants".into());
        }
        let normal = style(
            &self.input.normal.computed[id],
            underline,
            self.policy,
            false,
        )?;
        let link = if node.tag_name() == Some("a") {
            node.attribute("href")
                .map(|href| Link {
                    element: NodeId(id as u64),
                    href: href.into(),
                })
                .or(link)
        } else {
            link
        };
        let first = self
            .input
            .first
            .as_ref()
            .map(|cascade| {
                let cv = cascade.computed[id]
                    .as_ref()
                    .ok_or("missing first-line descendant")?;
                style(cv, first_underline, self.policy, false)
            })
            .transpose()?;
        if let Some(first) = &first {
            validate_first_line_projection(&normal, first)?;
            self.builder.open_inline_with_first_line(
                NodeId(id as u64),
                &normal,
                first,
                Default::default(),
            );
        } else {
            self.builder
                .open_inline(NodeId(id as u64), &normal, Default::default());
        }
        for child in self.input.parsed.dom.child_ids(DomId(id as u64)) {
            self.walk(
                child.0 as usize,
                link.clone(),
                normal.paint.underline,
                first.as_ref().and_then(|s| s.paint.underline),
            )?;
        }
        self.builder.close_inline();
        Ok(())
    }
}

pub fn layout(input: &ResolvedInput, fonts: &FontCollection, width: f32) -> Result<Output, String> {
    layout_with_font_policy(input, fonts, width, FontPolicy::FixtureLatin)
}

pub fn layout_with_font_policy(
    input: &ResolvedInput,
    fonts: &FontCollection,
    width: f32,
    policy: FontPolicy,
) -> Result<Output, String> {
    let mut context = LayoutContext::new();
    let prepared = prepare(input, &mut context, fonts, policy)?;
    let lines = prepared.paragraph.break_all(
        &mut context,
        &Default::default(),
        width,
        &AtomicSizes::EMPTY,
    );
    prepared.output(lines)
}

/// Prepared real CSS/DOM inputs, reusable while input and font generations match.
pub struct PreparedParagraph {
    pub paragraph: Paragraph,
    sources: HashMap<NodeId, Option<Link>>,
}

impl PreparedParagraph {
    /// Assemble source links from these accepted lines, independently of shaping.
    pub fn output(&self, lines: Vec<Line>) -> Result<Output, String> {
        output(lines, &self.sources)
    }
}

pub fn prepare(
    input: &ResolvedInput,
    context: &mut LayoutContext,
    fonts: &FontCollection,
    policy: FontPolicy,
) -> Result<PreparedParagraph, String> {
    let count = input.parsed.dom.node_count();
    if input.normal.computed.len() != count
        || input
            .first
            .as_ref()
            .is_some_and(|c| c.computed.len() != count)
    {
        return Err("resolved cascades must cover this same DOM".into());
    }
    let root = input.root.0 as usize;
    if input.normal.computed[root].display != raikiri_style::property::DisplayValue::Block {
        return Err("representative caller expects a block IFC root".into());
    }
    reject_generated_content(input, root)?;
    let normal = style(&input.normal.computed[root], None, policy, true)?;
    let first = input
        .first
        .as_ref()
        .map(|c| {
            style(
                c.computed[root].as_ref().ok_or("missing first-line root")?,
                normal.paint.underline,
                policy,
                true,
            )
        })
        .transpose()?;
    if let Some(first) = &first {
        validate_first_line_projection(&normal, first)?;
    }
    let paragraph_style = ParagraphStyle {
        root: normal.clone(),
        first_line: first.clone(),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, &Limits::default());
    builder.with_offset_mapping(true);
    let mut walker = Walker {
        input,
        builder,
        sources: HashMap::new(),
        policy,
    };
    for child in input.parsed.dom.child_ids(DomId(root as u64)) {
        walker.walk(
            child.0 as usize,
            None,
            normal.paint.underline,
            first.as_ref().and_then(|s| s.paint.underline),
        )?;
    }
    let paragraph = walker
        .builder
        .build(context, fonts)
        .map_err(|e| format!("{e:?}"))?;
    Ok(PreparedParagraph {
        paragraph,
        sources: walker.sources,
    })
}

// A private optimization of this caller, with a checked fallback for arbitrary mappings.
fn mapping_is_ordered(units: &[shodo::mapping::MappingUnit]) -> bool {
    units.windows(2).all(|pair| {
        pair[0].text.start <= pair[1].text.start && pair[0].text.end <= pair[1].text.end
    })
}
fn accepted_mapping_units(
    units: &[shodo::mapping::MappingUnit],
    accepted: Range<usize>,
    ordered: bool,
) -> &[shodo::mapping::MappingUnit] {
    if accepted.is_empty() {
        return &units[..0];
    }
    if !ordered {
        return units;
    }
    let first = units.partition_point(|unit| unit.text.end <= accepted.start as u32);
    let end = units.partition_point(|unit| unit.text.start < accepted.end as u32);
    &units[first..end]
}

fn output(lines: Vec<Line>, sources: &HashMap<NodeId, Option<Link>>) -> Result<Output, String> {
    let mut index = None;
    let mut links = Vec::new();
    let mut checked_mapping = None;
    for (line_id, line) in lines.iter().enumerate() {
        let accepted = line.text_range();
        let mapping = line
            .offset_mapping()
            .ok_or("missing accepted-line mapping")?;
        if checked_mapping.is_none_or(|(last, _)| !std::ptr::eq(last, mapping)) {
            // Preserve the original error contract even for unaccepted/collapsed sources.
            for unit in mapping.units() {
                if !sources.contains_key(&unit.node) {
                    return Err("mapping source lost its DOM identity".into());
                }
            }
            checked_mapping = Some((mapping, mapping_is_ordered(mapping.units())));
        }
        for unit in accepted_mapping_units(
            mapping.units(),
            accepted.clone(),
            checked_mapping.unwrap().1,
        ) {
            let Some(link) = sources
                .get(&unit.node)
                .ok_or("mapping source lost its DOM identity")?
            else {
                continue;
            };
            if unit.kind == MappingKind::Collapsed {
                continue;
            }
            let start = (accepted.start as u32).max(unit.text.start);
            let end = (accepted.end as u32).min(unit.text.end);
            if start >= end {
                continue;
            }
            let dom = if unit.kind == MappingKind::Identity {
                unit.dom.start + start - unit.text.start..unit.dom.start + end - unit.text.start
            } else {
                unit.dom.clone()
            };
            let index = index.get_or_insert_with(|| LineLayout::new(&lines));
            for rect in index.selection_rects(
                TextPosition {
                    line: line_id,
                    offset: start,
                    affinity: Affinity::Downstream,
                },
                TextPosition {
                    line: line_id,
                    offset: end,
                    affinity: Affinity::Upstream,
                },
            ) {
                if rect.inline_size > 0.0 && rect.block_size > 0.0 {
                    links.push(LinkRegion {
                        line: line_id,
                        node: unit.node,
                        dom: dom.clone(),
                        text: start..end,
                        kind: unit.kind,
                        link: link.clone(),
                        rect,
                    });
                }
            }
        }
    }
    Ok(Output { lines, links })
}

pub fn paint(output: &Output) -> Result<(tiny_skia::Pixmap, usize), glyph_paint::PaintError> {
    glyph_paint::try_paint_styled_on_canvas(&output.lines, 512, 256)
}
#[cfg(test)]
mod mapping_window_tests {
    use super::*;
    use shodo::mapping::MappingUnit;
    fn unit(node: u64, range: Range<u32>) -> MappingUnit {
        MappingUnit {
            kind: MappingKind::Expanded,
            node: NodeId(node),
            dom: 0..1,
            text: range,
        }
    }
    #[test]
    fn overlapping_expansions_collapsed_points_and_touching_boundaries_keep_original_order() {
        let mut units = vec![
            unit(1, 0..2),
            unit(2, 2..4),
            unit(3, 2..4),
            unit(4, 4..4),
            unit(5, 4..8),
            unit(6, 8..10),
        ];
        units[3].kind = MappingKind::Collapsed;
        assert!(mapping_is_ordered(&units));
        assert_eq!(
            accepted_mapping_units(&units, 3..5, true)
                .iter()
                .map(|u| u.node.0)
                .collect::<Vec<_>>(),
            [2, 3, 4, 5]
        );
        assert_eq!(
            accepted_mapping_units(&units, 4..8, true)
                .iter()
                .map(|u| u.node.0)
                .collect::<Vec<_>>(),
            [5]
        );
        assert!(accepted_mapping_units(&units, 4..4, true).is_empty());
        assert!(accepted_mapping_units(&units, 10..12, true).is_empty());
    }
    #[test]
    fn disordered_ranges_fall_back_without_dropping_source_records() {
        for units in [
            vec![unit(1, 5..8), unit(2, 0..4)],
            vec![unit(1, 0..8), unit(2, 2..4)],
        ] {
            assert!(!mapping_is_ordered(&units));
            assert_eq!(accepted_mapping_units(&units, 3..6, false), units);
        }
    }
}
