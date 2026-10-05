mod common;
use common::*;
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{
    BoxDecorationBreak, InlineStyle, LineHeight, LineOptions, ParagraphStyle, VerticalAlign,
    WhiteSpaceCollapse,
};
use shodo::{AtomicSize, AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder};

fn root(quirk: bool) -> ParagraphStyle {
    let mut s = style();
    s.root.line_height = LineHeight::Px(20.0);
    s.line_height_quirk = quirk;
    s
}

fn span(lh: f32) -> InlineStyle {
    InlineStyle {
        font_size: 10.0,
        line_height: LineHeight::Px(lh),
        ..Default::default()
    }
}

fn edges(start: f32, end: f32) -> InlineEdges {
    InlineEdges {
        padding: Sides {
            inline_start: start,
            inline_end: end,
            ..Sides::default()
        },
        ..InlineEdges::default()
    }
}

struct Doc {
    next: u64,
    atomics: AtomicSizes,
}

impl Doc {
    fn new() -> Self {
        Self {
            next: 100,
            atomics: AtomicSizes::new(),
        }
    }
    fn img(&mut self, b: &mut ParagraphBuilder, align: VerticalAlign) {
        self.next += 1;
        let id = NodeId(self.next);
        self.atomics.insert(
            id,
            AtomicSize {
                inline_size: 2.0,
                block_size: 2.0,
                ..Default::default()
            },
        );
        b.push_atomic(
            id,
            &InlineStyle {
                vertical_align: align,
                ..span(20.0)
            },
            InlineEdges::default(),
        );
    }
}

fn text(b: &mut ParagraphBuilder, s: &str) {
    b.push_text(TextSource::Generated { node: NodeId(1) }, s);
}

fn heights(
    quirk: bool,
    width: f32,
    input: impl FnOnce(&mut ParagraphBuilder, &mut Doc),
) -> Vec<f32> {
    let mut doc = Doc::new();
    let p = build(&root(quirk), |b| input(b, &mut doc));
    let mut cx = LayoutContext::new();
    let mut token = p.start_token();
    let mut out = Vec::new();
    loop {
        match p.next_line(
            &mut cx,
            token,
            &LineOptions::default(),
            &LineConstraint::new(width),
            &doc.atomics,
        ) {
            LineResult::Line(line) => {
                token = line.break_token();
                out.push(line.block_size());
            }
            LineResult::Done => {
                // A trailing forced break leaves an empty line behind (with the
                // quirk off too); the matrix is about the lines with content.
                if out.len() > 1 && out.last() == Some(&0.0) {
                    out.pop();
                }
                return out;
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(out.len() < 64, "no progress");
    }
}

fn q(width: f32, input: impl FnOnce(&mut ParagraphBuilder, &mut Doc)) -> Vec<f32> {
    heights(true, width, input)
}

const WIDE: f32 = 1000.0;

#[test]
fn issue_repro_first_fragment_is_text_free() {
    // a: <span lh40><img><br>x</span> = 2 + 40
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.push_forced_break(NodeId(3));
            text(b, "x");
            b.close_inline();
        }),
        [2.0, 40.0]
    );
}

#[test]
fn flag_off_keeps_struts() {
    assert_eq!(
        heights(false, WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.push_forced_break(NodeId(3));
            text(b, "x");
            b.close_inline();
        }),
        [40.0, 40.0]
    );
}

#[test]
fn text_free_boxes_and_root() {
    // b: <span lh40><img></span> = 2
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
        }),
        [2.0]
    );
    // f: <img><br>x = 2 + 20 (root per line)
    assert_eq!(
        q(WIDE, |b, d| {
            d.img(b, VerticalAlign::Baseline);
            b.push_forced_break(NodeId(3));
            text(b, "x");
        }),
        [2.0, 20.0]
    );
    // n: middle-aligned img in a text-free span = 2
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Middle);
            b.close_inline();
        }),
        [2.0]
    );
}

#[test]
fn forced_break_rule() {
    // c: <span lh40><img><br></span> = 2
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [2.0]
    );
    // d: <span lh40><br></span> = 40
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [40.0]
    );
    // h: x<br><br>x = 20 20 20
    assert_eq!(
        q(WIDE, |b, _| {
            text(b, "x");
            b.push_forced_break(NodeId(3));
            b.push_forced_break(NodeId(4));
            text(b, "x");
        }),
        [20.0, 20.0, 20.0]
    );
    // A: x<span lh40><br></span> = 40
    assert_eq!(
        q(WIDE, |b, _| {
            text(b, "x");
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [40.0]
    );
    // B: <span lh40>x<span lh60><br></span></span> = 60
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            text(b, "x");
            b.open_inline(NodeId(4), &span(60.0), InlineEdges::default());
            b.push_forced_break(NodeId(3));
            b.close_inline();
            b.close_inline();
        }),
        [60.0]
    );
    // D: <span lh40><img></span><br> = 2
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
            b.push_forced_break(NodeId(3));
        }),
        [2.0]
    );
    // E: <img><span lh40><br></span> = 40
    assert_eq!(
        q(WIDE, |b, d| {
            d.img(b, VerticalAlign::Baseline);
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [40.0]
    );
    // BD: <span lh40><span lh10>x</span><br></span> = 10
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            b.open_inline(NodeId(4), &span(10.0), InlineEdges::default());
            text(b, "x");
            b.close_inline();
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [10.0]
    );
    // Y: <span lh10><br></span> = 10 (root not credited)
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &span(10.0), InlineEdges::default());
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [10.0]
    );
}

#[test]
fn edges_are_border_and_padding_on_the_unit_line() {
    // l: padding-right keeps the strut = 40
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), edges(0.0, 1.0));
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
        }),
        [40.0]
    );
    // H: margins do not = 2
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(
                NodeId(2),
                &span(40.0),
                InlineEdges {
                    margin: Sides {
                        inline_start: 1.0,
                        inline_end: 1.0,
                        ..Sides::default()
                    },
                    ..InlineEdges::default()
                },
            );
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
        }),
        [2.0]
    );
    // m/CI: wrapping span, padding on the end only: only the last line keeps it.
    for decoration in [BoxDecorationBreak::Slice, BoxDecorationBreak::Clone] {
        let s = InlineStyle {
            box_decoration_break: decoration,
            ..span(40.0)
        };
        let lines = q(3.0, |b, d| {
            b.open_inline(NodeId(2), &s, edges(0.0, 1.0));
            for _ in 0..3 {
                d.img(b, VerticalAlign::Baseline);
            }
            text(b, "x");
            b.close_inline();
        });
        assert_eq!(lines.first(), Some(&2.0), "{decoration:?}: {lines:?}");
        assert_eq!(lines.last(), Some(&40.0), "{decoration:?}: {lines:?}");
        // CG/CH: padding-left: the first and the text line keep it.
        let lines = q(3.0, |b, d| {
            b.open_inline(NodeId(2), &s, edges(1.0, 0.0));
            for _ in 0..3 {
                d.img(b, VerticalAlign::Baseline);
            }
            text(b, "x");
            b.close_inline();
        });
        assert_eq!(lines.first(), Some(&40.0), "{decoration:?}: {lines:?}");
        assert!(
            lines[1..lines.len() - 1].iter().all(|h| *h == 2.0),
            "{decoration:?}: {lines:?}"
        );
    }
}

#[test]
fn white_space_presence() {
    // k: <span lh40 pre><img> </span> = 40
    let pre = InlineStyle {
        white_space_collapse: WhiteSpaceCollapse::Preserve,
        ..span(40.0)
    };
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &pre, InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            text(b, " ");
            b.close_inline();
        }),
        [40.0]
    );
    // K: <span lh40 pre>\t</span> = 40
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &pre, InlineEdges::default());
            text(b, "\t");
            b.close_inline();
        }),
        [40.0]
    );
    // X: x<span lh40> </span>y = 40 (interior collapsible space)
    assert_eq!(
        q(WIDE, |b, _| {
            text(b, "x");
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            text(b, " ");
            b.close_inline();
            text(b, "y");
        }),
        [40.0]
    );
    // L: <span lh40><img> </span>x = 40
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            text(b, " ");
            b.close_inline();
            text(b, "x");
        }),
        [40.0]
    );
    // F: <span lh40>&shy;</span>x = 40 (a lone soft hyphen line is empty
    // and 0 high regardless of the quirk, so keep text beside it)
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            text(b, "\u{ad}");
            b.close_inline();
            text(b, "x");
        }),
        [40.0]
    );
    // BI: x<span lh40 pre-line> </span> = 20 (trailing space removed)
    let pre_line = InlineStyle {
        white_space_collapse: WhiteSpaceCollapse::PreserveBreaks,
        ..span(40.0)
    };
    assert_eq!(
        q(WIDE, |b, _| {
            text(b, "x");
            b.open_inline(NodeId(2), &pre_line, InlineEdges::default());
            text(b, " ");
            b.close_inline();
        }),
        [20.0]
    );
}

#[test]
fn zero_width_trailing_space_is_not_text() {
    // W: x<span lh40 font-size:0> </span> = 20
    let zero = InlineStyle {
        font_size: 0.0,
        ..span(40.0)
    };
    assert_eq!(
        q(WIDE, |b, _| {
            text(b, "x");
            b.open_inline(NodeId(2), &zero, InlineEdges::default());
            text(b, " ");
            b.close_inline();
        }),
        [20.0]
    );
}

#[test]
fn trailing_space_at_a_soft_wrap_is_not_text() {
    // j: wrapping <img><img> <img><img>x keeps 2px image lines.
    let lines = q(3.0, |b, d| {
        b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
        d.img(b, VerticalAlign::Baseline);
        d.img(b, VerticalAlign::Baseline);
        text(b, " ");
        d.img(b, VerticalAlign::Baseline);
        d.img(b, VerticalAlign::Baseline);
        text(b, "x");
        b.close_inline();
    });
    assert!(
        lines[..lines.len() - 1].iter().all(|h| *h == 2.0),
        "{lines:?}"
    );
    assert_eq!(lines.last(), Some(&40.0));
}

#[test]
fn empty_top_aligned_span_adds_no_height() {
    // S: <span lh40 va:top></span>x = 20, and no panic.
    let top = InlineStyle {
        vertical_align: VerticalAlign::Top,
        ..span(40.0)
    };
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &top, InlineEdges::default());
            b.close_inline();
            text(b, "x");
        }),
        [20.0]
    );
    // R: <span lh40 va:top><img></span> = 2
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &top, InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
        }),
        [2.0]
    );
}

#[test]
fn vertical_align_children_of_suppressed_boxes_keep_matching_cases() {
    // CC/DG/DH/CB/CD/CM/BP/BQ/CA: Chromium 2.
    for (outer, inner) in [
        (VerticalAlign::Baseline, VerticalAlign::TextTop),
        (VerticalAlign::Baseline, VerticalAlign::TextBottom),
        (VerticalAlign::Baseline, VerticalAlign::Middle),
        (VerticalAlign::Baseline, VerticalAlign::Sub),
        (VerticalAlign::TextTop, VerticalAlign::Baseline),
        (VerticalAlign::Top, VerticalAlign::Baseline),
        (VerticalAlign::Bottom, VerticalAlign::Baseline),
    ] {
        let s = InlineStyle {
            vertical_align: outer,
            ..span(40.0)
        };
        assert_eq!(
            q(WIDE, |b, d| {
                b.open_inline(NodeId(2), &s, InlineEdges::default());
                d.img(b, inner);
                b.push_forced_break(NodeId(3));
                b.close_inline();
            }),
            [2.0],
            "{outer:?}/{inner:?}"
        );
    }
    // CE: <span lh60><span lh40 va:top><img></span><br></span> — Chromium 60;
    // same shape as BA, so shodo gives 2 until shodo-9kt.
    let top = InlineStyle {
        vertical_align: VerticalAlign::Top,
        ..span(40.0)
    };
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(60.0), InlineEdges::default());
            b.open_inline(NodeId(4), &top, InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [2.0]
    );
}

/// Known differences from Chromium 152 (shodo-9kt): Blink credits a parent
/// through pending top/bottom (and empty text-top/bottom) descendants. Flip
/// these when shodo-9kt lands.
#[test]
fn known_pending_vertical_align_differences() {
    let top = InlineStyle {
        vertical_align: VerticalAlign::Top,
        ..span(20.0)
    };
    // BA: <span lh40><span va:top><img></span><br></span> — Chromium 40.
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            b.open_inline(NodeId(4), &top, InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [2.0]
    );
    // BB: <span lh40><img va:top><br></span> — Chromium 40.
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Top);
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [2.0]
    );
    // CP: <span lh40><span va:top></span><br></span> — Chromium 40.
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            b.open_inline(NodeId(4), &top, InlineEdges::default());
            b.close_inline();
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [40.0]
    );
    // CO: <span lh40 va:top></span><br> — Chromium 0; shodo credits the root.
    let top40 = InlineStyle {
        vertical_align: VerticalAlign::Top,
        ..span(40.0)
    };
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &top40, InlineEdges::default());
            b.close_inline();
            b.push_forced_break(NodeId(3));
        }),
        [20.0]
    );
}

/// First atomic's (top offset from the line's alphabetic baseline, gap from
/// its bottom edge to the line bottom, line top offset to its top edge, line height).
fn img_position(
    quirk: bool,
    input: impl FnOnce(&mut ParagraphBuilder, &mut Doc),
) -> (f32, f32, f32, f32) {
    let mut doc = Doc::new();
    let p = build(&root(quirk), |b| input(b, &mut doc));
    let mut cx = LayoutContext::new();
    let line = first_line(&p, &mut cx, &doc);
    let h = line.block_size();
    for f in line.fragments() {
        if let shodo::Fragment::Atomic(a) = f {
            let r = a.margin_rect;
            return (
                r.block_start - line.baseline(shodo::geometry::BaselineKind::Alphabetic),
                h - (r.block_start + r.block_size),
                r.block_start,
                h,
            );
        }
    }
    panic!("no atomic fragment");
}

fn first_line(p: &shodo::Paragraph, cx: &mut LayoutContext, doc: &Doc) -> shodo::Line {
    match p.next_line(
        cx,
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(WIDE),
        &doc.atomics,
    ) {
        LineResult::Line(l) => l,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn suppressed_boxes_keep_their_vertical_align_shifts() {
    // x<span lh40><img va></span>: the root text sizes the quirk line (20)
    // while the span's strut is suppressed. The span still shifts its
    // descendants, so the image sits at the same offset from the baseline
    // as without the quirk.
    for align in [
        VerticalAlign::Middle,
        VerticalAlign::TextTop,
        VerticalAlign::Sub,
    ] {
        let input = |b: &mut ParagraphBuilder, d: &mut Doc| {
            text(b, "x");
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, align);
            b.close_inline();
        };
        let on = img_position(true, input);
        let off = img_position(false, input);
        assert_eq!(on.0, off.0, "{align:?}: {on:?} vs {off:?}");
    }
}

#[test]
fn ghost_groups_position_without_sizing() {
    // x<span lh40 va:top><img va:middle></span>: the span is suppressed but
    // its group is still top aligned, and the quirk line is 20 high (flag
    // off: 40).
    let top = InlineStyle {
        vertical_align: VerticalAlign::Top,
        ..span(40.0)
    };
    let input = |b: &mut ParagraphBuilder, d: &mut Doc| {
        text(b, "x");
        b.open_inline(NodeId(2), &top, InlineEdges::default());
        d.img(b, VerticalAlign::Middle);
        b.close_inline();
    };
    let on = img_position(true, input);
    let off = img_position(false, input);
    assert_eq!(on.3, 20.0);
    // Only the image sizes its group, so it sits flush with the line top;
    // with the strut (flag off) it is centered inside the 40px strut instead.
    assert_eq!(on.2, 0.0, "{on:?} vs {off:?}");
    assert_ne!(off.2, 0.0);
    // The same with va:bottom: the image sits flush with the line bottom.
    let bottom = InlineStyle {
        vertical_align: VerticalAlign::Bottom,
        ..span(40.0)
    };
    let input = |b: &mut ParagraphBuilder, d: &mut Doc| {
        text(b, "x");
        b.open_inline(NodeId(2), &bottom, InlineEdges::default());
        d.img(b, VerticalAlign::Baseline);
        b.close_inline();
    };
    let on = img_position(true, input);
    let off = img_position(false, input);
    assert_eq!(on.1, 0.0, "{on:?} vs {off:?}");
    // An empty bottom aligned span is a ghost with no members: no panic.
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &bottom, InlineEdges::default());
            b.close_inline();
            text(b, "x");
        }),
        [20.0]
    );
}
