mod common;

use common::{build, first_line, style};
use shodo::limits::WarningKind;
use shodo::mapping::MappingKind;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{InlineStyle, WordSpaceTransform};

fn dom(node: u64, offset: u32) -> TextSource {
    TextSource::Dom {
        node: NodeId(node),
        offset,
    }
}

#[test]
fn ideographic_space_transforms_zero_width_spaces_and_wbr_markers() {
    let mut paragraph_style = style();
    paragraph_style.root.word_space_transform = WordSpaceTransform::IdeographicSpace;
    let paragraph = build(&paragraph_style, |builder| {
        builder
            .push_text(dom(1, 0), "a\u{200b}")
            .open_inline(NodeId(2), &paragraph_style.root, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(2) }, "\u{200b}")
            .close_inline()
            .push_text(dom(3, 0), "b");
    });

    assert_eq!(paragraph.text(), "a\u{3000}\u{3000}b");
}

#[test]
fn space_transforms_zero_width_spaces_and_wbr_markers() {
    let mut paragraph_style = style();
    paragraph_style.root.word_space_transform = WordSpaceTransform::Space;
    let paragraph = build(&paragraph_style, |builder| {
        builder
            .push_text(dom(1, 0), "a\u{200b}")
            .open_inline(NodeId(2), &paragraph_style.root, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(2) }, "\u{200b}")
            .close_inline()
            .push_text(dom(3, 0), "b");
    });

    assert_eq!(paragraph.text(), "a  b");
}

#[test]
fn inherited_transform_applies_to_inline_content_but_none_overrides_it() {
    let mut paragraph_style = style();
    paragraph_style.root.word_space_transform = WordSpaceTransform::Space;
    let inherited = paragraph_style.root.clone();
    let mut disabled = inherited.clone();
    disabled.word_space_transform = WordSpaceTransform::None;
    let paragraph = build(&paragraph_style, |builder| {
        builder
            .open_inline(NodeId(1), &inherited, InlineEdges::default())
            .push_text(dom(2, 0), "x\u{200b}")
            .close_inline()
            .open_inline(NodeId(3), &disabled, InlineEdges::default())
            .push_text(dom(4, 0), "y\u{200b}")
            .close_inline();
    });

    assert_eq!(paragraph.text(), "x y\u{200b}");
}

#[test]
fn inline_transform_applies_when_root_is_none() {
    let paragraph_style = style();
    let mut inline = InlineStyle::default();
    inline.word_space_transform = WordSpaceTransform::Space;
    let paragraph = build(&paragraph_style, |builder| {
        builder
            .open_inline(NodeId(1), &inline, InlineEdges::default())
            .push_text(dom(2, 0), "a\u{200b}b")
            .close_inline();
    });

    assert_eq!(paragraph.text(), "a b");
}

#[test]
fn first_line_root_transform_is_inherited_by_inline_styles() {
    let mut paragraph_style = style();
    let mut first = paragraph_style.root.clone();
    first.word_space_transform = WordSpaceTransform::Space;
    paragraph_style.first_line = Some(first);
    let inherited = paragraph_style.root.clone();
    let paragraph = build(&paragraph_style, |builder| {
        builder
            .open_inline(NodeId(1), &inherited, InlineEdges::default())
            .push_text(dom(2, 0), "a\u{200b}b")
            .close_inline();
    });

    assert_eq!(paragraph.text(), "a\u{200b}b");
    assert_eq!(
        first_line(&paragraph, 100.0, &Default::default(), &Default::default()).text(),
        "a b"
    );
}

#[test]
fn auto_phrase_transforms_explicit_markers_and_warns_when_phrase_analysis_is_unavailable() {
    for (transform, expected) in [
        (WordSpaceTransform::SpaceAutoPhrase, "a b"),
        (WordSpaceTransform::IdeographicSpaceAutoPhrase, "a\u{3000}b"),
    ] {
        let mut paragraph_style = style();
        paragraph_style.root.word_space_transform = transform;
        let paragraph = build(&paragraph_style, |builder| {
            builder.push_text(dom(1, 0), "a\u{200b}b");
        });
        assert_eq!(paragraph.text(), expected);
        assert!(paragraph.warnings().iter().any(|warning| {
            warning.kind == WarningKind::Unsupported
                && warning.message.contains("automatic phrase segmentation")
        }));

        let no_marker = build(&paragraph_style, |builder| {
            builder.push_text(dom(1, 0), "日本語");
        });
        assert_eq!(no_marker.text(), "日本語");
    }
}

#[test]
fn mapping_tracks_the_original_zero_width_space_range_after_replacement() {
    let mut paragraph_style = style();
    paragraph_style.root.word_space_transform = WordSpaceTransform::Space;
    let paragraph = build(&paragraph_style, |builder| {
        builder
            .with_offset_mapping(true)
            .push_text(dom(7, 10), "a\u{200b}b");
    });

    assert_eq!(paragraph.text(), "a b");
    let units = paragraph.offset_mapping().unwrap().units();
    assert!(units.iter().any(|unit| {
        unit.kind == MappingKind::Expanded
            && unit.node == NodeId(7)
            && unit.dom == (11..14)
            && unit.text == (1..2)
    }));
}
