mod common;
use common::*;
use shodo::geometry::{BaselineKind, Direction};
use shodo::limits::{LimitKind, Limits};
use shodo::mapping::{Affinity, MappingKind, TextOrigin};
use shodo::node::{InlineEdges, NodeId, OutOfFlowKind, Sides, TextSource};
use shodo::style::{
    BoxDecorationBreak, LineOptions, TabSize, TextAlign, TextJustify, VerticalAlign,
    WhiteSpaceCollapse,
};
use shodo::{
    AtomicSize, AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
};

fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}
fn dimension(value: u64) -> f32 {
    match value % 8 {
        0 => f32::NAN,
        1 => f32::INFINITY,
        2 => -5.0,
        _ => (value % 40) as f32,
    }
}
fn geometry(line: &shodo::Line, seed: u64) {
    assert!(
        line.inline_size().is_finite()
            && line.block_size().is_finite()
            && line.baseline(BaselineKind::Alphabetic).is_finite(),
        "seed {seed}: {line:?}"
    );
    let rect = |r: shodo::geometry::LogicalRect| {
        assert!(
            [r.inline_start, r.inline_size, r.block_start, r.block_size]
                .iter()
                .all(|v| v.is_finite()),
            "seed {seed}: {r:?}"
        )
    };
    for f in line.fragments() {
        match f {
            Fragment::GlyphRun(r) => {
                assert!(r.baseline().is_finite());
                for g in r.glyphs() {
                    assert!(
                        [g.inline_position, g.block_offset, g.advance]
                            .iter()
                            .all(|v| v.is_finite()),
                        "seed {seed}: {g:?}"
                    );
                }
            }
            Fragment::InlineBox(b) => {
                rect(b.rect);
                rect(b.content_rect);
            }
            Fragment::Atomic(a) => {
                rect(a.margin_rect);
                rect(a.border_rect);
                assert!(a.baseline.is_finite());
            }
            Fragment::OutOfFlowAnchor(a) => assert!(a.inline_position.is_finite()),
        }
    }
}

#[test]
fn generated_paragraphs_progress_and_preserve_shared_clusters() {
    for seed in 1..=256 {
        let mut state = seed;
        let mut root = style();
        if seed % 2 == 0 {
            root.direction = Direction::Rtl;
        }
        root.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
        root.root.tab_size = TabSize::Px(40.0);
        let mut b = ParagraphBuilder::new(&root, &Limits::default());
        let mut depth = 0;
        let mut floats = 0;
        let mut sizes = AtomicSizes::new();
        for n in 1..=32 {
            let v = random(&mut state);
            let node = NodeId(n);
            let source = TextSource::Dom { node, offset: 100 };
            match v % 10 {
                0 if depth < 3 => {
                    let mut child = root.root.clone();
                    child.font_size = 10.0 + (v % 3) as f32 * 5.0;
                    child.vertical_align = VerticalAlign::Length((v % 7) as f32 - 3.0);
                    if v.is_multiple_of(2) {
                        child.box_decoration_break = BoxDecorationBreak::Clone;
                    }
                    b.open_inline(
                        node,
                        &child,
                        InlineEdges {
                            padding: Sides {
                                inline_start: (v % 3) as f32,
                                inline_end: 2.0,
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    depth += 1;
                }
                1 if depth > 0 => {
                    b.close_inline();
                    depth -= 1;
                }
                2 => {
                    b.push_atomic(node, &root.root, InlineEdges::default());
                    sizes.insert(
                        node,
                        AtomicSize {
                            inline_size: dimension(v),
                            block_size: dimension(v / 8),
                            baseline: Some(dimension(v / 64)),
                            margins: Sides {
                                inline_start: -1.0,
                                block_start: -2.0,
                                ..Default::default()
                            },
                        },
                    );
                }
                3 => {
                    b.push_out_of_flow(node, OutOfFlowKind::Float);
                    floats += 1;
                }
                4 => {
                    b.push_forced_break(node);
                }
                5 => {
                    b.push_block_in_inline(node);
                }
                6 => {
                    b.push_text(source, "א ");
                }
                7 => {
                    b.push_text(source, "a\u{301}");
                }
                8 => {
                    b.push_text(source, "\ta");
                }
                _ => {
                    b.push_text(source, "a  ");
                }
            }
        }
        for _ in 0..depth {
            b.close_inline();
        }
        let p = b
            .build(
                &mut LayoutContext::new(),
                &shodo::font::FontCollection::new(&Limits::default()),
            )
            .unwrap();
        let width = dimension(seed * 17);
        let normal = p.break_all(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            width,
            &sizes,
        );
        let o = LineOptions {
            text_align: TextAlign::JustifyAll,
            text_justify: TextJustify::InterCharacter,
            ..Default::default()
        };
        let adjusted = p.break_all(&mut LayoutContext::new(), &o, width, &sizes);
        fn dump(lines: &[shodo::Line]) -> Vec<(u32, u32)> {
            lines
                .iter()
                .flat_map(glyphs)
                .map(|g| (g.id, g.cluster))
                .collect()
        }
        assert_eq!(
            dump(&normal),
            dump(&adjusted),
            "seed {seed}: {:?}",
            p.text()
        );
        for l in normal.iter().chain(&adjusted) {
            geometry(l, seed);
        }
        let mut token = p.start_token();
        let mut c = LineConstraint::new(width);
        let mut cx = LayoutContext::new();
        let mut retries = 0;
        let mut adopted = Vec::new();
        for step in 0..256 {
            match p.next_line(&mut cx, token, &LineOptions::default(), &c, &sizes) {
                LineResult::FloatEncountered { float_cursor, .. } => {
                    c.floats_placed_through = Some(float_cursor);
                    retries += 1;
                    assert!(retries <= 3 * floats + 1, "seed {seed}");
                }
                LineResult::Line(l) => {
                    assert_ne!(l.break_token(), token, "seed {seed}");
                    assert!(l.displaced_floats().is_empty());
                    token = l.break_token();
                    adopted.push(l);
                    retries = 0;
                }
                LineResult::BlockInInline { token_after, .. } => {
                    assert_ne!(token_after, token);
                    token = token_after;
                }
                LineResult::Done => break,
                other => panic!("seed {seed}: {other:?}"),
            }
            assert!(step < 255, "seed {seed}: failed to terminate");
        }
        assert_eq!(dump(&adopted), dump(&normal), "seed {seed}: {:?}", p.text());
        let mapping = p.offset_mapping().unwrap();
        for u in mapping.units() {
            if u.kind == MappingKind::Identity
                && u.dom.end - u.dom.start == u.text.end - u.text.start
            {
                for offset in u.dom.start..u.dom.end {
                    let (text, affinity) = mapping.dom_to_text(u.node, offset).unwrap();
                    assert_eq!(
                        mapping.text_to_dom(text, affinity),
                        Some(TextOrigin::Dom {
                            node: u.node,
                            offset
                        }),
                        "seed {seed}"
                    );
                }
            } else if u.kind == MappingKind::Collapsed {
                for offset in u.dom.start..u.dom.end {
                    assert_eq!(
                        mapping.dom_to_text(u.node, offset),
                        Some((u.text.end, Affinity::Downstream))
                    );
                }
            }
        }
    }
}

#[test]
fn generated_small_limits_fail_without_large_allocations() {
    for cap in 0..32 {
        let limits = Limits {
            max_text_bytes: Some(cap),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style(), &limits);
        b.push_text(
            TextSource::Generated { node: NodeId(1) },
            &"a".repeat(cap as usize + 1),
        );
        assert_eq!(
            b.build(
                &mut LayoutContext::new(),
                &shodo::font::FontCollection::new(&limits)
            )
            .unwrap_err()
            .kind,
            LimitKind::TextBytes
        );
    }
}
