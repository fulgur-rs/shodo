//! Paint ownership and source interaction are deliberately separate contracts.
#[path = "support/glyph_paint.rs"]
mod glyph_paint;
use shodo::geometry::Direction;
use shodo::hit::{LineLayout, TextPosition};
use shodo::mapping::{Affinity, TextOrigin};
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, LayoutContext, Line, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};

fn build(parts: &[&str], direction: Direction, font: usize, width: f32) -> Vec<Line> {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[font].family.into())],
        font_size: 32.0,
        direction,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            direction,
            root: style.clone(),
            ..Default::default()
        },
        &limits,
    );
    b.with_offset_mapping(true);
    for (i, text) in parts.iter().enumerate() {
        b.open_inline(NodeId(10 + i as u64), &style, Default::default())
            .push_text(
                TextSource::Dom {
                    node: NodeId(100 + i as u64),
                    offset: 0,
                },
                text,
            )
            .close_inline();
    }
    b.build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        )
}
fn pos(line: usize, offset: u32, affinity: Affinity) -> TextPosition {
    TextPosition {
        line,
        offset,
        affinity,
    }
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1.0 / 32.0, "{a} vs {b}");
}
fn glyphs(lines: &[Line]) -> Vec<(u32, u32, f32, f32)> {
    lines
        .iter()
        .flat_map(|l| l.fragments())
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .flat_map(|r| r.glyphs())
        .map(|g| (g.id, g.cluster, g.inline_position, g.advance))
        .collect()
}

#[test]
fn ffi_is_painted_once_using_its_owner_even_across_color_boundaries() {
    let lines = build(&["f", "f", "i"], Direction::Ltr, 0, 1000.0);
    let runs: Vec<_> = lines[0]
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].node(), Some(NodeId(100)));
    assert_eq!(runs[0].text_range(), 0..3);
    assert_eq!(runs[0].glyphs().len(), 1);
    for differing in [false, true] {
        let mut requested = Vec::new();
        let (image, count) = glyph_paint::paint(
            &lines,
            |owner| {
                requested.push(owner);
                if differing && owner != NodeId(100) {
                    [0, 0, 255, 255]
                } else {
                    [255, 0, 0, 255]
                }
            },
            &[],
        );
        assert_eq!(count, 1);
        assert_eq!(requested, vec![NodeId(100)]);
        let pixels = image.data().as_chunks::<4>().0;
        assert!(pixels.iter().filter(|p| p[0] > p[2]).count() > 20);
        assert_eq!(pixels.iter().filter(|p| p[2] > p[0]).count(), 0);
    }
}

#[test]
fn ffi_source_regions_partition_the_advance_and_drive_links_and_underline() {
    let lines = build(&["f", "f", "i"], Direction::Ltr, 0, 1000.0);
    let layout = LineLayout::new(&lines);
    let mapping = lines[0].offset_mapping().unwrap();
    let mut regions = Vec::new();
    for i in 0..3 {
        let source = NodeId(100 + i);
        let (start, _) = mapping.dom_to_text(source, 0).unwrap();
        let (end, _) = mapping.dom_to_text(source, 1).unwrap();
        let rects = layout.selection_rects(
            pos(0, start, Affinity::Downstream),
            pos(0, end, Affinity::Upstream),
        );
        assert_eq!(rects.len(), 1);
        assert!(rects[0].inline_size > 0.0);
        let start_caret = layout.caret(pos(0, start, Affinity::Downstream)).unwrap();
        let hit = layout
            .hit_test(
                start_caret.rect.inline_start,
                start_caret.rect.block_start + 1.0,
            )
            .unwrap();
        assert_eq!(hit.position.offset, start);
        // At a shared edge the hit's affinity decides the side. Source identity
        // is retrieved explicitly for the downstream link interval.
        assert_eq!(
            mapping.text_to_dom(start, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: source,
                offset: 0
            })
        );
        regions.push(rects[0]);
    }
    close(regions[0].inline_start, 0.0);
    close(
        regions[0].inline_start + regions[0].inline_size,
        regions[1].inline_start,
    );
    close(
        regions[1].inline_start + regions[1].inline_size,
        regions[2].inline_start,
    );
    close(
        regions.iter().map(|r| r.inline_size).sum(),
        lines[0].inline_size(),
    );
    // Only the middle source is linked/underlined, despite the first glyph
    // owner. Annotation geometry must never duplicate the whole glyph.
    let (image, count) = glyph_paint::paint(&lines, |_| [255, 0, 0, 255], &[regions[1]]);
    assert_eq!(count, 1);
    assert!(image.data().as_chunks::<4>().0.iter().any(|p| p[2] > p[0]));
    assert_blue_bounds(&image, regions[1]);
    assert!(regions[1].inline_size < lines[0].inline_size());
}

#[test]
fn arabic_joining_survives_node_boundaries_and_rtl_source_regions() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        let joined = build(&["سلام"], direction, 2, 1000.0);
        let split = build(&["س", "ل", "ا", "م"], direction, 2, 1000.0);
        assert_eq!(logical_glyphs(&joined), logical_glyphs(&split));
        let layout = LineLayout::new(&split);
        let mut widths = 0.0;
        for i in 0..4 {
            let m = split[0].offset_mapping().unwrap();
            let (a, _) = m.dom_to_text(NodeId(100 + i), 0).unwrap();
            let (b, _) = m.dom_to_text(NodeId(100 + i), 2).unwrap();
            let rects = layout.selection_rects(
                pos(0, a, Affinity::Downstream),
                pos(0, b, Affinity::Upstream),
            );
            assert!(!rects.is_empty());
            widths += rects.iter().map(|r| r.inline_size).sum::<f32>();
            assert_eq!(
                m.text_to_dom(a, Affinity::Downstream),
                Some(TextOrigin::Dom {
                    node: NodeId(100 + i),
                    offset: 0
                })
            );
        }
        close(widths, split[0].inline_size());
        let (image, count) = glyph_paint::paint(&split, |_| [0, 0, 0, 255], &[]);
        let (reference, _) = glyph_paint::paint(&joined, |_| [0, 0, 0, 255], &[]);
        assert_eq!(image.data(), reference.data());
        assert_eq!(count, glyphs(&split).len());
    }
}

#[test]
fn narrow_arabic_lines_match_unsplit_source_without_lost_or_duplicated_glyphs() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        for width in [38.0, 70.0] {
            let joined = build(&["سلام سلام"], direction, 2, width);
            let split = build(&["س", "لام ", "س", "لام"], direction, 2, width);
            assert!(split.len() > 1);
            assert_eq!(
                split.iter().map(Line::text_range).collect::<Vec<_>>(),
                joined.iter().map(Line::text_range).collect::<Vec<_>>()
            );
            assert_eq!(logical_glyphs(&split), logical_glyphs(&joined));
            let (image, count) = glyph_paint::paint(&split, |_| [0, 0, 0, 255], &[]);
            let (reference, _) = glyph_paint::paint(&joined, |_| [0, 0, 0, 255], &[]);
            assert_eq!(image.data(), reference.data());
            assert_eq!(count, glyphs(&split).len());
            let layout = LineLayout::new(&split);
            let first = split[0].text_range().start as u32;
            let n = split.len() - 1;
            let end = split[n].text_range().end as u32;
            assert!(
                !layout
                    .selection_rects(
                        pos(0, first, Affinity::Downstream),
                        pos(n, end, Affinity::Upstream)
                    )
                    .is_empty()
            );
        }
    }
}

// Fragment subdivision can change enumeration order in bidi text. Compare
// actual IDs, source clusters, advances and positioned coordinates.
fn logical_glyphs(lines: &[Line]) -> Vec<(u32, u32, f32, f32)> {
    let mut g = glyphs(lines);
    g.sort_by_key(|g| (g.1, g.0));
    g
}

#[test]
fn raikiri_cascade_to_shared_glyph_paint_keeps_link_and_decoration_source_identity() {
    use raikiri_html::{ParseOptions, parse_html};
    use raikiri_style::property::TextDecorationLine;
    use raikiri_traits::{Dom, NodeId as DomId};
    let html = b"<style>#root{font-family:'Shodo Fixture Latin';font-size:32px;color:red}#link{color:blue;text-decoration-line:underline}</style><div id=root><span>f</span><a id=link href='/target'>f</a><span>i</span></div>";
    let doc = parse_html(
        &html[..],
        &ParseOptions {
            extra_stylesheets: &[],
            network: None,
            base_url: None,
        },
    )
    .unwrap();
    let (parsed, cascade) = doc.into_parts();
    let root = (0..parsed.dom.node_count())
        .find(|&i| parsed.dom.get_node(i).unwrap().attribute("id") == Some("root"))
        .unwrap();
    let style = InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[0].family.into())],
        font_size: 32.0,
        ..Default::default()
    };
    let limits = Default::default();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style.clone(),
            ..Default::default()
        },
        &limits,
    );
    b.with_offset_mapping(true);
    let mut sources = Vec::new();
    for child in parsed.dom.child_ids(DomId::new(root as u64)) {
        let element = child.0 as usize;
        let cv = &cascade.computed[element];
        assert_eq!(cv.font_family[0].as_str(), FONTS[0].family);
        assert_eq!(cv.font_size.0, 32.0);
        b.open_inline(NodeId(element as u64), &style, Default::default());
        for text_id in parsed.dom.child_ids(child) {
            let text_node = parsed.dom.get_node(text_id.0 as usize).unwrap();
            let text = text_node.text_content().unwrap();
            assert_eq!(text.len(), 1);
            let source = NodeId(text_id.0);
            b.push_text(
                TextSource::Dom {
                    node: source,
                    offset: 0,
                },
                text,
            );
            sources.push((source, element));
        }
        b.close_inline();
    }
    assert_eq!(sources.len(), 3);
    let fonts = load_fonts(&limits).unwrap();
    let lines = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
    assert_eq!(lines[0].text(), "ffi");
    let layout = LineLayout::new(&lines);
    let m = lines[0].offset_mapping().unwrap();
    let (source, element) = sources[1];
    let link = parsed.dom.get_node(element).unwrap();
    assert_eq!(link.attribute("href"), Some("/target"));
    assert_eq!(
        cascade.computed[element].text_decoration_line,
        TextDecorationLine::UNDERLINE
    );
    let link_color = cascade.computed[element].color;
    assert_eq!(
        [link_color.r, link_color.g, link_color.b, link_color.a],
        [0, 0, 255, 255]
    );
    let (a, _) = m.dom_to_text(source, 0).unwrap();
    let (z, _) = m.dom_to_text(source, 1).unwrap();
    let rects = layout.selection_rects(
        pos(0, a, Affinity::Downstream),
        pos(0, z, Affinity::Upstream),
    );
    assert_eq!(rects.len(), 1);
    let link_x = rects[0].inline_start + rects[0].inline_size * 0.5;
    // The caller resolves the link by containment in the source's region;
    // caret hit testing returns a nearest stop, not a link's element ID.
    assert!(
        link_x >= rects[0].inline_start && link_x < rects[0].inline_start + rects[0].inline_size
    );
    assert!(
        layout
            .hit_test(link_x, rects[0].block_start + 1.0)
            .unwrap()
            .inside
    );
    assert_eq!(
        m.text_to_dom(a, Affinity::Downstream),
        Some(TextOrigin::Dom {
            node: source,
            offset: 0
        })
    );
    let mut owners = Vec::new();
    let (image, count) = glyph_paint::paint(
        &lines,
        |owner| {
            owners.push(owner);
            let parent = sources
                .iter()
                .find(|(source, _)| *source == owner)
                .unwrap()
                .1;
            let c = cascade.computed[parent].color;
            [c.r, c.g, c.b, c.a]
        },
        &rects,
    );
    assert_eq!(count, 1);
    assert_eq!(owners, vec![sources[0].0]);
    let pixels = image.data().as_chunks::<4>().0;
    assert!(pixels.iter().any(|p| p[0] > p[2]));
    assert!(pixels.iter().any(|p| p[2] > p[0]));
    assert_blue_bounds(&image, rects[0]);
    if let Some(path) = std::env::var_os("SHODO_SHARED_GLYPH_PNG") {
        image.save_png(path).unwrap();
    }
}

fn assert_blue_bounds(image: &tiny_skia::Pixmap, region: shodo::geometry::LogicalRect) {
    let blue: Vec<_> = image
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, p)| p[2] > p[0])
        .map(|(i, _)| (i as u32 % image.width()) as f32)
        .collect();
    let min = blue.iter().copied().fold(f32::INFINITY, f32::min);
    let max = blue.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(min >= (10.0 + region.inline_start).floor());
    assert!(max < (10.0 + region.inline_start + region.inline_size).ceil());
    assert!(max - min + 1.0 >= region.inline_size - 2.0);
}
