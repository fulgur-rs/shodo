//! Representative raikiri caller checks for the full hanging-punctuation grammar.
//! Development-only; it does not adopt or change either S4 spike.
#[allow(dead_code)]
#[path = "support/raikiri_style_diffs.rs"]
mod diagnostic;
#[allow(dead_code)]
#[path = "support/source_fonts.rs"]
mod fonts;
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
    hang_end: f32,
    glyphs: Vec<G>,
}

fn find_by_attr(input: &offline::ScreenInput, name: &str, value: &str) -> Result<usize, String> {
    let dom = &input.parsed.dom;
    (0..dom.node_count())
        .find(|&id| {
            dom.get_node(id)
                .is_some_and(|n| n.attribute(name) == Some(value))
        })
        .ok_or_else(|| format!("no element with {name}={value}"))
}

/// Lay out the IFC rooted at `root` through the caller path. `force_none`
/// replaces the root's resolved options with the default (`none`) as a control.
/// Nested inline computed overrides remain active; the original first-002
/// control measures root-level text.
fn measure(
    input: &offline::ScreenInput,
    root: usize,
    fonts: &FontCollection,
    width: f32,
    force_none: bool,
) -> Result<Vec<LineMeasure>, String> {
    let mut context = LayoutContext::new();
    let mut prepared =
        replay::project(input, root, width, &mut context, fonts, &Limits::default())?;
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
                hang_end: line.hang_end(),
                glyphs,
            });
        }
    }
    Ok(lines)
}

const TEST: &str = "css/css-text/hanging-punctuation/hanging-punctuation-first-002.html";
const REFERENCE: &str =
    "css/css-text/hanging-punctuation/reference/hanging-punctuation-first-002-ref.html";
const RAIKIRI_PIN: &str = "fe9aea9ade56ff046d38a6dddd31d02303466bfc";
/// The pinned screen viewport used by the original comparison.
const VIEWPORT_WIDTH: f32 = 800.0;

/// The WPT pass condition: the arrow (last glyph on the line) sits at the
/// same inline position in the test and in the reference.
fn arrows_aligned(test: &LineMeasure, reference: &LineMeasure) -> bool {
    match (test.glyphs.last(), reference.glyphs.last()) {
        (Some(t), Some(r)) => (t.inline_position - r.inline_position).abs() <= 1.0 / 64.0,
        _ => false,
    }
}

fn run(wpt: &std::path::Path, output: &std::path::Path) -> Result<(), String> {
    let registry = fonts::load(&wpt.join("fonts"), &Limits::default())?;
    let test = offline::parse_screen(wpt, TEST)?;
    let reference = offline::parse_screen(wpt, REFERENCE)?;
    let test_root = find_by_attr(&test, "class", "test")?;
    let dom = &reference.parsed.dom;
    let reference_root = (0..dom.node_count())
        .find(|&id| {
            dom.get_node(id)
                .is_some_and(|n| n.tag_name() == Some("div"))
        })
        .ok_or("reference has no div")?;
    let width = VIEWPORT_WIDTH;
    let hung = measure(&test, test_root, &registry.collection, width, false)?;
    let control = measure(&test, test_root, &registry.collection, width, true)?;
    let reference_lines = measure(
        &reference,
        reference_root,
        &registry.collection,
        width,
        false,
    )?;
    let (hung, control, reference_line) =
        match (hung.first(), control.first(), reference_lines.first()) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => return Err("expected one accepted line in every replay".into()),
        };
    let hung_glyph = hung.glyphs.first().ok_or("hung line has no glyph")?;
    let checks = serde_json::json!({
        "glyph_retained": hung.glyphs.len() == control.glyphs.len() && hung.glyphs.len() >= 2,
        "hang_start_equals_leading_advance": (hung.hang_start - hung_glyph.advance).abs() <= 1.0 / 64.0 && hung.hang_start > 0.0,
        "arrows_aligned_with_reference": arrows_aligned(hung, reference_line),
        "control_none_is_not_aligned": !arrows_aligned(control, reference_line),
    });
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    let describe = |l: &LineMeasure| {
        serde_json::json!({"hang_start": l.hang_start, "hang_end": l.hang_end,
            "glyphs": l.glyphs.iter().map(|g| serde_json::json!({
                "inline_position": g.inline_position, "advance": g.advance, "cluster": g.cluster
            })).collect::<Vec<_>>()})
    };
    let report = serde_json::json!({
        "scope": "shodo layout of the original static test/reference through the representative caller; not a WPT verdict, page paint or baseline PASS count",
        "raikiri_pin": RAIKIRI_PIN, "viewport_width": width,
        "test": TEST, "reference": REFERENCE,
        "font_registry_sha256": registry.hashes,
        "resources": {"test": test.resources, "reference": reference.resources},
        "resolved": {"test": describe(hung), "control_none": describe(control), "reference": describe(reference_line)},
        "checks": checks, "passed": passed,
    });
    std::fs::write(
        output,
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| e.to_string())?;
    if passed {
        Ok(())
    } else {
        Err("hanging-punctuation-first-002 replay did not reproduce the expected behavior".into())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let wpt = args
        .next()
        .ok_or("usage: hanging_punctuation <wpt-root> [output.json]")?;
    let output = args
        .next()
        .unwrap_or_else(|| "hanging-punctuation-first-002.json".into());
    run(std::path::Path::new(&wpt), std::path::Path::new(&output))?;
    println!("{output}: original test aligns with its reference; none control does not");
    Ok(())
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
            let dir =
                std::env::temp_dir().join(format!("shodo-hanging-{}-{n}", std::process::id()));
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

    fn line(hang: f32, xs: &[f32]) -> LineMeasure {
        LineMeasure {
            hang_start: hang,
            hang_end: 0.0,
            glyphs: xs
                .iter()
                .map(|&x| G {
                    inline_position: x,
                    advance: 40.0,
                    cluster: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn arrows_are_the_last_glyph_and_must_share_an_inline_position() {
        assert!(arrows_aligned(
            &line(40.0, &[-40.0, 0.0]),
            &line(0.0, &[0.0])
        ));
        assert!(!arrows_aligned(
            &line(0.0, &[0.0, 40.0]),
            &line(0.0, &[0.0])
        ));
        assert!(!arrows_aligned(&line(0.0, &[]), &line(0.0, &[0.0])));
    }

    #[test]
    fn first_on_root_hangs_the_leading_u3000_and_keeps_its_glyph() {
        let lines = lines("#root{hanging-punctuation:first}", "\u{3000}日", false);
        let line = &lines[0];
        assert_eq!(line.glyphs.len(), 2, "the hung glyph is retained");
        assert!(line.hang_start > 0.0);
        assert!(close(line.hang_start, line.glyphs[0].advance));
        assert!(close(
            line.glyphs[0].inline_position,
            -line.glyphs[0].advance
        ));
        assert!(close(line.glyphs[1].inline_position, 0.0));
    }

    #[test]
    fn absent_declaration_is_none() {
        let lines = lines("", "\u{3000}日", false);
        let line = &lines[0];
        assert_eq!(line.hang_start, 0.0);
        assert!(close(line.glyphs[0].inline_position, 0.0));
        assert!(close(
            line.glyphs[1].inline_position,
            line.glyphs[0].advance
        ));
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
    /// shodo core mirrors the edge for RTL (see `crates/shodo/tests/japanese.rs`,
    /// `rtl_hanging_uses_the_inline_start_and_end_after_mirroring`). This test
    /// only pins that the caller hangs under `direction: rtl`; it does not
    /// prove mirroring, and it is not native parity.
    #[test]
    fn rtl_leading_u3000_is_shodo_only_behavior() {
        let lines = lines(
            "#root{hanging-punctuation:first;direction:rtl}",
            "\u{3000}日",
            false,
        );
        assert!(lines[0].hang_start > 0.0);
    }

    #[test]
    fn full_keyword_sets_reach_root_options() {
        for (css, first, last, force_end, allow_end) in [
            ("none", false, false, false, false),
            ("first", true, false, false, false),
            ("last", false, true, false, false),
            ("force-end", false, false, true, false),
            ("allow-end", false, false, false, true),
            ("first last", true, true, false, false),
            ("first force-end", true, false, true, false),
            ("first allow-end", true, false, false, true),
            ("force-end last", false, true, true, false),
            ("allow-end last", false, true, false, true),
            ("first force-end last", true, true, true, false),
            ("first allow-end last", true, true, false, true),
        ] {
            let html = format!(
                "<!doctype html><style>{BASE}#root{{hanging-punctuation:{css}}}</style><div id=root>日</div>"
            );
            let dir = TempDir::new(&html);
            let input = offline::parse_screen(&dir.0, "index.html").unwrap();
            let root = find_by_attr(&input, "id", "root").unwrap();
            assert_eq!(
                input.cascade.computed[root]
                    .hanging_punctuation
                    .as_css_str(),
                css
            );
            let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
            let prepared = replay::project(
                &input,
                root,
                400.0,
                &mut LayoutContext::new(),
                &fonts.collection,
                &Limits::default(),
            )
            .unwrap();
            assert_eq!(
                prepared.options.hanging_punctuation,
                HangingPunctuation {
                    first,
                    last,
                    force_end,
                    allow_end
                },
                "{css}"
            );
        }
    }

    #[test]
    fn combined_first_last_hangs_both_retained_edge_glyphs() {
        let lines = lines("#root{hanging-punctuation:first last}", "「日」", false);
        assert_eq!(lines[0].glyphs.len(), 3);
        assert!(lines[0].hang_start > 0.0);
        assert!(lines[0].hang_end > 0.0);
    }

    #[test]
    fn inline_none_clears_inherited_first_and_last() {
        let lines = lines(
            "#root{hanging-punctuation:first last}",
            "<span style='hanging-punctuation:none'>「日」</span>",
            false,
        );
        assert_eq!(lines[0].hang_start, 0.0);
        assert_eq!(lines[0].hang_end, 0.0);
    }

    #[test]
    fn inline_combination_overrides_root_none_and_inherits_to_descendants() {
        let lines = lines(
            "#root{hanging-punctuation:none}",
            "<span style='hanging-punctuation:first last'><span>「日」</span></span>",
            false,
        );
        assert!(lines[0].hang_start > 0.0);
        assert!(lines[0].hang_end > 0.0);
    }

    #[test]
    fn force_end_hangs_a_stop_and_allow_end_keeps_it_when_it_fits() {
        let forced = lines("#root{hanging-punctuation:force-end}", "日。", false);
        let allowed = lines("#root{hanging-punctuation:allow-end}", "日。", false);
        assert_eq!(forced[0].glyphs.len(), 2);
        assert!(forced[0].hang_end > 0.0);
        assert_eq!(allowed[0].hang_end, 0.0);
    }

    #[test]
    fn force_none_control_changes_root_text_and_keeps_inline_override() {
        let root_text = lines("#root{hanging-punctuation:first}", "「日", true);
        assert_eq!(root_text[0].hang_start, 0.0);
        let inline_text = lines(
            "#root{hanging-punctuation:none}",
            "<span style='hanging-punctuation:first'>「日</span>",
            true,
        );
        assert!(inline_text[0].hang_start > 0.0);
    }

    #[test]
    fn allow_end_hangs_the_stop_when_needed_to_fit_the_original_line() {
        let allowed = lines(
            "#root{font-size:400px;hanging-punctuation:allow-end}",
            "日。",
            false,
        );
        assert_eq!(allowed.len(), 1);
        assert_eq!(allowed[0].glyphs.len(), 2);
        assert!(allowed[0].hang_end > 0.0);
    }
}
