use shodo::geometry::{Direction, WritingMode};
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{
    InlineStyle, LineHeight, LineOptions, ParagraphStyle, TabSize, TextAlign, TextOrientation,
    UnicodeBidi, WhiteSpaceCollapse,
};

#[test]
fn defaults_match_css_initial_values() {
    let s = InlineStyle::default();
    assert_eq!(s.font_size, 16.0);
    assert_eq!(s.line_height, LineHeight::Normal);
    assert_eq!(s.white_space_collapse, WhiteSpaceCollapse::Collapse);
    assert_eq!(s.direction, Direction::Ltr);
    assert_eq!(s.unicode_bidi, UnicodeBidi::Normal);
    assert_eq!(s.text_orientation, TextOrientation::Mixed);
    assert_eq!(s.tab_size, TabSize::Spaces(8.0));

    let p = ParagraphStyle::default();
    assert_eq!(p.writing_mode, WritingMode::HorizontalTb);
    assert!(p.first_line.is_none());

    assert_eq!(LineOptions::default().text_align, TextAlign::Start);
}

#[test]
fn edges_sum_margin_border_padding_per_side() {
    let side = |v: f32| Sides {
        inline_start: v,
        inline_end: v * 2.0,
        block_start: 0.0,
        block_end: 0.0,
    };
    let e = InlineEdges {
        margin: side(1.0),
        border: side(2.0),
        padding: side(3.0),
    };
    assert_eq!(e.inline_start_total(), 6.0);
    assert_eq!(e.inline_end_total(), 12.0);
    assert_eq!(side(1.0).inline_sum(), 3.0);
}

#[test]
fn text_source_carries_node() {
    let s = TextSource::Dom {
        node: NodeId(7),
        offset: 3,
    };
    assert_eq!(s.node(), NodeId(7));
    assert_eq!(TextSource::Generated { node: NodeId(8) }.node(), NodeId(8));
}
