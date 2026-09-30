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
use shodo::{AtomicSizes, LayoutContext, Line, ParagraphBuilder};
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

fn style(
    cv: &ComputedValues,
    inherited_underline: Option<TextDecoration>,
    policy: FontPolicy,
) -> Result<InlineStyle, String> {
    use raikiri_style::property as css;
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
        let normal = style(&self.input.normal.computed[id], underline, self.policy)?;
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
                style(cv, first_underline, self.policy)
            })
            .transpose()?;
        if let Some(first) = &first {
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
    let normal = style(&input.normal.computed[root], None, policy)?;
    let first = input
        .first
        .as_ref()
        .map(|c| {
            style(
                c.computed[root].as_ref().ok_or("missing first-line root")?,
                normal.paint.underline,
                policy,
            )
        })
        .transpose()?;
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
    let mut context = LayoutContext::new();
    let paragraph = walker
        .builder
        .build(&mut context, fonts)
        .map_err(|e| format!("{e:?}"))?;
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        width,
        &AtomicSizes::EMPTY,
    );
    let index = LineLayout::new(&lines);
    let mut links = Vec::new();
    for (line_id, line) in lines.iter().enumerate() {
        let accepted = line.text_range();
        for unit in line
            .offset_mapping()
            .ok_or("missing accepted-line mapping")?
            .units()
        {
            let Some(link) = walker
                .sources
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
