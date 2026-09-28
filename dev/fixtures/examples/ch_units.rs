//! A fixed horizontal CSS→font measurement→shodo caller reproduction.
//! This is separate from the unmerged raikiri integration spikes.
use raikiri_html::{ParseOptions, parse_html};
use raikiri_style::{
    ChFontKey, ChLengthProvenance, ComputedLengthPercentage as Length,
    ComputedLengthPercentageOrAuto as Margin,
};
use raikiri_traits::{Dom, NodeId as DomId};
use shodo::font::{FontCollection, FontQuery};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{FontFamily, FontStyle, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, LayoutContext, Line, ParagraphBuilder};

fn family(names: &[raikiri_style::property::FontFamilyName]) -> Vec<FontFamily> {
    // This fixed example supplies named fixture families, never host fonts.
    names
        .iter()
        .map(|n| FontFamily::Named(n.as_str().to_owned()))
        .collect()
}
fn font_style(style: raikiri_style::property::FontStyle) -> Result<FontStyle, String> {
    match style {
        raikiri_style::property::FontStyle::Normal => Ok(FontStyle::Normal),
        raikiri_style::property::FontStyle::Italic => Ok(FontStyle::Italic),
        raikiri_style::property::FontStyle::Oblique => Ok(FontStyle::Oblique(14.0)),
        _ => Err("example font style unsupported".into()),
    }
}
fn ch(
    fonts: &FontCollection,
    factor: Option<f32>,
    key: Option<&ChFontKey>,
    fallback: f32,
) -> Result<f32, String> {
    match factor {
        None => Ok(fallback),
        Some(factor) => {
            let key = key.ok_or("ch value lost its declaring-font key")?;
            let query = FontQuery {
                families: family(&key.family),
                weight: key.weight,
                style: font_style(key.style)?,
                ..Default::default()
            };
            // Select the face supplying U+0030, including normal CSS fallback.
            Ok(factor * fonts.resolve_ch(&query, key.size.0).advance)
        }
    }
}
fn edge(
    fonts: &FontCollection,
    provenance: Option<&ChLengthProvenance>,
    fallback: f32,
) -> Result<f32, String> {
    ch(
        fonts,
        provenance.map(|p| p.factor),
        provenance.map(|p| &p.font),
        fallback,
    )
}
fn padding(value: Length) -> Result<f32, String> {
    match value {
        Length::Px(px) => Ok(px),
        _ => Err("example needs absolute/ch padding".into()),
    }
}
fn margin(value: Margin) -> Result<f32, String> {
    match value {
        Margin::Px(px) => Ok(px),
        Margin::Auto => Ok(0.0),
        _ => Err("example needs absolute/ch margin".into()),
    }
}
fn layout(html: &str, fonts: &FontCollection) -> Result<Line, String> {
    let doc = parse_html(
        html.as_bytes(),
        &ParseOptions {
            extra_stylesheets: &[],
            network: None,
            base_url: None,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    let find = |name| {
        (0..doc.dom().node_count())
            .find(|&id| doc.dom().get_node(id).unwrap().attribute("id") == Some(name))
            .ok_or_else(|| format!("missing #{name}"))
    };
    let parent = find("parent")?;
    let child = find("child")?;
    let cv = &doc.cascade().computed[child];
    let root = &doc.cascade().computed[parent];
    for values in [root, cv] {
        if values.direction != raikiri_style::property::Direction::Ltr
            || values.cssom_writing_mode != raikiri_style::property::WritingMode::HorizontalTb
        {
            return Err("fixed example requires horizontal LTR layout".into());
        }
    }
    let absolute = |v| match v {
        raikiri_style::ComputedLetterSpacing::Px(px) => Ok(px),
        _ => Err("example spacing needs absolute/ch value".to_owned()),
    };
    let style = InlineStyle {
        font_families: family(&cv.font_family),
        font_size: cv.font_size.0,
        font_weight: cv.font_weight,
        font_style: font_style(cv.font_style)?,
        letter_spacing: ch(
            fonts,
            cv.letter_spacing_ch_factor,
            cv.letter_spacing_ch_font.as_ref(),
            absolute(cv.letter_spacing_computed)?,
        )?,
        word_spacing: ch(
            fonts,
            cv.word_spacing_ch_factor,
            cv.word_spacing_ch_font.as_ref(),
            absolute(cv.word_spacing_computed)?,
        )?,
        ..Default::default()
    };
    let mut options = LineOptions::default();
    let indent = match root.text_indent {
        raikiri_style::ComputedTextIndent::Px(px) => px,
        _ => return Err("example indent needs absolute/ch value".into()),
    };
    options.text_indent.length = ch(
        fonts,
        root.text_indent_ch_factor,
        root.text_indent_ch_font.as_ref(),
        indent,
    )?;
    options.text_indent.hanging = root.text_indent_hanging;
    options.text_indent.each_line = root.text_indent_each_line;
    let sides = |top, right, bottom, left| Sides {
        inline_start: left,
        inline_end: right,
        block_start: top,
        block_end: bottom,
    };
    let edges = InlineEdges {
        margin: sides(
            edge(fonts, cv.margin_ch.top.as_ref(), margin(cv.margin.top)?)?,
            edge(fonts, cv.margin_ch.right.as_ref(), margin(cv.margin.right)?)?,
            edge(
                fonts,
                cv.margin_ch.bottom.as_ref(),
                margin(cv.margin.bottom)?,
            )?,
            edge(fonts, cv.margin_ch.left.as_ref(), margin(cv.margin.left)?)?,
        ),
        padding: sides(
            edge(fonts, cv.padding_ch.top.as_ref(), padding(cv.padding.top)?)?,
            edge(
                fonts,
                cv.padding_ch.right.as_ref(),
                padding(cv.padding.right)?,
            )?,
            edge(
                fonts,
                cv.padding_ch.bottom.as_ref(),
                padding(cv.padding.bottom)?,
            )?,
            edge(
                fonts,
                cv.padding_ch.left.as_ref(),
                padding(cv.padding.left)?,
            )?,
        ),
        ..Default::default()
    };
    let paragraph_style = ParagraphStyle {
        root: style.clone(),
        ..Default::default()
    };
    let limits = Limits::default();
    let mut builder = ParagraphBuilder::new(&paragraph_style, &limits);
    builder.open_inline(NodeId(child as u64), &style, edges);
    for id in doc.dom().child_ids(DomId(child as u64)) {
        let text = doc
            .dom()
            .get_node(id.0 as usize)
            .unwrap()
            .text_content()
            .ok_or("example child must contain text nodes")?;
        builder.push_text(
            TextSource::Dom {
                node: NodeId(id.0),
                offset: 0,
            },
            text,
        );
    }
    builder.close_inline();
    let mut context = LayoutContext::new();
    let paragraph = builder
        .build(&mut context, fonts)
        .map_err(|e| format!("{e:?}"))?;
    let lines = paragraph.break_all(&mut context, &options, 10000.0, &AtomicSizes::EMPTY);
    if lines.len() != 1 {
        return Err("fixed example expected exactly one line".into());
    }
    Ok(lines.into_iter().next().unwrap())
}
fn main() -> Result<(), String> {
    let html = "<style>#parent{font-family:'Shodo Fixture Latin';font-size:20px;word-spacing:2ch;text-indent:3ch}#child{font-family:'Shodo Fixture CJK';font-size:40px;margin-left:4ch;margin-right:4ch;padding-left:5ch;padding-right:5ch}</style><div id=parent><span id=child>0 0</span></div>";
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).map_err(|e| format!("{e:?}"))?;
    let line = layout(html, &fonts.collection)?;
    let runs: Vec<_> = line.fragments().filter_map(|f|match f {
        Fragment::GlyphRun(run) => Some(serde_json::json!({"font":format!("{:?}",run.font()),"font_size":run.font_size(),"inline_start":run.inline_start(),"inline_size":run.inline_size()})), _ => None,
    }).collect();
    println!(
        "{}",
        serde_json::json!({"text":line.text(),"inline_size":line.inline_size(),"runs":runs})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shodo::Fragment;
    use shodo_fixtures::load_fonts;

    const HTML: &str = "<style>#parent{font-family:'Shodo Fixture Latin';font-size:20px;word-spacing:2ch;letter-spacing:0px;text-indent:3ch}#child{font-family:'Shodo Fixture CJK';font-size:40px;margin-left:4ch;margin-right:4ch;padding-left:5ch;padding-right:5ch}</style><div id=parent><span id=child>0 0</span></div>";
    fn close(actual: f32, expected: f32) {
        // Caller lengths are rounded to 1/64px. The two inline edges can
        // together differ from the unrounded font-table oracle by one unit.
        assert!(
            (actual - expected).abs() < 1.0 / 64.0,
            "{actual} != {expected}"
        );
    }
    #[test]
    fn inherited_ch_spacing_uses_declaring_font_in_actual_lines() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let full = layout(HTML, &fonts.collection).unwrap();
        let zero = layout(
            &HTML.replace("word-spacing:2ch", "word-spacing:0px"),
            &fonts.collection,
        )
        .unwrap();
        // Raw pinned Latin hmtx: U+0030 gid19, advance572/1000em.
        // Parent20px means2ch=22.88px, even though the text uses CJK40px.
        close(full.inline_size() - zero.inline_size(), 22.88);
        let runs: Vec<_> = full
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .collect();
        assert!(!runs.is_empty());
        assert!(runs.iter().all(|r| r.font() == fonts.ids[1]));
        assert_eq!(full.text_range(), 0..3);
    }
    #[test]
    fn inherited_indent_and_own_ch_edges_reach_line_geometry() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let full = layout(HTML, &fonts.collection).unwrap();
        let no_indent = layout(
            &HTML.replace("text-indent:3ch", "text-indent:0px"),
            &fonts.collection,
        )
        .unwrap();
        let first = |line: &shodo::Line| {
            line.fragments()
                .find_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r.inline_start()),
                    _ => None,
                })
                .unwrap()
        };
        close(first(&full) - first(&no_indent), 34.32);
        // Raw pinned CJK hmtx: U+0030 gid17, advance555/1000em.
        // Each4ch margin /5ch padding uses child's40px, not parent's20px.
        let no_margin = layout(
            &HTML.replace(
                "margin-left:4ch;margin-right:4ch",
                "margin-left:0px;margin-right:0px",
            ),
            &fonts.collection,
        )
        .unwrap();
        let no_padding = layout(
            &HTML.replace(
                "padding-left:5ch;padding-right:5ch",
                "padding-left:0px;padding-right:0px",
            ),
            &fonts.collection,
        )
        .unwrap();
        close(full.inline_size() - no_margin.inline_size(), 177.6);
        close(full.inline_size() - no_padding.inline_size(), 222.0);
        close(first(&full) - first(&no_margin), 88.8);
        close(first(&full) - first(&no_padding), 111.0);
    }

    #[test]
    fn fixed_example_rejects_other_directions_and_writing_modes() {
        let fonts = load_fonts(&Default::default()).unwrap();
        for (selector, declaration) in [
            ("#parent", "direction:rtl"),
            ("#child", "direction:rtl"),
            ("#parent", "writing-mode:vertical-rl"),
            ("#child", "writing-mode:sideways-lr"),
        ] {
            let html = HTML.replace(
                &format!("{selector}{{"),
                &format!("{selector}{{{declaration};"),
            );
            assert!(
                layout(&html, &fonts.collection).is_err(),
                "fixed LTR example silently accepted {selector} {declaration}"
            );
        }
    }

    #[test]
    fn declaring_font_fallback_is_measured_before_line_layout() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let html = HTML.replace(
            "font-family:'Shodo Fixture Latin'",
            "font-family:'Missing Fixture','Shodo Fixture Latin'",
        );
        let full = layout(&html, &fonts.collection).unwrap();
        let zero = layout(
            &html.replace("word-spacing:2ch", "word-spacing:0px"),
            &fonts.collection,
        )
        .unwrap();
        close(full.inline_size() - zero.inline_size(), 22.88);
    }

    #[test]
    fn missing_zero_uses_half_em_in_actual_lines() {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            shodo::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let full = layout(HTML, &fonts).unwrap();
        let zero = layout(
            &HTML.replace("word-spacing:2ch", "word-spacing:0px"),
            &fonts,
        )
        .unwrap();
        // With no U+0030 face, CSS fallback at the declaring20px is0.5em.
        close(full.inline_size() - zero.inline_size(), 20.0);
    }
}
