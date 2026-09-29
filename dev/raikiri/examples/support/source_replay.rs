//! Original screen DOM to ordinary shodo IFC input for source diagnostics.
//! The typed guard covers the observed input footprint, not general CSS.
use super::{diagnostic, offline::ScreenInput};
use raikiri_style::{ComputedValues, property as css};
use raikiri_traits::{Dom, NodeId as DomId};
use shodo::{LayoutContext, Paragraph, ParagraphBuilder};
use shodo::{
    font::FontCollection,
    limits::Limits,
    node::{InlineEdges, NodeId, TextSource},
    style as s,
};

pub struct Prepared {
    pub paragraph: Paragraph,
    pub options: s::LineOptions,
}

fn text_style(
    cv: &ComputedValues,
    profile: diagnostic::InputProfile,
) -> Result<s::InlineStyle, String> {
    let prepared = diagnostic::prepare_input(cv, profile);
    let initial = ComputedValues::initial();
    let mut remaining = prepared.clone();
    // These are the nine noninitial fields observed across all 167 original
    // IFCs. Root sizing is already retained by the original measured width.
    remaining.font_family = initial.font_family;
    remaining.font_size = initial.font_size;
    remaining.font_weight = initial.font_weight;
    remaining.line_height = initial.line_height;
    remaining.display = initial.display;
    remaining.direction = initial.direction;
    remaining.text_align = initial.text_align;
    remaining.text_autospace = initial.text_autospace;
    remaining.word_break = initial.word_break;
    if remaining != ComputedValues::initial() {
        return Err("computed style outside the verified ordinary-source input footprint".into());
    }
    let font_families = cv
        .font_family
        .iter()
        .map(|f| {
            if f.1 == css::FontFamilyKind::Named {
                return Ok(s::FontFamily::Named(f.as_str().into()));
            }
            let generic = match f.as_str() {
                "serif" => s::GenericFamily::Serif,
                "sans-serif" => s::GenericFamily::SansSerif,
                "monospace" => s::GenericFamily::Monospace,
                "cursive" => s::GenericFamily::Cursive,
                "fantasy" => s::GenericFamily::Fantasy,
                "system-ui" => s::GenericFamily::SystemUi,
                _ => return Err("font generic outside the pinned registry".into()),
            };
            Ok(s::FontFamily::Generic(generic))
        })
        .collect::<Result<_, String>>()?;
    let line_height = match cv.line_height {
        raikiri_style::ComputedLineHeight::Normal => s::LineHeight::Normal,
        raikiri_style::ComputedLineHeight::Number(value) => s::LineHeight::Number(value),
        raikiri_style::ComputedLineHeight::Length(value) => s::LineHeight::Px(value.0),
    };
    let direction = match cv.direction {
        css::Direction::Ltr => shodo::geometry::Direction::Ltr,
        css::Direction::Rtl => shodo::geometry::Direction::Rtl,
        _ => return Err("unsupported source direction".into()),
    };
    let text_autospace = match cv.text_autospace {
        css::TextAutospace::Normal => s::TextAutospace::Normal,
        css::TextAutospace::NoAutospace => s::TextAutospace::NoAutospace,
        _ => return Err("unsupported source autospace".into()),
    };
    let word_break = match cv.word_break {
        css::WordBreak::Normal => s::WordBreak::Normal,
        css::WordBreak::KeepAll => s::WordBreak::KeepAll,
        _ => return Err("word-break outside the verified source input footprint".into()),
    };
    Ok(s::InlineStyle {
        font_families,
        font_size: cv.font_size.0,
        font_weight: cv.font_weight,
        line_height,
        direction,
        text_autospace,
        word_break,
        ..Default::default()
    })
}

pub fn project(
    input: &ScreenInput,
    root: usize,
    width: f32,
    context: &mut LayoutContext,
    fonts: &FontCollection,
    limits: &Limits,
) -> Result<Prepared, String> {
    if !width.is_finite() || width < 0.0 {
        return Err("invalid original measured content width".into());
    }
    let dom = &input.parsed.dom;
    let values = &input.cascade.computed;
    let root_node = dom.get_node(root).ok_or("missing original root")?;
    let root_cv = values.get(root).ok_or("missing root computed style")?;
    if !root_node.is_in_document()
        || root_node.kind() != raikiri_dom::NodeKind::Element
        || root_cv.display != css::DisplayValue::Block
    {
        return Err("source root is not an in-document block".into());
    }
    let mut parents = vec![None; dom.node_count()];
    for id in 0..dom.node_count() {
        for child in dom.child_ids(DomId(id as u64)) {
            parents[child.0 as usize] = Some(id);
        }
    }
    let language = |id| {
        let mut cursor = Some(id);
        while let Some(id) = cursor {
            if let Some(lang) = dom.element_attribute(id, "lang") {
                return if lang.is_empty() {
                    None
                } else {
                    Some(lang.to_owned())
                };
            }
            cursor = parents[id];
        }
        None
    };
    let mut root_style = text_style(root_cv, diagnostic::InputProfile::MeasuredBlock)?;
    root_style.lang = language(root);
    let paragraph_style = s::ParagraphStyle {
        direction: root_style.direction,
        root: root_style,
        ..Default::default()
    };
    let options = s::LineOptions {
        text_align: match root_cv.text_align {
            css::TextAlign::Start => s::TextAlign::Start,
            css::TextAlign::End => s::TextAlign::End,
            css::TextAlign::Left => s::TextAlign::Left,
            css::TextAlign::Right => s::TextAlign::Right,
            _ => return Err("text-align outside the verified source input footprint".into()),
        },
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, limits);
    // An explicit enter/leave stack preserves nested inline styles and the
    // original DOM order. Separate blocks are handed off without descent.
    let mut stack: Vec<_> = dom
        .child_ids(DomId(root as u64))
        .map(|id| Some(id.0 as usize))
        .collect();
    stack.reverse();
    let mut visited = vec![root];
    while let Some(event) = stack.pop() {
        let Some(id) = event else {
            builder.close_inline();
            continue;
        };
        let node = dom.get_node(id).ok_or("missing original child")?;
        let cv = values.get(id).ok_or("missing child computed style")?;
        if !node.is_in_document() || cv.display == css::DisplayValue::None {
            continue;
        }
        if node.kind() == raikiri_dom::NodeKind::Element
            && cv.display == css::DisplayValue::Block
            && cv.float == css::FloatValue::None
            && matches!(
                cv.position,
                css::PositionValue::Static | css::PositionValue::Relative
            )
        {
            builder.push_block_in_inline(NodeId(id as u64));
            continue;
        }
        visited.push(id);
        let mut style = text_style(cv, diagnostic::InputProfile::Plain)
            .map_err(|e| format!("node {id}: {e}"))?;
        style.lang = language(id);
        match node.kind() {
            raikiri_dom::NodeKind::Text => {
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(id as u64),
                        offset: 0,
                    },
                    node.text_content().ok_or("text node has no text")?,
                );
            }
            raikiri_dom::NodeKind::Element if cv.display == css::DisplayValue::Inline => {
                builder.open_inline(NodeId(id as u64), &style, InlineEdges::default());
                if node.tag_name() == Some("br") {
                    builder.push_forced_break(NodeId(id as u64));
                    builder.close_inline();
                } else {
                    stack.push(None);
                    let children: Vec<_> = dom.child_ids(DomId(id as u64)).collect();
                    stack.extend(children.into_iter().rev().map(|id| Some(id.0 as usize)));
                }
            }
            _ => {
                return Err(format!(
                    "node {id}: geometry outside the verified source input footprint"
                ));
            }
        }
    }
    // Active generated content cannot be silently omitted from a source audit.
    for ((id, _), cv) in &input.cascade.pseudo {
        if visited.contains(&(id.0 as usize))
            && cv.display != css::DisplayValue::None
            && !cv.content.is_empty()
        {
            return Err("generated content outside the verified source input footprint".into());
        }
    }
    let paragraph = builder
        .build(context, fonts)
        .map_err(|e| format!("{e:?}"))?;
    Ok(Prepared { paragraph, options })
}
