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

fn hanging_punctuation(value: css::HangingPunctuation) -> Result<s::HangingPunctuation, String> {
    use css::HangingPunctuation as H;
    match value {
        H::None
        | H::First
        | H::Last
        | H::ForceEnd
        | H::AllowEnd
        | H::FirstLast
        | H::FirstForceEnd
        | H::FirstAllowEnd
        | H::ForceEndLast
        | H::AllowEndLast
        | H::FirstForceEndLast
        | H::FirstAllowEndLast => Ok(s::HangingPunctuation {
            first: value.first(),
            last: value.last(),
            force_end: value.force_end(),
            allow_end: value.allow_end(),
        }),
        // Future non-exhaustive variants need an explicit caller mapping.
        _ => Err("unsupported source hanging-punctuation".into()),
    }
}

fn text_style(
    cv: &ComputedValues,
    profile: diagnostic::InputProfile,
) -> Result<s::InlineStyle, String> {
    let prepared = diagnostic::prepare_input(cv, profile);
    let initial = ComputedValues::initial();
    let mut remaining = prepared.clone();
    // Nine noninitial fields were observed across all 167 original IFCs, and
    // hanging-punctuation is mapped explicitly below. Root sizing is already
    // retained by the original measured width.
    remaining.font_family = initial.font_family.clone();
    remaining.font_size = initial.font_size;
    remaining.font_weight = initial.font_weight;
    remaining.line_height = initial.line_height;
    remaining.display = initial.display;
    remaining.direction = initial.direction;
    remaining.text_align = initial.text_align;
    remaining.text_autospace = initial.text_autospace;
    remaining.word_space_transform = initial.word_space_transform;
    remaining.word_break = initial.word_break;
    // Each inline box retains the computed flags, including explicit none.
    // The IFC root delegates its computed flags to LineOptions in project.
    remaining.hanging_punctuation = initial.hanging_punctuation;
    if remaining != ComputedValues::initial() {
        let fields = diagnostic::public_differences(&remaining, &initial)
            .into_iter()
            .map(|difference| difference.field)
            .collect::<Vec<_>>();
        return Err(format!(
            "computed style outside the verified ordinary-source input footprint: {}",
            if fields.is_empty() {
                "private computed style".to_owned()
            } else {
                fields.join(", ")
            }
        ));
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
        css::WordBreak::Manual => s::WordBreak::Manual,
        _ => return Err("word-break outside the verified source input footprint".into()),
    };
    let word_space_transform = match cv.word_space_transform {
        css::WordSpaceTransform::None => s::WordSpaceTransform::None,
        css::WordSpaceTransform::Space => s::WordSpaceTransform::Space,
        css::WordSpaceTransform::IdeographicSpace => s::WordSpaceTransform::IdeographicSpace,
        css::WordSpaceTransform::SpaceAutoPhrase => s::WordSpaceTransform::SpaceAutoPhrase,
        css::WordSpaceTransform::IdeographicSpaceAutoPhrase => {
            s::WordSpaceTransform::IdeographicSpaceAutoPhrase
        }
        _ => return Err("word-space-transform outside the verified source input footprint".into()),
    };
    Ok(s::InlineStyle {
        font_families,
        font_size: cv.font_size.0,
        font_weight: cv.font_weight,
        line_height,
        direction,
        text_autospace,
        word_break,
        word_space_transform,
        hanging_punctuation: Some(hanging_punctuation(cv.hanging_punctuation)?),
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
    // Root-level text follows LineOptions; nested inlines keep their explicit
    // computed override. This also preserves the root-only diagnostic control.
    root_style.hanging_punctuation = None;
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
        hanging_punctuation: hanging_punctuation(root_cv.hanging_punctuation)?,
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
                } else if node.tag_name() == Some("wbr") {
                    builder.push_text(
                        TextSource::Generated {
                            node: NodeId(id as u64),
                        },
                        "\u{200b}",
                    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hanging_punctuation_inline_mapping_retains_all_twelve_keyword_sets() {
        use css::HangingPunctuation as H;
        for (value, first, last, force_end, allow_end) in [
            (H::None, false, false, false, false),
            (H::First, true, false, false, false),
            (H::Last, false, true, false, false),
            (H::ForceEnd, false, false, true, false),
            (H::AllowEnd, false, false, false, true),
            (H::FirstLast, true, true, false, false),
            (H::FirstForceEnd, true, false, true, false),
            (H::FirstAllowEnd, true, false, false, true),
            (H::ForceEndLast, false, true, true, false),
            (H::AllowEndLast, false, true, false, true),
            (H::FirstForceEndLast, true, true, true, false),
            (H::FirstAllowEndLast, true, true, false, true),
        ] {
            let mut values = ComputedValues::initial();
            values.hanging_punctuation = value;
            let expected = s::HangingPunctuation {
                first,
                last,
                force_end,
                allow_end,
            };
            let projected = text_style(&values, diagnostic::InputProfile::Plain).unwrap();
            assert_eq!(projected.hanging_punctuation, Some(expected), "{value:?}");
        }
    }

    #[test]
    fn manual_word_break_projects_from_pinned_raikiri_style() {
        let mut values = ComputedValues::initial();
        values.word_break = css::WordBreak::Manual;
        let projected = text_style(&values, diagnostic::InputProfile::Plain).unwrap();
        assert_eq!(projected.word_break, s::WordBreak::Manual);
    }

    #[test]
    fn word_space_transform_projects_from_pinned_raikiri_style() {
        let mut values = ComputedValues::initial();
        values.word_space_transform = css::WordSpaceTransform::IdeographicSpaceAutoPhrase;
        let projected = text_style(&values, diagnostic::InputProfile::Plain).unwrap();
        assert_eq!(
            projected.word_space_transform,
            s::WordSpaceTransform::IdeographicSpaceAutoPhrase
        );
    }

    #[test]
    fn original_wpt_bytes_parse_and_project_hanging_without_hiding_other_fields() {
        use sha2::{Digest, Sha256};
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("inputs/hanging-punctuation");
        let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
        for (asset, hash) in [
            (
                "fonts/ahem.css",
                "5d8b9526d7be573871022125d5ec44f4893e4d851c26ab3fb6df44219422111c",
            ),
            (
                "fonts/Ahem.ttf",
                "b719ecb31c5b21fc573c03f6421c74ac63c271a5a3ff841e34f9705fb94b8448",
            ),
        ] {
            assert_eq!(
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(directory.join(asset)).unwrap())
                ),
                hash,
                "original resource changed: {asset}"
            );
        }
        for (file, hash, (first, last, force_end, allow_end)) in [
            (
                "hanging-punctuation-last.html",
                "9a1f7573c815df95191d5118335c219f77472742543798c0e8d0412f7a9ca042",
                (false, true, false, false),
            ),
            (
                "hanging-punctuation-last-whitespace.html",
                "88bc75b0bd7d1d394d20d9f6ef9b1bca361c6357d8fa201de412f90726fa65d5",
                (false, true, false, false),
            ),
            (
                "hanging-punctuation-first-and-last-together.html",
                "9521a981499b7567e1bc418e6149064f305789cd450aed005b401333bee666c8",
                (true, true, false, false),
            ),
            (
                "hanging-punctuation-force-end-001.xht",
                "28adae1b68c8e56837f157261da80c7ea8c8fb27b8b67791ba6df584bf3c6476",
                (false, false, true, false),
            ),
            (
                "hanging-punctuation-allow-end-001.xht",
                "6bddf4b8e0779358c005b16f3eb0e34322f70bef2e39c6a03a961e1bdf9940d5",
                (false, false, false, true),
            ),
        ] {
            let bytes = std::fs::read(directory.join(file)).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                hash,
                "original source changed: {file}"
            );
            let input = super::super::offline::parse_screen(&directory, file).unwrap();
            let expected = s::HangingPunctuation {
                first,
                last,
                force_end,
                allow_end,
            };
            let mut mapped = 0;
            let mut color_rejections = 0;
            for (node, cv) in input.cascade.computed.iter().enumerate() {
                if cv.hanging_punctuation == css::HangingPunctuation::None {
                    continue;
                }
                assert_eq!(
                    hanging_punctuation(cv.hanging_punctuation).unwrap(),
                    expected,
                    "{file}: node {node}"
                );
                mapped += 1;
                let profile = match cv.display {
                    css::DisplayValue::Block => diagnostic::InputProfile::MeasuredBlock,
                    css::DisplayValue::InlineBlock => diagnostic::InputProfile::Atomic,
                    _ => diagnostic::InputProfile::Plain,
                };
                match text_style(cv, profile) {
                    Ok(style) => assert_eq!(style.hanging_punctuation, Some(expected)),
                    Err(error) => {
                        assert!(error.contains("computed style outside"), "{file}: {error}");
                        if cv.color != ComputedValues::initial().color {
                            assert!(
                                error.contains("color"),
                                "paint ownership was hidden: {file}: {error}"
                            );
                            color_rejections += 1;
                        }
                        assert!(!error.contains("hanging_punctuation"), "{file}: {error}");
                        assert!(!error.contains("private computed style"), "{file}: {error}");
                    }
                }
            }
            assert!(
                mapped > 0,
                "original hanging declaration was dropped: {file}"
            );
            if last {
                assert!(
                    color_rejections > 0,
                    "original color was silently discarded: {file}"
                );
            }
            if force_end || allow_end {
                let roots: Vec<_> = (0..input.parsed.dom.node_count())
                    .filter(|&node| {
                        input
                            .parsed
                            .dom
                            .get_node(node)
                            .is_some_and(|element| element.attribute("class") == Some("test"))
                    })
                    .collect();
                assert!(!roots.is_empty());
                for root in roots {
                    let prepared = project(
                        &input,
                        root,
                        400.0,
                        &mut LayoutContext::new(),
                        &fonts.collection,
                        &Limits::default(),
                    )
                    .unwrap();
                    assert_eq!(prepared.options.hanging_punctuation, expected);
                    assert!(!prepared.paragraph.text().is_empty());
                }
            }
            // The real native IFC path accepts all original hanging styles.
            // This proves assignment, not painted reference equivalence; the
            // force/allow Japanese fonts are not bundled in this small input.
            let native_fonts =
                raikiri_dom::build_wpt_font_collection(&directory.join("fonts")).unwrap();
            let mut document = input.parsed.dom;
            document.set_font_collection(native_fonts);
            raikiri_dom::layout_single_page(
                &mut document,
                &input.cascade,
                raikiri_traits::PageBox::new(),
            )
            .unwrap_or_else(|error| {
                panic!("native IFC rejected original style in {file}: {error:?}")
            });
            assert_eq!(
                std::fs::read(directory.join(file)).unwrap(),
                bytes,
                "replay changed source: {file}"
            );
        }
    }
}
