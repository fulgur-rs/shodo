//! Caller CBDT/CBLC PNG example using retained, accepted glyph output.
//! cargo run -p shodo-fixtures --example emoji_png -- OUTPUT.png
#[path = "support/glyph_paint.rs"]
mod glyph_paint;
use shodo::{AtomicSizes, Fragment, LayoutContext};
use shodo_fixtures::{emoji_cases, load_emoji_fonts};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let output = args.next().ok_or("usage: emoji_png OUTPUT.png")?;
    if args.next().is_some() {
        return Err("usage: emoji_png OUTPUT.png".into());
    }
    let limits = Default::default();
    let fonts = load_emoji_fonts(&limits)?;
    let case = emoji_cases()
        .iter()
        .find(|c| c.id == "emoji-mixed")
        .expect("fixed mixed emoji corpus");
    let mut cx = LayoutContext::new();
    let paragraph = case.build(&mut cx, &fonts.base, &limits)?;
    if !paragraph.warnings().is_empty() {
        return Err(format!("unexpected fixture warnings: {:?}", paragraph.warnings()).into());
    }
    let lines = paragraph.break_all(
        &mut cx,
        &Default::default(),
        case.width,
        &AtomicSizes::EMPTY,
    );
    let height = lines
        .iter()
        .map(|l| l.block_offset() + l.block_size())
        .fold(0., f32::max)
        .ceil() as u32
        + 40;
    let (image, count) =
        glyph_paint::try_paint_styled_on_canvas(&lines, case.width.ceil() as u32 + 40, height)?;
    let colored = image
        .pixels()
        .iter()
        .filter(|p| p.red() != p.green() && p.green() != p.blue())
        .count();
    if colored == 0 {
        return Err("accepted glyph output produced no intrinsic color pixels".into());
    }
    image.save_png(&output)?;
    let runs:Vec<_> = lines.iter().enumerate().flat_map(|(line,l)| {
        l.fragments().filter_map(move |f| if let Fragment::GlyphRun(r)=f {
            let data=r.font_data().expect("accepted font retained");
            Some(serde_json::json!({
                "line":line,"font":format!("{:?}",r.font()),"face_index":data.index,
                "font_size":r.font_size(),"text_range":[r.text_range().start,r.text_range().end],
                "normalized_coords":r.normalized_coords().iter().map(|c|c.to_f32()).collect::<Vec<_>>(),
                "glyphs":r.glyphs().enumerate().map(|(i,g)|serde_json::json!({
                    "id":g.id,"origin":r.glyph_origin(i),"advance":g.advance,"cluster":g.cluster,
                })).collect::<Vec<_>>()
            }))
        } else {None})
    }).collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "output":output,"case":case.id,"accepted_glyphs":count,"intrinsic_color_pixels":colored,
            "format":"CBDT/CBLC PNG plus outlines","runs":runs
        }))?
    );
    Ok(())
}
