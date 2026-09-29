//! A fixed horizontal CSS→font measurement→shodo caller reproduction.
//! This is separate from the unmerged raikiri integration spikes.
use raikiri_html::{ParseOptions, parse_html};
use raikiri_style::{ComputedLengthPercentage as Length, ComputedLengthPercentageOrAuto as Margin};
use raikiri_traits::{Dom, NodeId as DomId};

#[path = "../adapter/ch.rs"]
mod ch_adapter;
use ch_adapter::{
    Physical, direction, family, font_style, resolve_edge as edge, resolve_px as ch, to_logical,
};
use shodo::font::FontCollection;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, LayoutContext, Line, ParagraphBuilder};

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
    if root.direction != cv.direction {
        // shodo swaps a box's inline edges when its direction opposes the
        // paragraph's; that placement is not verified against CSS here.
        return Err("mixed box/paragraph direction unsupported".into());
    }
    for values in [root, cv] {
        ch_adapter::require_keyed_font_inputs(values)?;
        if values.cssom_writing_mode != raikiri_style::property::WritingMode::HorizontalTb {
            return Err("fixed example requires a horizontal writing mode".into());
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
        direction: direction(cv.direction)?,
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
    let physical = |top, right, bottom, left| Physical {
        top,
        right,
        bottom,
        left,
    };
    let (dir, wm) = (cv.direction, cv.cssom_writing_mode);
    let edges = InlineEdges {
        margin: to_logical(
            dir,
            wm,
            physical(
                edge(fonts, cv.margin_ch.top.as_ref(), margin(cv.margin.top)?)?,
                edge(fonts, cv.margin_ch.right.as_ref(), margin(cv.margin.right)?)?,
                edge(
                    fonts,
                    cv.margin_ch.bottom.as_ref(),
                    margin(cv.margin.bottom)?,
                )?,
                edge(fonts, cv.margin_ch.left.as_ref(), margin(cv.margin.left)?)?,
            ),
        )?,
        padding: to_logical(
            dir,
            wm,
            physical(
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
        )?,
        ..Default::default()
    };
    let paragraph_style = ParagraphStyle {
        root: InlineStyle {
            direction: direction(root.direction)?,
            ..style.clone()
        },
        direction: direction(root.direction)?,
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
        // The caller rounds each applied length to 1/64px.
        assert!(
            (actual - expected).abs() < 1.0 / 64.0,
            "{actual} != {expected}"
        );
    }
    fn rounded_ch(advance: f32, size: f32) -> f32 {
        // The pinned fixtures use 1000 units per em. Match the glyph's
        // layout-unit rounding before multiplying by a CSS ch factor.
        ((advance * (size / 1000.0)) * 64.0).round() / 64.0
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
        // Parent20px supplies ch, even though the text uses CJK40px.
        close(
            full.inline_size() - zero.inline_size(),
            2.0 * rounded_ch(572.0, 20.0),
        );
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
        close(
            first(&full) - first(&no_indent),
            3.0 * rounded_ch(572.0, 20.0),
        );
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
        let child_ch = rounded_ch(555.0, 40.0);
        close(full.inline_size() - no_margin.inline_size(), 8.0 * child_ch);
        close(
            full.inline_size() - no_padding.inline_size(),
            10.0 * child_ch,
        );
        close(first(&full) - first(&no_margin), 4.0 * child_ch);
        close(first(&full) - first(&no_padding), 5.0 * child_ch);
    }

    fn first_run(line: &shodo::Line) -> f32 {
        line.fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r.inline_start()),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn rtl_maps_physical_ch_margins_to_inline_edges() {
        let fonts = load_fonts(&Default::default()).unwrap();
        // Only the physical left margin is 4ch (child 40px: 88.8px).
        let base = HTML
            .replace("margin-right:4ch", "margin-right:0px")
            .replace(
                "padding-left:5ch;padding-right:5ch",
                "padding-left:0px;padding-right:0px",
            );
        let none = base.replace("margin-left:4ch", "margin-left:0px");
        let ltr = |html: &str| layout(html, &fonts.collection).unwrap();
        let rtl = |html: &str| {
            let html = html
                .replace("#parent{", "#parent{direction:rtl;")
                .replace("#child{", "#child{direction:rtl;");
            layout(&html, &fonts.collection).unwrap()
        };
        // LTR: the left margin is inline-start and shifts the first run.
        close(first_run(&ltr(&base)) - first_run(&ltr(&none)), 88.8);
        // RTL: it is the box's inline-end, so it trails the content.
        close(first_run(&rtl(&base)) - first_run(&rtl(&none)), 0.0);
        close(rtl(&base).inline_size() - rtl(&none).inline_size(), 88.8);
    }

    #[test]
    fn rtl_paragraph_keeps_inherited_ch_indent() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let rtl = |html: &str| {
            let html = html
                .replace("#parent{", "#parent{direction:rtl;")
                .replace("#child{", "#child{direction:rtl;");
            layout(&html, &fonts.collection).unwrap()
        };
        let full = rtl(HTML);
        let zero = rtl(&HTML.replace("text-indent:3ch", "text-indent:0px"));
        close(first_run(&full) - first_run(&zero), 34.32);
    }

    #[test]
    fn mixed_box_and_paragraph_direction_is_rejected() {
        let fonts = load_fonts(&Default::default()).unwrap();
        let html = HTML.replace("#child{", "#child{direction:rtl;");
        assert!(layout(&html, &fonts.collection).is_err());
    }

    #[test]
    fn fixed_example_rejects_other_directions_and_writing_modes() {
        let fonts = load_fonts(&Default::default()).unwrap();
        for (selector, declaration) in [
            ("#parent", "writing-mode:vertical-rl"),
            ("#child", "writing-mode:sideways-lr"),
            ("#parent", "font-variation-settings:'wght' 700"),
            ("#child", "font-variation-settings:'wght' 700"),
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
