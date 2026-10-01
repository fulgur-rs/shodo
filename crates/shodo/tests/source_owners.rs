mod common;

use common::{build, first_line, style};
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, OutOfFlowKind, TextSource};
use shodo::style::{FontFamily, LineOptions, TextTransform, WordBreak};
use shodo::{
    AtomicSizes, Fragment, LayoutContext, Line, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder,
};

fn dom(node: u64, offset: u32) -> TextSource {
    TextSource::Dom {
        node: NodeId(node),
        offset,
    }
}

fn lines(p: &Paragraph, width: f32) -> Vec<Line> {
    let mut cx = LayoutContext::new();
    let mut token = p.start_token();
    let mut out = Vec::new();
    while let LineResult::Line(line) = p.next_line(
        &mut cx,
        token,
        &LineOptions::default(),
        &LineConstraint::new(width),
        &AtomicSizes::EMPTY,
    ) {
        token = line.break_token();
        assert!(out.len() < 20, "line layout must progress");
        out.push(line);
    }
    out
}

fn sources(line: &Line) -> Vec<Option<TextSource>> {
    line.fragments()
        .filter_map(|fragment| match fragment {
            Fragment::GlyphRun(run) => {
                assert!(
                    run.glyphs().len() > 0,
                    "public glyph fragments must be nonempty"
                );
                Some(run.source())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn wrapped_sources_and_owners_account_for_collapsed_dom_bytes() {
    let p = build(&style(), |b| {
        b.push_text(dom(7, 20), "  ab   cd ");
    });
    let result = lines(&p, 25.0);
    assert_eq!(result.len(), 2);
    assert_eq!(sources(&result[0]), [Some(dom(7, 22))]);
    assert_eq!(sources(&result[1]), [Some(dom(7, 27))]);
    // Leading collapsed bytes belong to the first line; a collapsed gap at
    // the wrap belongs to the preceding line, so no DOM bytes are duplicated.
    assert_eq!(
        result[0].owners().collect::<Vec<_>>(),
        [(NodeId(7), 20..27)]
    );
    assert_eq!(
        result[1].owners().collect::<Vec<_>>(),
        [(NodeId(7), 27..30)]
    );
}

#[test]
fn expansion_owners_cover_the_original_indivisible_dom_character() {
    let mut input = style();
    input.root.text_transform = TextTransform::Uppercase;
    input.root.word_break = WordBreak::BreakAll;
    let p = build(&input, |b| {
        b.push_text(dom(1, 100), "ßa");
    });
    let result = lines(&p, 15.0);
    assert_eq!(result.len(), 2);
    assert_eq!(&result[0].text()[result[0].text_range()], "SS");
    assert_eq!(sources(&result[0]), [Some(dom(1, 100))]);
    assert_eq!(sources(&result[1]), [Some(dom(1, 102))]);
    assert_eq!(
        result[0].owners().collect::<Vec<_>>(),
        [(NodeId(1), 100..102)]
    );
    assert_eq!(
        result[1].owners().collect::<Vec<_>>(),
        [(NodeId(1), 102..103)]
    );
}

#[test]
fn owners_include_every_node_in_a_shared_ligature_with_one_paint_owner() {
    let limits = Limits::default();
    let mut input = style();
    input.root.font_families = vec![FontFamily::Named("Owner Fixture".into())];
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Owner Fixture".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut builder = ParagraphBuilder::new(&input, &limits);
    builder
        .push_text(dom(1, 5), "f")
        .push_text(dom(2, 10), "f")
        .push_text(dom(3, 15), "i");
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    let runs: Vec<_> = line
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(run) => Some(run),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].glyphs().len(), 1);
    assert_eq!(sources(&line), [Some(dom(1, 5))]);
    assert_eq!(
        line.owners().collect::<Vec<_>>(),
        [(NodeId(1), 5..6), (NodeId(2), 10..11), (NodeId(3), 15..16)]
    );
}

#[test]
fn generated_text_and_object_anchors_do_not_invent_dom_owner_ranges() {
    let input = style();
    let p = build(&input, |b| {
        b.push_text(TextSource::Generated { node: NodeId(9) }, "X")
            .push_text(dom(7, 50), "ab")
            .push_atomic(NodeId(10), &input.root, InlineEdges::default())
            .push_out_of_flow(NodeId(11), OutOfFlowKind::Absolute)
            .push_text(dom(8, 70), "cd");
    });
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(
        sources(&line),
        [
            Some(TextSource::Generated { node: NodeId(9) }),
            Some(dom(7, 50)),
            Some(dom(8, 70))
        ]
    );
    assert_eq!(
        line.owners().collect::<Vec<_>>(),
        [(NodeId(7), 50..52), (NodeId(8), 70..72)]
    );
}

#[test]
fn owners_use_each_lines_selected_first_line_mapping() {
    let mut input = style();
    input.root.word_break = WordBreak::BreakAll;
    let mut first = input.root.clone();
    first.text_transform = TextTransform::FullWidth;
    input.first_line = Some(first);
    let p = build(&input, |b| {
        b.push_text(dom(1, 50), "ab");
    });
    let result = lines(&p, 15.0);
    assert_eq!(result.len(), 2);
    assert_eq!(p.text(), "ab");
    assert_eq!(result[0].text(), "ａｂ");
    assert_eq!(result[1].text(), "ab");
    assert_eq!(sources(&result[0]), [Some(dom(1, 50))]);
    assert_eq!(sources(&result[1]), [Some(dom(1, 51))]);
    assert_eq!(
        result[0].owners().collect::<Vec<_>>(),
        [(NodeId(1), 50..51)]
    );
    assert_eq!(
        result[1].owners().collect::<Vec<_>>(),
        [(NodeId(1), 51..52)]
    );
}

#[test]
fn owners_keep_disjoint_ranges_when_a_caller_reuses_a_node() {
    let p = build(&style(), |b| {
        b.push_text(dom(1, 0), "ab").push_text(dom(1, 10), "cd");
    });
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(sources(&line), [Some(dom(1, 0)), Some(dom(1, 10))]);
    assert_eq!(
        line.owners().collect::<Vec<_>>(),
        [(NodeId(1), 0..2), (NodeId(1), 10..12)]
    );
}

#[test]
fn owners_include_a_node_whose_voicing_mark_was_consumed_by_composition() {
    let mut input = style();
    input.root.text_transform = TextTransform::FullWidth;
    let p = build(&input, |b| {
        b.push_text(dom(1, 5), "ｶ").push_text(dom(2, 10), "ﾞ");
    });
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.text(), "ガ");
    assert_eq!(sources(&line), [Some(dom(1, 5))]);
    assert_eq!(
        line.owners().collect::<Vec<_>>(),
        [(NodeId(1), 5..8), (NodeId(2), 10..13)]
    );
}

#[test]
fn disabling_mapping_explicitly_disables_dom_source_queries() {
    let p = build(&style(), |b| {
        b.with_offset_mapping(false).push_text(dom(1, 50), "ab");
    });
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert!(line.offset_mapping().is_none());
    assert_eq!(sources(&line), [None]);
    assert!(line.owners().next().is_none());
    assert!(
        line.fragments()
            .any(|f| matches!(f, Fragment::GlyphRun(run) if run.node()==Some(NodeId(1))))
    );
}
