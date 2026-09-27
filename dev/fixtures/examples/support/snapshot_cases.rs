//! Fixed accepted-output inputs. No renderer-side shaping or host fonts.
#[path = "float_flow.rs"]
mod flow;
#[path = "glyph_paint.rs"]
mod glyph_paint;
use serde_json::{Value, json};
use shodo::geometry::{BaselineKind, Direction, LogicalRect};
use shodo::hit::{LineLayout, TextPosition};
use shodo::mapping::Affinity;
use shodo::node::{InlineEdges, NodeId, OutOfFlowKind, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineHeight, LineOptions, ParagraphStyle, TabSize, WhiteSpaceCollapse,
};
use shodo::{
    AtomicSize, AtomicSizes, Fragment, LayoutContext, Line, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder,
};
use shodo_fixtures::{FONTS, FixtureFonts, load_fonts};

const VARIANTS: [&str; 9] = [
    "shared-ffi-color",
    "arabic-wrap",
    "nested-atomic-baseline",
    "preserved-tabs",
    "normal-white-space",
    "hanging-white-space",
    "indent-baseline",
    "japanese-kinsoku",
    "float-pages",
];
pub struct Rendered {
    pub id: String,
    pub image: tiny_skia::Pixmap,
    pub settings: Value,
    pub geometry: Value,
    pub glyph_count: usize,
}
pub fn case_ids() -> Vec<String> {
    shodo_fixtures::cases()
        .iter()
        .map(|c| c.id.clone())
        .chain(VARIANTS.iter().map(|s| s.to_string()))
        .collect()
}
pub fn conditions() -> Value {
    json!({"canvas":[512,1024],"scale":1,"origin":[10,10],"background":[255,255,255,255],"renderer":"skrifa-0.44.0/tiny-skia-0.12.0/unhinted", "fonts":FONTS.iter().map(|f|json!({"id":f.id,"sha256":f.sha256,"face":f.face_index})).collect::<Vec<_>>()})
}
fn fixed_font(run: shodo::GlyphRunView<'_>) -> Result<&'static str, String> {
    let data = run.font_data().ok_or("accepted glyph has no font bytes")?;
    FONTS
        .iter()
        .find(|f| f.bytes == data.data.as_ref() && f.face_index == data.index)
        .map(|f| f.id)
        .ok_or_else(|| "accepted glyph does not use a fixture face".into())
}
pub fn paint_lines(
    lines: &[Line],
    annotations: &[LogicalRect],
    color: impl FnMut(NodeId) -> [u8; 4],
) -> Result<(tiny_skia::Pixmap, usize), String> {
    for line in lines {
        if !line.block_offset().is_finite()
            || line.block_offset() < 0.0
            || line.block_offset() + line.block_size() + 40.0 > 1024.0
        {
            return Err("accepted line exceeds fixed canvas".into());
        }
        for fragment in line.fragments() {
            match fragment {
                Fragment::GlyphRun(run) => {
                    fixed_font(run)?;
                    for g in run.glyphs() {
                        if !g.inline_position.is_finite()
                            || g.inline_position + 10.0 < 0.0
                            || g.inline_position + g.advance + 10.0 > 512.0
                        {
                            return Err("accepted glyph exceeds fixed canvas".into());
                        }
                    }
                }
                Fragment::Atomic(a) => {
                    let r = a.border_rect;
                    if r.inline_start + 10.0 < 0.0
                        || r.inline_start + r.inline_size + 10.0 > 512.0
                        || line.block_offset() + r.block_start + 10.0 < 0.0
                        || line.block_offset() + r.block_start + r.block_size + 10.0 > 1024.0
                    {
                        return Err("accepted atomic exceeds fixed canvas".into());
                    }
                }
                _ => {}
            }
        }
    }
    glyph_paint::try_paint_on_canvas(lines, color, annotations, 512, 1024)
        .map_err(|e| e.to_string())
}
fn text(b: &mut ParagraphBuilder, node: u64, value: &str) {
    b.push_text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 0,
        },
        value,
    );
}
fn layout(
    p: &Paragraph,
    cx: &mut LayoutContext,
    options: &LineOptions,
    atomics: &AtomicSizes,
    width: f32,
) -> Result<Vec<Line>, String> {
    let mut token = p.start_token();
    let mut block = 0.0;
    let mut lines = Vec::new();
    for _ in 0..4096 {
        let constraint = LineConstraint {
            block_offset: block,
            ..LineConstraint::new(width)
        };
        match p.next_line(cx, token, options, &constraint, atomics) {
            LineResult::Line(line) => {
                if line.break_token() == token {
                    return Err("line made no source progress".into());
                }
                token = line.break_token();
                block += line.block_size();
                lines.push(line);
            }
            LineResult::Done => return Ok(lines),
            other => return Err(format!("unsupported snapshot line outcome: {other:?}")),
        }
    }
    Err("snapshot line budget exceeded".into())
}
struct Page {
    lines: Vec<Line>,
    floats: Vec<flow::Placement>,
    offset: f32,
    fragment: usize,
}
fn prepare(
    id: &str,
    fonts: &FixtureFonts,
    cx: &mut LayoutContext,
) -> Result<(Vec<Page>, Vec<LogicalRect>, Value, bool), String> {
    let limits = Default::default();
    if let Some(case) = shodo_fixtures::case(id) {
        let p = case.build(cx, fonts, &limits).map_err(|e| e.to_string())?;
        let lines = layout(&p, cx, &Default::default(), &AtomicSizes::EMPTY, case.width)?;
        return Ok((
            vec![Page {
                lines,
                floats: vec![],
                offset: 0.0,
                fragment: 0,
            }],
            vec![],
            json!({"id":id,"text":case.text,"width":case.width,"font_size":case.font_size,"fonts":case.font_ids,"lang":case.lang,"direction":format!("{:?}",case.direction)}),
            false,
        ));
    }
    if !VARIANTS.contains(&id) {
        return Err(format!("unknown snapshot case: {id}"));
    }
    let mut style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named(FONTS[0].family.into())],
            font_size: 20.0,
            line_height: LineHeight::Px(24.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut width = 180.0;
    match id {
        "shared-ffi-color" => {
            style.root.font_size = 32.0;
            width = 200.0;
        }
        "arabic-wrap" => {
            style.root.line_height = LineHeight::Px(48.0);
            style.direction = Direction::Rtl;
            style.root.direction = Direction::Rtl;
            style.root.font_families = vec![FontFamily::Named(FONTS[2].family.into())];
            style.root.font_size = 32.0;
            width = 65.0;
        }
        "preserved-tabs" => {
            style.root.font_size = 16.0;
            style.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
            style.root.tab_size = TabSize::Px(16.0);
            width = 90.0;
        }
        "normal-white-space" => width = 90.0,
        "hanging-white-space" => {
            style.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
            width = 90.0;
        }
        "japanese-kinsoku" => {
            style.root.font_families = vec![FontFamily::Named(FONTS[1].family.into())];
            style.root.font_size = 16.0;
            style.root.lang = Some("ja".into());
            width = 90.0;
        }
        "float-pages" => {
            style.root.font_size = 16.0;
            style.root.line_height = LineHeight::Px(20.0);
            width = 80.0;
        }
        _ => {}
    }
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.with_offset_mapping(true);
    let mut atomics = AtomicSizes::new();
    let mut options = LineOptions::default();
    let input = match id {
        "shared-ffi-color" => {
            for (i, s) in ["f", "f", "i"].iter().enumerate() {
                b.open_inline(NodeId(11 + i as u64), &style.root, Default::default());
                text(&mut b, 1 + i as u64, s);
                b.close_inline();
            }
            "ffi"
        }
        "arabic-wrap" => {
            for (i, s) in ["سل", "ام ", "سلام"].iter().enumerate() {
                b.open_inline(NodeId(11 + i as u64), &style.root, Default::default());
                text(&mut b, 1 + i as u64, s);
                b.close_inline();
            }
            "سلام سلام"
        }
        "nested-atomic-baseline" => {
            text(&mut b, 1, "A");
            b.push_forced_break(NodeId(88));
            for i in 0..4 {
                let mut edges = InlineEdges::default();
                edges.padding.inline_end = 2.0;
                b.open_inline(NodeId(11 + i), &style.root, edges);
            }
            b.push_atomic(NodeId(9), &style.root, Default::default());
            text(&mut b, 2, "B");
            for _ in 0..4 {
                b.close_inline();
            }
            atomics.insert(
                NodeId(9),
                AtomicSize {
                    inline_size: 20.0,
                    block_size: 20.0,
                    baseline: Some(16.0),
                    margins: Default::default(),
                },
            );
            "A\n[atomic]B"
        }
        "preserved-tabs" => {
            let s = "One  two\tthree\nFour five.";
            text(&mut b, 1, s);
            s
        }
        "normal-white-space" => {
            let s = "One   two\n  three   four.";
            text(&mut b, 1, s);
            s
        }
        "hanging-white-space" => {
            let s = "One two   three   ";
            text(&mut b, 1, s);
            s
        }
        "indent-baseline" => {
            options.text_indent.length = 10.0;
            let s = "Alpha beta gamma delta.";
            text(&mut b, 1, s);
            s
        }
        "japanese-kinsoku" => {
            let s = "「日本語」、句読点。読みやすい文章。";
            text(&mut b, 1, s);
            s
        }
        "float-pages" => {
            b.push_out_of_flow(NodeId(2), OutOfFlowKind::Float);
            let s = "aa bb cc dd ee ff gg hh ii jj";
            text(&mut b, 1, s);
            s
        }
        _ => unreachable!(),
    };
    let settings = json!({"id":id,"text":input,"width":width,"font_size":style.root.font_size,"line_height":format!("{:?}",style.root.line_height),"direction":format!("{:?}",style.direction),"families":style.root.font_families.iter().map(|f|format!("{f:?}")).collect::<Vec<_>>(),"white_space":format!("{:?}",style.root.white_space_collapse),"tab_px":if id=="preserved-tabs"{Some(16.0)}else{None},"indent":options.text_indent.length});
    let p = b.build(cx, &fonts.collection).map_err(|e| e.to_string())?;
    if id == "float-pages" {
        return Ok((float_pages(&p, cx)?, vec![], settings, true));
    }
    let lines = layout(&p, cx, &options, &atomics, width)?;
    let annotations = if id == "shared-ffi-color" {
        let mapping = lines[0]
            .offset_mapping()
            .ok_or("missing ffi source mapping")?;
        let (start, _) = mapping
            .dom_to_text(NodeId(2), 0)
            .ok_or("missing ffi start")?;
        let (end, _) = mapping.dom_to_text(NodeId(2), 1).ok_or("missing ffi end")?;
        LineLayout::new(&lines).selection_rects(
            TextPosition {
                line: 0,
                offset: start,
                affinity: Affinity::Downstream,
            },
            TextPosition {
                line: 0,
                offset: end,
                affinity: Affinity::Upstream,
            },
        )
    } else {
        vec![]
    };
    Ok((
        vec![Page {
            lines,
            floats: vec![],
            offset: 0.0,
            fragment: 0,
        }],
        annotations,
        settings,
        false,
    ))
}
fn float_pages(p: &Paragraph, cx: &mut LayoutContext) -> Result<Vec<Page>, String> {
    use flow::{Checkpoint, Clear, Driver, FloatSpec, Outcome, Side};
    let driver = Driver::new(vec![FloatSpec {
        node: NodeId(2),
        side: Side::Right,
        clear: Clear::None,
        width: 20.0,
        height: 50.0,
    }])
    .map_err(|e| e.to_string())?;
    let state = Checkpoint::new(p, 80.0).map_err(|e| e.to_string())?;
    let rejected = driver
        .trial(
            p,
            cx,
            &state,
            &Default::default(),
            &AtomicSizes::EMPTY,
            Some(0.0),
        )
        .map_err(|e| e.to_string())?;
    if !matches!(rejected.outcome, Outcome::HeightRejected { .. })
        || rejected.state.token() != state.token()
    {
        return Err("height rejection consumed source".into());
    }
    let first = driver
        .trial(
            p,
            cx,
            &rejected.state,
            &Default::default(),
            &AtomicSizes::EMPTY,
            None,
        )
        .map_err(|e| e.to_string())?;
    let Outcome::Line(line) = first.outcome else {
        return Err("first float page produced no line".into());
    };
    let first_page = Page {
        lines: vec![line],
        floats: first.state.placed().to_vec(),
        offset: 0.0,
        fragment: 0,
    };
    let mut state = first
        .state
        .next_fragment(20.0, 100.0)
        .map_err(|e| e.to_string())?;
    let floats = state.placed().to_vec();
    let mut lines = Vec::new();
    for _ in 0..4096 {
        let trial = driver
            .trial(
                p,
                cx,
                &state,
                &Default::default(),
                &AtomicSizes::EMPTY,
                None,
            )
            .map_err(|e| e.to_string())?;
        match trial.outcome {
            Outcome::Line(line) => {
                if trial.state.token() == state.token() {
                    return Err("float page made no source progress".into());
                }
                lines.push(line);
                state = trial.state;
            }
            Outcome::Done => {
                return Ok(vec![
                    first_page,
                    Page {
                        lines,
                        floats,
                        offset: 160.0,
                        fragment: state.fragment(),
                    },
                ]);
            }
            other => return Err(format!("unexpected float page outcome: {other:?}")),
        }
    }
    Err("float page budget exceeded".into())
}
pub fn render(id: &str) -> Result<Rendered, String> {
    let limits = Default::default();
    let fonts = load_fonts(&limits).map_err(|e| e.to_string())?;
    let (pages, annotations, settings, rejected) = prepare(id, &fonts, &mut LayoutContext::new())?;
    let mut image = tiny_skia::Pixmap::new(512, 1024).ok_or("cannot allocate snapshot")?;
    image.fill(tiny_skia::Color::WHITE);
    let mut glyph_count = 0;
    let mut lines_json = Vec::new();
    let mut glyphs_json = Vec::new();
    let mut atomics_json = Vec::new();
    let mut boxes_json = Vec::new();
    let mut pages_json = Vec::new();
    for page in &pages {
        let color = |node: NodeId| {
            if node == NodeId(9) {
                [0, 128, 0, 255]
            } else if id == "shared-ffi-color" && node == NodeId(1) {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 255]
            }
        };
        let (painted, count) = paint_lines(&page.lines, &annotations, color)?;
        let used = page
            .lines
            .iter()
            .map(|l| l.block_offset() + l.block_size() + 40.0)
            .chain(
                page.floats
                    .iter()
                    .map(|f| f.rect.block_start + f.rect.block_size + 40.0),
            )
            .fold(0.0, f32::max)
            .ceil() as usize;
        let offset = page.offset as usize;
        if offset + used > 1024 {
            return Err("page panel exceeds fixed canvas".into());
        }
        let start = offset * 512 * 4;
        image.data_mut()[start..start + used * 512 * 4]
            .copy_from_slice(&painted.data()[..used * 512 * 4]);
        glyph_count += count;
        for line in &page.lines {
            let index = lines_json.len();
            let range = line.text_range();
            lines_json.push(json!({"page":page.fragment,"start":range.start,"end":range.end,"block_offset":line.block_offset(),"block_size":line.block_size(),"inline_size":line.inline_size(),"baseline":line.baseline(BaselineKind::Alphabetic)}));
            for fragment in line.fragments() {
                match fragment {
                    Fragment::GlyphRun(run) => {
                        let font = fixed_font(run)?;
                        for g in run.glyphs() {
                            glyphs_json.push(json!({"line":index,"owner":run.node().map(|n|n.0),"font":font,"size":run.font_size(),"coords":run.normalized_coords().iter().map(|c|c.to_bits()).collect::<Vec<_>>(),"embolden":run.embolden(),"skew":run.skew(),"id":g.id,"cluster":g.cluster,"inline_position":g.inline_position,"block_offset":g.block_offset,"advance":g.advance,"baseline":run.baseline()}));
                        }
                    }
                    Fragment::Atomic(a) => {
                        let r = a.border_rect;
                        atomics_json.push(json!({"line":index,"node":a.node.0,"inline_start":r.inline_start,"block_start_in_line":r.block_start,"block_start":line.block_offset()+r.block_start+page.offset,"width":r.inline_size,"height":r.block_size,"baseline":a.baseline}));
                    }
                    Fragment::InlineBox(b) => {
                        let r = b.rect;
                        boxes_json.push(json!({"line":index,"node":b.node.0,"inline_start":r.inline_start,"block_start":r.block_start,"width":r.inline_size,"height":r.block_size,"parent":b.parent,"start_edge":b.has_start_edge,"end_edge":b.has_end_edge}));
                    }
                    Fragment::OutOfFlowAnchor(_) => {}
                }
            }
            if id == "indent-baseline" {
                let rect = tiny_skia::Rect::from_xywh(
                    10.0,
                    10.0 + page.offset
                        + line.block_offset()
                        + line.baseline(BaselineKind::Alphabetic),
                    180.0,
                    1.0,
                )
                .ok_or("invalid baseline guide")?;
                let mut paint = tiny_skia::Paint::default();
                paint.set_color_rgba8(160, 160, 160, 255);
                image.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
            }
        }
        let mut floats = Vec::new();
        for f in &page.floats {
            let r = f.rect;
            floats.push(json!({"node":f.node.0,"inline_start":r.inline_start,"block_start":r.block_start,"width":r.inline_size,"height":r.block_size}));
            let rect = tiny_skia::Rect::from_xywh(
                10.0 + r.inline_start,
                10.0 + page.offset + r.block_start,
                r.inline_size,
                r.block_size,
            )
            .ok_or("invalid float rectangle")?;
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(0, 80, 220, 255);
            image.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
        }
        pages_json
            .push(json!({"fragment":page.fragment,"panel_offset":page.offset,"floats":floats}));
    }
    if glyph_count == 0
        || glyph_count != glyphs_json.len()
        || !image
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| *p != [255, 255, 255, 255])
    {
        return Err("snapshot has no valid glyph ink".into());
    }
    let geometry = json!({"schema":1,"lines":lines_json,"glyphs":glyphs_json,"atomics":atomics_json,"inline_boxes":boxes_json,"annotations":annotations.iter().map(|r|json!({"inline_start":r.inline_start,"block_start":r.block_start,"width":r.inline_size,"height":r.block_size})).collect::<Vec<_>>(),"pages":pages_json,"height_rejections":usize::from(rejected),"rejected_token_preserved":rejected});
    Ok(Rendered {
        id: id.into(),
        image,
        settings,
        geometry,
        glyph_count,
    })
}
