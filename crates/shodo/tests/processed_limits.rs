use shodo::font::FontCollection;
use shodo::limits::{LimitKind, Limits};
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{ParagraphStyle, UnicodeBidi, WhiteSpaceCollapse};
use shodo::{LayoutContext, ParagraphBuilder};

#[test]
fn generated_bytes_are_checked_before_append() {
    let root = ParagraphStyle::default();
    let limits = Limits {
        max_text_bytes: Some(2),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&root, &limits);
    b.push_atomic(NodeId(1), &root.root, InlineEdges::default());
    assert_eq!(
        b.build(
            &mut LayoutContext::new(),
            &FontCollection::with_options(
                &limits,
                shodo::font::FontOptions {
                    system_fonts: false,
                    ..Default::default()
                }
            )
        )
        .unwrap_err()
        .kind,
        LimitKind::TextBytes
    );
    let mut inline = root.root.clone();
    inline.unicode_bidi = UnicodeBidi::Isolate;
    let mut b = ParagraphBuilder::new(&root, &limits);
    b.open_inline(NodeId(1), &inline, InlineEdges::default())
        .push_text(TextSource::Generated { node: NodeId(2) }, "a")
        .close_inline();
    assert_eq!(
        b.build(
            &mut LayoutContext::new(),
            &FontCollection::with_options(
                &limits,
                shodo::font::FontOptions {
                    system_fonts: false,
                    ..Default::default()
                }
            )
        )
        .unwrap_err()
        .kind,
        LimitKind::TextBytes
    );
}

#[test]
fn split_control_items_obey_processed_item_budget() {
    let mut root = ParagraphStyle::default();
    root.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let limits = Limits {
        max_items: Some(2),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&root, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a\tb");
    assert_eq!(
        b.build(
            &mut LayoutContext::new(),
            &FontCollection::with_options(
                &limits,
                shodo::font::FontOptions {
                    system_fonts: false,
                    ..Default::default()
                }
            )
        )
        .unwrap_err()
        .kind,
        LimitKind::Items
    );
}
