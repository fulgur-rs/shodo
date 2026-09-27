//! Fixed-font float caller example; no DOM or CSS parsing is involved.
#[path = "support/float_flow.rs"]
pub mod flow;
#[path = "support/glyph_paint.rs"]
mod glyph_paint;
use flow::{Checkpoint, Clear, Driver, FloatSpec, Outcome, Side};
use shodo::{AtomicSizes, LayoutContext, Line, ParagraphBuilder};
use shodo::{
    limits::Limits,
    node::{NodeId, OutOfFlowKind, TextSource},
    style::{FontFamily, LineHeight, LineOptions, ParagraphStyle},
};

pub fn sample() -> Result<(Vec<Line>, Checkpoint), Box<dyn std::error::Error>> {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits)?;
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())];
    style.root.font_size = 16.0;
    style.root.line_height = LineHeight::Px(20.0);
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(1) }, "A short line with ")
        .push_out_of_flow(NodeId(3), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(1) }, "floats around fixed font text. The caller keeps all checkpoints and retries the same line.");
    let p = builder.build(&mut LayoutContext::new(), &fonts.collection)?;
    let d = Driver::new(vec![
        FloatSpec {
            node: NodeId(2),
            side: Side::Left,
            clear: Clear::None,
            width: 40.0,
            height: 40.0,
        },
        FloatSpec {
            node: NodeId(3),
            side: Side::Right,
            clear: Clear::None,
            width: 30.0,
            height: 20.0,
        },
    ])?;
    let mut state = Checkpoint::new(&p, 200.0)?;
    let mut lines = Vec::new();
    let mut cx = LayoutContext::new();
    for _ in 0..100 {
        let t = d.trial(
            &p,
            &mut cx,
            &state,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            None,
        )?;
        match t.outcome {
            Outcome::Line(line) => {
                lines.push(line);
                state = t.state;
            }
            Outcome::Done => return Ok((lines, state)),
            other => return Err(format!("unexpected sample outcome: {other:?}").into()),
        }
    }
    Err("sample line bound exceeded".into())
}
pub fn paint(
    lines: &[Line],
    state: &Checkpoint,
) -> Result<tiny_skia::Pixmap, Box<dyn std::error::Error>> {
    let (mut image, _) = glyph_paint::try_paint(lines, |_| [0, 0, 0, 255], &[])?;
    for p in state.placed() {
        let r = p.rect;
        let rect = tiny_skia::Rect::from_xywh(
            10.0 + r.inline_start,
            10.0 + r.block_start,
            r.inline_size,
            r.block_size,
        )
        .ok_or("invalid float rectangle")?;
        let mut paint = tiny_skia::Paint::default();
        if p.node == NodeId(2) {
            paint.set_color_rgba8(0, 80, 220, 255);
        } else {
            paint.set_color_rgba8(0, 150, 80, 255);
        }
        image.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
    }
    Ok(image)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "target/shodo-floats.png".into());
    let (lines, state) = sample()?;
    let image = paint(&lines, &state)?;
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    image.save_png(output)?;
    Ok(())
}
