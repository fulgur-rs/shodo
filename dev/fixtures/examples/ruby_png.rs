//! Fixed-font ruby example: ruby_png OUTPUT_DIR emits three PNG/JSON pairs.
#[path = "support/glyph_paint.rs"]
mod glyph_paint;
use serde_json::{Value, json};
use shodo::geometry::WritingMode;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, PaintStyle, ParagraphStyle};
use shodo::{
    AtomicSizes, Fragment, LayoutContext, Line, ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation,
    RubyBase, RubyContent, RubyLevel, RubyOverhang, RubyPosition, RubySpan, RubyStyle,
    RubyVisibility,
};
use shodo_fixtures::{FONTS, load_fonts};
use std::path::Path;

fn style(size: f32, color: [u8; 4]) -> InlineStyle {
    InlineStyle {
        font_size: size,
        font_families: vec![FontFamily::Named(FONTS[1].family.into())],
        paint: PaintStyle {
            color,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn content(node: u64, text: &str, s: &InlineStyle, limits: &Limits) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 40,
        },
        text,
        s,
        limits,
    )
}
fn sample(mode: WritingMode) -> Result<Vec<Line>, Box<dyn std::error::Error>> {
    let limits = Limits::default();
    let fonts = load_fonts(&limits)?;
    let base = style(24.0, [180, 0, 0, 255]);
    let reading = style(12.0, [0, 0, 180, 255]);
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            writing_mode: mode,
            root: base.clone(),
            ..Default::default()
        },
        &limits,
    );
    for (container, node, base_text, reading_text, position) in [
        (8, 10, "日本語", "にほんご", RubyPosition::Over),
        (9, 11, "読み", "よみ", RubyPosition::Under),
    ] {
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(node),
                content: content(node, base_text, &base, &limits),
                align: RubyAlign::SpaceAround,
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(node + 10),
                    content: content(node + 10, reading_text, &reading, &limits),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle {
                    position,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            }],
        )?;
        b.push_ruby(NodeId(container), &base, ruby);
    }
    let mut cx = LayoutContext::new();
    let paragraph = b.build(&mut cx, &fonts.collection)?;
    if !paragraph.warnings().is_empty() {
        return Err(format!("unexpected input warnings: {:?}", paragraph.warnings()).into());
    }
    let lines = paragraph.break_all(&mut cx, &Default::default(), 96.0, &AtomicSizes::EMPTY);
    let warnings = cx.take_warnings();
    if !warnings.is_empty() {
        return Err(format!("unexpected layout warnings: {warnings:?}").into());
    }
    Ok(lines)
}
fn metadata(line: &Line) -> Result<Value, Box<dyn std::error::Error>> {
    let mut runs = Vec::new();
    for fragment in line.fragments() {
        if let Fragment::GlyphRun(r) = fragment {
            let data = r.font_data().ok_or("retained glyph has no font bytes")?;
            let fixed = FONTS
                .iter()
                .find(|f| f.face_index == data.index && f.bytes == data.data.as_ref())
                .ok_or("accepted glyph is not a fixed-font glyph")?;
            let glyphs:Vec<_>=r.glyphs().enumerate().map(|(i,g)|json!({"id":g.id,"origin":r.glyph_origin(i),"advance":g.advance,"cluster":g.cluster})).collect();
            if r.glyphs().any(|g| g.id == 0) {
                return Err("example contains missing glyphs".into());
            }
            runs.push(json!({"node":r.node().map(|n|n.0),"text_range":[r.text_range().start,r.text_range().end],"font":fixed.id,"sha256":fixed.sha256,"face_index":data.index,"font_size":r.font_size(),"normalized_coords":r.normalized_coords().iter().map(|c|c.to_f32()).collect::<Vec<_>>(),"inline_start":r.inline_start(),"inline_size":r.inline_size(),"baseline":r.baseline(),"orientation":format!("{:?}",r.orientation()),"glyphs":glyphs}));
        }
    }
    let mut ruby = Vec::new();
    for a in line.ruby_annotations() {
        let t = a.transform();
        ruby.push(json!({"container":a.container().0,"base_nodes":a.base_nodes().iter().map(|n|n.0).collect::<Vec<_>>(),"node":a.node().map(|n|n.0),"level":a.level(),"base_text_range":[a.base_text_range().start,a.base_text_range().end],"text_range":[a.text_range().start,a.text_range().end],"visibility":format!("{:?}",a.visibility()),"origin":a.origin(),"transform":[t.inline_inline,t.inline_block,t.block_inline,t.block_block,t.inline_offset,t.block_offset],"line":metadata(a.line())?}));
    }
    let source:Vec<_>=line.offset_mapping().map(|m|m.units().iter().map(|u|json!({"node":u.node.0,"dom":[u.dom.start,u.dom.end],"text":[u.text.start,u.text.end],"kind":format!("{:?}",u.kind)})).collect()).unwrap_or_default();
    let range = line.text_range();
    let overflow = line.overflow_rect();
    Ok(
        json!({"text_range":[range.start,range.end],"text":&line.text()[range],"source":source,"block_offset":line.block_offset(),"inline_size":line.inline_size(),"block_size":line.block_size(),"overflow":[overflow.inline_start,overflow.block_start,overflow.inline_size,overflow.block_size],"runs":runs,"ruby":ruby}),
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let directory = args.next().ok_or("usage: ruby_png OUTPUT_DIR")?;
    if args.next().is_some() {
        return Err("usage: ruby_png OUTPUT_DIR".into());
    }
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory)?;
    for (name, mode) in [
        ("horizontal", WritingMode::HorizontalTb),
        ("vertical-rl", WritingMode::VerticalRl),
        ("vertical-lr", WritingMode::VerticalLr),
    ] {
        let lines = sample(mode)?;
        let (image, count) = glyph_paint::try_paint_styled_on_canvas(&lines, 256, 256)?;
        if count != 11 {
            return Err(format!("expected 11 retained glyphs, got {count}").into());
        }
        image.save_png(directory.join(format!("{name}.png")))?;
        let output = json!({"writing_mode":format!("{mode:?}"),"canvas":[256,256],"physical_origin":[10,10],"accepted_glyphs":count,"lines":lines.iter().map(metadata).collect::<Result<Vec<_>,_>>()?});
        std::fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec_pretty(&output)?,
        )?;
        println!("{name}: {} lines, {count} retained glyphs", lines.len());
    }
    Ok(())
}
