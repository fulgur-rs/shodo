mod common;
use common::*;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{
    BoxDecorationBreak, InlineStyle, LineHeight, LineOptions, ParagraphStyle, VerticalAlign,
    WhiteSpaceCollapse,
};
use shodo::{
    AtomicSize, AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder, Ruby,
    RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan,
};

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
fn styled_break_and_forced_root_strut_keep_independent_line_heights() {
    use shodo::geometry::WritingMode;
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        for force in [false, true] {
            for nested in [false, true] {
                for atomic in [false, true] {
                    for first in [false, true] {
                        let mut s = root(true);
                        s.writing_mode = mode;
                        s.force_root_strut = force;
                        s.first_line = first.then(|| span(30.0));
                        let mut doc = Doc::new();
                        let p = build(&s, |b| {
                            if nested {
                                b.open_inline(NodeId(2), &span(80.0), Default::default());
                                b.open_inline(NodeId(5), &span(60.0), Default::default());
                            }
                            if atomic {
                                doc.img(b, VerticalAlign::Baseline);
                            }
                            b.push_forced_break_with_style(NodeId(3), &span(10.0));
                            b.push_forced_break_with_style(NodeId(4), &span(10.0));
                            if nested {
                                b.close_inline().close_inline();
                            }
                        });
                        let lines = p.break_all(
                            &mut LayoutContext::new(),
                            &LineOptions::default(),
                            WIDE,
                            &doc.atomics,
                        );
                        let root_height = if first { 30.0 } else { 20.0 };
                        let expected = [
                            if force || (!nested && !atomic) {
                                root_height
                            } else {
                                10.0
                            },
                            if force || !nested { 20.0 } else { 10.0 },
                        ];
                        assert_eq!(
                            [lines[0].block_size(), lines[1].block_size()],
                            expected,
                            "{mode:?}/force={force}/nested={nested}/atomic={atomic}/first={first}"
                        );
                        for (line, node) in lines[..2].iter().zip([3, 4]) {
                            let br = line.forced_break().unwrap();
                            assert_eq!(br.node, NodeId(node));
                            assert_eq!(br.style.line_height, LineHeight::Px(10.0));
                            assert!(br.has_own_style);
                        }
                        if nested {
                            assert_eq!(lines.len(), 3);
                            assert_eq!(lines[2].block_size(), 0.0);
                            assert!(lines[2].forced_break().is_none());
                        } else {
                            assert_eq!(lines.len(), 2);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn styled_break_credits_itself_without_crediting_text_free_ancestors() {
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            b.open_inline(NodeId(4), &span(60.0), InlineEdges::default());
            b.push_forced_break_with_style(NodeId(3), &span(10.0));
            b.close_inline().close_inline();
        }),
        [10.0]
    );
}

#[test]
fn styled_break_directly_in_root_keeps_the_root_break_rule() {
    assert_eq!(
        q(WIDE, |b, _| {
            b.push_forced_break_with_style(NodeId(3), &span(10.0));
        }),
        [20.0]
    );
    assert_eq!(
        q(WIDE, |b, d| {
            d.img(b, VerticalAlign::Baseline);
            b.push_forced_break_with_style(NodeId(3), &span(10.0));
        }),
        [10.0]
    );
}

#[test]
fn styled_break_owns_a_strut_even_after_text_or_an_atomic() {
    for atomic in [false, true] {
        assert_eq!(
            q(WIDE, |b, d| {
                b.open_inline(NodeId(2), &span(10.0), InlineEdges::default());
                if atomic {
                    d.img(b, VerticalAlign::Baseline);
                } else {
                    text(b, "x");
                }
                b.push_forced_break_with_style(NodeId(3), &span(40.0));
                b.close_inline();
            }),
            [40.0]
        );
    }
}

#[test]
fn styled_break_keeps_its_own_strut_when_it_interns_to_the_parent_style() {
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.push_forced_break_with_style(NodeId(3), &span(40.0));
            b.close_inline();
        }),
        [40.0]
    );
}

#[test]
fn styled_break_metadata_needs_no_wrapper_and_is_absent_on_the_terminal_line() {
    let p = build(&root(true), |b| {
        b.push_forced_break_with_style(NodeId(3), &span(40.0));
    });
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        WIDE,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].block_size(), 40.0);
    assert_eq!(lines[0].forced_break().unwrap().node, NodeId(3));
    assert_eq!(lines[0].fragments().len(), 0);

    // A real ancestor's pending close produces the terminal empty line.
    let p = build(&root(true), |b| {
        b.open_inline(NodeId(2), &span(60.0), InlineEdges::default());
        b.push_forced_break_with_style(NodeId(3), &span(40.0));
        b.close_inline();
    });
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        WIDE,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].forced_break().unwrap().node, NodeId(3));
    assert_eq!(lines[1].block_size(), 0.0);
    assert!(lines[1].forced_break().is_none());
}

#[test]
fn styled_break_metadata_tracks_first_line_style_and_keeps_text_style() {
    let mut style = root(true);
    style.first_line = Some(span(30.0));
    let p = build(&style, |b| {
        b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
        b.push_forced_break_with_style(NodeId(3), &span(20.0));
        b.push_forced_break_with_style(NodeId(4), &span(10.0));
        text(b, "x");
        b.close_inline();
    });
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        WIDE,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        lines.iter().map(|l| l.block_size()).collect::<Vec<_>>(),
        [30.0, 10.0, 40.0]
    );
    for (i, line) in lines[..2].iter().enumerate() {
        let br = line.forced_break().unwrap();
        assert_eq!(br.node, NodeId(3 + i as u64));
        assert_eq!(
            br.style.line_height,
            LineHeight::Px(if i == 0 { 30.0 } else { 10.0 })
        );
        assert_eq!(&line.text()[br.text_range], "\n");
        assert!(br.has_own_style);
        assert_eq!(line.fragments().len(), 1, "only the existing ancestor box");
        assert!(
            line.fragments()
                .all(|f| matches!(f, shodo::Fragment::InlineBox(_)))
        );
    }
    assert!(lines[2].forced_break().is_none());
}

#[test]
fn legacy_and_preserved_break_metadata_use_the_current_style() {
    let mut style = root(true);
    style.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let p = build(&style, |b| {
        b.push_forced_break(NodeId(3));
        text(b, "\nx");
    });
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        WIDE,
        &AtomicSizes::EMPTY,
    );
    for (i, line) in lines[..2].iter().enumerate() {
        let br = line.forced_break().unwrap();
        assert_eq!(br.node, if i == 0 { NodeId(3) } else { NodeId(1) });
        assert_eq!(br.style.line_height, LineHeight::Px(20.0));
        assert!(!br.has_own_style);
    }
    assert!(lines[2].forced_break().is_none());
}

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
    // C: <span lh40>x</span><span lh10><br></span> = 40
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            text(b, "x");
            b.close_inline();
            b.open_inline(NodeId(4), &span(10.0), InlineEdges::default());
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [40.0]
    );
    // g: <img><br> = 2
    assert_eq!(
        q(WIDE, |b, d| {
            d.img(b, VerticalAlign::Baseline);
            b.push_forced_break(NodeId(3));
        }),
        [2.0]
    );
    // i: <br> = 20
    assert_eq!(
        q(WIDE, |b, _| {
            b.push_forced_break(NodeId(3));
        }),
        [20.0]
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
    // I: <span lh40 padding-right:1px><img></span> = 40
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), edges(0.0, 1.0));
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
        }),
        [40.0]
    );
    // J: <span lh40 border-left:1px><img></span> = 40
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(
                NodeId(2),
                &span(40.0),
                InlineEdges {
                    border: Sides {
                        inline_start: 1.0,
                        ..Sides::default()
                    },
                    ..InlineEdges::default()
                },
            );
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
    // CJ: clone, padding on both sides, wrapping = 40 2 2 40 (84): the
    // cloned edges on the continuation lines do not keep the strut.
    let clone = InlineStyle {
        box_decoration_break: BoxDecorationBreak::Clone,
        ..span(40.0)
    };
    assert_eq!(
        q(3.0, |b, d| {
            b.open_inline(NodeId(2), &clone, edges(1.0, 1.0));
            for _ in 0..3 {
                d.img(b, VerticalAlign::Baseline);
            }
            text(b, "x");
            b.close_inline();
        }),
        [40.0, 2.0, 2.0, 40.0]
    );
    // e: wrapping <span lh40><img><img><img><img>x</span> = 2 2 2 2 40 (48)
    assert_eq!(
        q(3.0, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            for _ in 0..4 {
                d.img(b, VerticalAlign::Baseline);
            }
            text(b, "x");
            b.close_inline();
        }),
        [2.0, 2.0, 2.0, 2.0, 40.0]
    );
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
    // M: x<br><span lh40> <img></span> = 20 2 (22): the space after the
    // forced break is removed at the line start, so it is not text.
    assert_eq!(
        q(WIDE, |b, d| {
            text(b, "x");
            b.push_forced_break(NodeId(3));
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            text(b, " ");
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
        }),
        [20.0, 2.0]
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
    // The matrix cases, Chromium 2 each.
    let va = |align| InlineStyle {
        vertical_align: align,
        ..span(20.0)
    };
    let va40 = |align| InlineStyle {
        vertical_align: align,
        ..span(40.0)
    };
    type Case = Box<dyn Fn(&mut ParagraphBuilder, &mut Doc)>;
    let cases: [(&str, Case); 9] = [
        // CC: <span lh40><span va:text-top><img></span><br></span>
        (
            "CC",
            Box::new(move |b, d| {
                b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
                b.open_inline(
                    NodeId(4),
                    &va(VerticalAlign::TextTop),
                    InlineEdges::default(),
                );
                d.img(b, VerticalAlign::Baseline);
                b.close_inline();
                b.push_forced_break(NodeId(3));
                b.close_inline();
            }),
        ),
        // BP: <span lh40><span va:sub><img></span><br></span>
        (
            "BP",
            Box::new(move |b, d| {
                b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
                b.open_inline(NodeId(4), &va(VerticalAlign::Sub), InlineEdges::default());
                d.img(b, VerticalAlign::Baseline);
                b.close_inline();
                b.push_forced_break(NodeId(3));
                b.close_inline();
            }),
        ),
        // DG: <span lh40><img va:text-bottom><br></span>
        (
            "DG",
            Box::new(|b, d| {
                b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
                d.img(b, VerticalAlign::TextBottom);
                b.push_forced_break(NodeId(3));
                b.close_inline();
            }),
        ),
        // DH: <span lh40><img va:middle><br></span>
        (
            "DH",
            Box::new(|b, d| {
                b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
                d.img(b, VerticalAlign::Middle);
                b.push_forced_break(NodeId(3));
                b.close_inline();
            }),
        ),
        // CA: <img va:top><br>
        (
            "CA",
            Box::new(|b, d| {
                d.img(b, VerticalAlign::Top);
                b.push_forced_break(NodeId(3));
            }),
        ),
        // CB: <span lh40 va:text-top><img></span><br>
        (
            "CB",
            Box::new(move |b, d| {
                b.open_inline(
                    NodeId(2),
                    &va40(VerticalAlign::TextTop),
                    InlineEdges::default(),
                );
                d.img(b, VerticalAlign::Baseline);
                b.close_inline();
                b.push_forced_break(NodeId(3));
            }),
        ),
        // CD: <span va:top><img></span><br>
        (
            "CD",
            Box::new(move |b, d| {
                b.open_inline(NodeId(2), &va(VerticalAlign::Top), InlineEdges::default());
                d.img(b, VerticalAlign::Baseline);
                b.close_inline();
                b.push_forced_break(NodeId(3));
            }),
        ),
        // CM: <span lh40 va:bottom><img></span><br>
        (
            "CM",
            Box::new(move |b, d| {
                b.open_inline(
                    NodeId(2),
                    &va40(VerticalAlign::Bottom),
                    InlineEdges::default(),
                );
                d.img(b, VerticalAlign::Baseline);
                b.close_inline();
                b.push_forced_break(NodeId(3));
            }),
        ),
        // BQ: <span lh40 va:top><img></span><br>
        (
            "BQ",
            Box::new(move |b, d| {
                b.open_inline(NodeId(2), &va40(VerticalAlign::Top), InlineEdges::default());
                d.img(b, VerticalAlign::Baseline);
                b.close_inline();
                b.push_forced_break(NodeId(3));
            }),
        ),
    ];
    for (id, case) in &cases {
        assert_eq!(q(WIDE, |b, d| case(b, d)), [2.0], "{id}");
    }
    // Not matrix cases: the same shapes with the vertical-align moved onto
    // the atomic or the forced break moved inside the aligned span. Under
    // the forced-break rule the atomic is content of the span, so 2 again.
    for (outer, inner) in [
        (VerticalAlign::Baseline, VerticalAlign::TextTop),
        (VerticalAlign::Baseline, VerticalAlign::Sub),
        (VerticalAlign::TextTop, VerticalAlign::Baseline),
        (VerticalAlign::Top, VerticalAlign::Baseline),
        (VerticalAlign::Bottom, VerticalAlign::Baseline),
    ] {
        let s = va40(outer);
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
}

/// Chromium 152: pending top/bottom descendants bypass baseline ancestors,
/// while empty text-top/bottom children still keep a br from adding a strut.
#[test]
fn pending_vertical_align_controls_forced_break_struts() {
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
        [40.0]
    );
    // BB: <span lh40><img va:top><br></span> — Chromium 40.
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Top);
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [40.0]
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
    // BO: <span lh40><span va:bottom><img></span><br></span> — Chromium 40.
    let bottom = InlineStyle {
        vertical_align: VerticalAlign::Bottom,
        ..span(20.0)
    };
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            b.open_inline(NodeId(4), &bottom, InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [40.0]
    );
    // CL: <span lh40><span lh10><span va:top><img></span></span><br></span>
    // — Chromium 40.
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            b.open_inline(NodeId(4), &span(10.0), InlineEdges::default());
            b.open_inline(NodeId(5), &top, InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
            b.close_inline();
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [40.0]
    );
    // CE: <span lh60><span lh40 va:top><img></span><br></span> — Chromium 60.
    let top40 = InlineStyle {
        vertical_align: VerticalAlign::Top,
        ..span(40.0)
    };
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(60.0), InlineEdges::default());
            b.open_inline(NodeId(4), &top40, InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [60.0]
    );
    // DI: <span lh40><span va:text-top></span><br></span> — Chromium 0
    // because the empty child is pending on its parent.
    let text_top = InlineStyle {
        vertical_align: VerticalAlign::TextTop,
        ..span(20.0)
    };
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            b.open_inline(NodeId(4), &text_top, InlineEdges::default());
            b.close_inline();
            b.push_forced_break(NodeId(3));
            b.close_inline();
        }),
        [0.0]
    );
    // CO: <span lh40 va:top></span><br> — Chromium 0.
    assert_eq!(
        q(WIDE, |b, _| {
            b.open_inline(NodeId(2), &top40, InlineEdges::default());
            b.close_inline();
            b.push_forced_break(NodeId(3));
        }),
        [0.0]
    );
}

#[test]
fn only_pending_alignments_credit_empty_children() {
    // Chromium 152, quirks: only alignments queued by ApplyBaselineShift
    // have metrics without content. Immediate baseline shifts stay empty.
    for (align, nested, root_height, top_parent) in [
        (VerticalAlign::Baseline, 40.0, 20.0, 40.0),
        (VerticalAlign::Sub, 40.0, 20.0, 40.0),
        (VerticalAlign::Super, 40.0, 20.0, 40.0),
        (VerticalAlign::Middle, 40.0, 20.0, 40.0),
        (VerticalAlign::Length(0.0), 40.0, 20.0, 40.0),
        (VerticalAlign::TextTop, 0.0, 0.0, 0.0),
        (VerticalAlign::TextBottom, 0.0, 0.0, 0.0),
        (VerticalAlign::Top, 40.0, 0.0, 0.0),
        (VerticalAlign::Bottom, 40.0, 0.0, 0.0),
    ] {
        let child = InlineStyle {
            vertical_align: align,
            ..span(20.0)
        };
        for (parent_align, height) in [
            (VerticalAlign::Baseline, nested),
            (VerticalAlign::Top, top_parent),
            (VerticalAlign::Bottom, top_parent),
        ] {
            assert_eq!(
                q(WIDE, |b, _| {
                    b.open_inline(
                        NodeId(2),
                        &InlineStyle {
                            vertical_align: parent_align,
                            ..span(40.0)
                        },
                        InlineEdges::default(),
                    );
                    b.open_inline(NodeId(4), &child, InlineEdges::default());
                    b.close_inline();
                    b.push_forced_break(NodeId(3));
                    b.close_inline();
                }),
                [height],
                "{parent_align:?}/{align:?}"
            );
        }
        assert_eq!(
            q(WIDE, |b, _| {
                b.open_inline(NodeId(4), &child, InlineEdges::default());
                b.close_inline();
                b.push_forced_break(NodeId(3));
            }),
            [root_height],
            "root/{align:?}"
        );
    }
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

/// `<ruby>base<rt>annotation</rt></ruby>`, the base and the annotation
/// `span(10)` (the matrix's 10px rt with line-height 10px); an image base
/// when `base` is `None`.
fn push_ruby(b: &mut ParagraphBuilder, d: &mut Doc, base: Option<&str>, annotation: &str) {
    let limits = Limits::default();
    let base = match base {
        Some(t) => RubyContent::text(
            TextSource::Generated { node: NodeId(11) },
            t,
            &span(10.0),
            &limits,
        ),
        None => {
            let mut inner = ParagraphBuilder::new(
                &shodo::style::ParagraphStyle {
                    root: span(10.0),
                    ..Default::default()
                },
                &limits,
            );
            d.img(&mut inner, VerticalAlign::Baseline);
            RubyContent::from_builder(inner)
        }
    };
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: base,
            align: Default::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(12),
                content: RubyContent::text(
                    TextSource::Generated { node: NodeId(13) },
                    annotation,
                    &span(10.0),
                    &limits,
                ),
                span: RubySpan::Auto,
                visibility: Default::default(),
            }],
            style: Default::default(),
        }],
    )
    .unwrap();
    b.push_ruby(NodeId(9), &span(10.0), ruby);
}

/// The height of the one line holding the ruby, inside a span
/// with line-height `lh`, or directly in the root when `lh` is `None`.
fn ruby_height(quirk: bool, lh: Option<f32>, base: Option<&str>, annotation: &str) -> f32 {
    let lines = heights(quirk, WIDE, |b, d| {
        if let Some(lh) = lh {
            b.open_inline(NodeId(2), &span(lh), InlineEdges::default());
        }
        push_ruby(b, d, base, annotation);
        if lh.is_some() {
            b.close_inline();
        }
    });
    assert_eq!(lines.len(), 1, "{lines:?}");
    lines[0]
}

/// A line holding ruby keeps the root strut (Chromium forces it, Blink
/// `EnsureTextMetrics`): DB/DC/DD/DE = 30/30/20/20. The ruby base and
/// annotation live in their own paragraphs, so neither the root nor a span
/// around the ruby directly contains text; without the ruby rule the quirk
/// would drop the root strut from all of these lines. Absolute heights
/// depend on how shodo stacks the annotation, so the rule is pinned by
/// relations that hold only when the root strut is present.
#[test]
fn ruby_lines_keep_the_root_strut() {
    // DA: <span lh10>x</span> = 10 (no ruby: the root strut is dropped).
    let da = heights(true, WIDE, |b, _| {
        b.open_inline(NodeId(2), &span(10.0), InlineEdges::default());
        text(b, "x");
        b.close_inline();
    });
    assert_eq!(da, [10.0]);
    // DB: <span lh10><ruby>x<rt>y</rt></ruby></span>. The lh10 span strut
    // nests inside the root's 20px one, so with the root strut the quirk
    // line is as high as the flag-off line (Chromium 30 both ways), and as
    // high as DC, where the ruby sits directly in the root.
    let db = ruby_height(true, Some(10.0), Some("x"), "y");
    assert_eq!(db, ruby_height(false, Some(10.0), Some("x"), "y"), "DB");
    assert!(db > da[0], "DB {db} vs DA {da:?}");
    // DC: <ruby>x<rt>y</rt></ruby>: unchanged by the quirk.
    let dc = ruby_height(true, None, Some("x"), "y");
    assert_eq!(dc, ruby_height(false, None, Some("x"), "y"), "DC");
    assert_eq!(db, dc, "DB vs DC");
    // DD: <span lh40><ruby><img><rt>y</rt></ruby></span>: the lh40 span has
    // no text and loses its strut (flag off it sizes the line), while the
    // root strut stays: the same height as the ruby in the root without
    // the quirk (Chromium 20).
    let dd = ruby_height(true, Some(40.0), None, "y");
    assert!(dd < ruby_height(false, Some(40.0), None, "y"), "DD {dd}");
    assert_eq!(dd, ruby_height(false, None, None, "y"), "DD");
    // DE: <span lh10><ruby>x<rt></rt></ruby></span>: an empty annotation
    // still keeps the root strut, so the line is the root's 20 (Chromium
    // 20), not the lh10 span's 10.
    let de = ruby_height(true, Some(10.0), Some("x"), "");
    assert_eq!(de, ruby_height(false, Some(10.0), Some("x"), ""), "DE");
    assert_eq!(de, 20.0, "DE");
}

#[test]
fn list_item_root_strut_does_not_restore_empty_child_struts() {
    for mode in [
        shodo::geometry::WritingMode::HorizontalTb,
        shodo::geometry::WritingMode::VerticalRl,
        shodo::geometry::WritingMode::VerticalLr,
    ] {
        for force in [false, true] {
            let mut s = root(true);
            s.writing_mode = mode;
            s.force_root_strut = force;
            let mut doc = Doc::new();
            let p = build(&s, |b| {
                b.open_inline(NodeId(2), &span(80.0), InlineEdges::default());
                doc.img(b, VerticalAlign::Baseline);
                b.close_inline();
            });
            let line = first_line(&p, &mut LayoutContext::new(), &doc);
            assert_eq!(
                line.block_size(),
                if force { 20.0 } else { 2.0 },
                "{mode:?}"
            );
        }
    }
}

#[test]
fn list_item_root_strut_contributes_on_every_continuation_line() {
    let mut s = root(true);
    s.force_root_strut = true;
    let p = build(&s, |b| {
        b.open_inline(NodeId(2), &span(10.0), InlineEdges::default());
        text(b, "a");
        b.push_forced_break(NodeId(3));
        text(b, "b");
        b.close_inline();
    });
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        WIDE,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        lines.iter().map(|l| l.block_size()).collect::<Vec<_>>(),
        [20.0, 20.0]
    );
}
