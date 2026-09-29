//! Representative raikiri caller check for `hanging-punctuation: none | first`.
//! Development-only; it does not adopt or change either S4 spike.
#[allow(dead_code)]
#[path = "support/raikiri_style_diffs.rs"]
mod diagnostic;
#[allow(dead_code)]
#[path = "support/offline_wpt.rs"]
mod offline;
#[allow(dead_code)]
#[path = "support/source_replay.rs"]
mod replay;

use shodo::{
    AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, font::FontCollection,
    limits::Limits, style::HangingPunctuation,
};

#[derive(Clone, Copy, Debug)]
struct G {
    inline_position: f32,
    advance: f32,
    cluster: u32,
}

#[derive(Clone, Debug)]
struct LineMeasure {
    hang_start: f32,
    glyphs: Vec<G>,
}

fn find_by_attr(input: &offline::ScreenInput, name: &str, value: &str) -> Result<usize, String> {
    let dom = &input.parsed.dom;
    (0..dom.node_count())
        .find(|&id| dom.get_node(id).is_some_and(|n| n.attribute(name) == Some(value)))
        .ok_or_else(|| format!("no element with {name}={value}"))
}

/// Lay out the IFC rooted at `root` through the caller path. `force_none`
/// replaces the resolved options with the default (`none`) as a control.
fn measure(
    input: &offline::ScreenInput,
    root: usize,
    fonts: &FontCollection,
    width: f32,
    force_none: bool,
) -> Result<Vec<LineMeasure>, String> {
    let mut context = LayoutContext::new();
    let mut prepared = replay::project(input, root, width, &mut context, fonts, &Limits::default())?;
    if force_none {
        prepared.options.hanging_punctuation = HangingPunctuation::default();
    }
    let p = &prepared.paragraph;
    let events: Vec<_> = p
        .lines(
            &mut context,
            p.start_token(),
            &prepared.options,
            |_, offset| {
                let mut c = LineConstraint::new(width);
                c.block_offset = offset;
                c
            },
            &AtomicSizes::EMPTY,
        )
        .collect();
    let mut lines = Vec::new();
    for event in &events {
        if let LineResult::Line(line) = event {
            let mut glyphs = Vec::new();
            for fragment in line.fragments() {
                if let Fragment::GlyphRun(run) = fragment {
                    glyphs.extend(run.glyphs().map(|g| G {
                        inline_position: g.inline_position,
                        advance: g.advance,
                        cluster: g.cluster,
                    }));
                }
            }
            lines.push(LineMeasure {
                hang_start: line.hang_start(),
                glyphs,
            });
        }
    }
    Ok(lines)
}

fn main() {
    eprintln!("Run `cargo test -p shodo-raikiri --example hanging_punctuation`.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(html: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "shodo-hanging-{}-{n}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("index.html"), html).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const BASE: &str = "#root{font-family:'Shodo Fixture CJK';font-size:16px;line-height:1}";

    /// Lay out `<div id=root>{body}</div>` under `css`, 400px wide.
    fn lines(css: &str, body: &str, force_none: bool) -> Vec<LineMeasure> {
        let html = format!(
            "<!doctype html><style>{BASE}{css}</style><div id=outer><div id=root>{body}</div></div>"
        );
        let dir = TempDir::new(&html);
        let input = offline::parse_screen(&dir.0, "index.html").unwrap();
        let root = find_by_attr(&input, "id", "root").unwrap();
        let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
        measure(&input, root, &fonts.collection, 400.0, force_none).unwrap()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1.0 / 64.0
    }

    #[test]
    fn first_on_root_hangs_the_leading_u3000_and_keeps_its_glyph() {
        let lines = lines("#root{hanging-punctuation:first}", "\u{3000}日", false);
        let line = &lines[0];
        assert_eq!(line.glyphs.len(), 2, "the hung glyph is retained");
        assert!(line.hang_start > 0.0);
        assert!(close(line.hang_start, line.glyphs[0].advance));
        assert!(close(line.glyphs[0].inline_position, -line.glyphs[0].advance));
        assert!(close(line.glyphs[1].inline_position, 0.0));
    }

    #[test]
    fn absent_declaration_is_none() {
        let lines = lines("", "\u{3000}日", false);
        let line = &lines[0];
        assert_eq!(line.hang_start, 0.0);
        assert!(close(line.glyphs[0].inline_position, 0.0));
        assert!(close(line.glyphs[1].inline_position, line.glyphs[0].advance));
    }

    #[test]
    fn inherited_first_reaches_the_ifc_root() {
        let lines = lines("#outer{hanging-punctuation:first}", "\u{3000}日", false);
        assert!(lines[0].hang_start > 0.0);
    }

    #[test]
    fn explicit_none_overrides_inherited_first() {
        let lines = lines(
            "#outer{hanging-punctuation:first}#root{hanging-punctuation:none}",
            "\u{3000}日",
            false,
        );
        assert_eq!(lines[0].hang_start, 0.0);
    }

    #[test]
    fn force_none_control_moves_the_following_glyph_by_the_advance() {
        let css = "#root{hanging-punctuation:first}";
        let hung = lines(css, "\u{3000}日", false);
        let control = lines(css, "\u{3000}日", true);
        assert!(close(
            control[0].glyphs[1].inline_position - hung[0].glyphs[1].inline_position,
            hung[0].glyphs[0].advance
        ));
    }

    #[test]
    fn line_after_forced_break_does_not_hang() {
        let lines = lines(
            "#root{hanging-punctuation:first}",
            "\u{3000}日<br>\u{3000}日",
            false,
        );
        assert_eq!(lines.len(), 2);
        assert!(lines[0].hang_start > 0.0);
        assert_eq!(lines[1].hang_start, 0.0);
        assert!(close(lines[1].glyphs[0].inline_position, 0.0));
    }

    #[test]
    fn mid_line_u3000_is_not_hung() {
        let lines = lines("#root{hanging-punctuation:first}", "日\u{3000}日", false);
        assert_eq!(lines[0].hang_start, 0.0);
        assert_eq!(lines[0].glyphs[0].cluster, 0);
        assert!(close(lines[0].glyphs[0].inline_position, 0.0));
    }

    /// Characterization: raikiri-paint only hangs a leading U+3000 in LTR text.
    /// shodo mirrors the edge for RTL, so this is not native parity.
    #[test]
    fn rtl_leading_u3000_is_shodo_only_behavior() {
        let lines = lines(
            "#root{hanging-punctuation:first;direction:rtl}",
            "\u{3000}日",
            false,
        );
        assert!(lines[0].hang_start > 0.0);
    }
}
