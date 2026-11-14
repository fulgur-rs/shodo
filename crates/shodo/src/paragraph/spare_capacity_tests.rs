//! A built paragraph keeps no room beyond the lengths of its vectors.

use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::style::{FontFamily, ParagraphStyle};
use crate::{LayoutContext, ParagraphBuilder};

#[test]
fn a_short_paragraph_keeps_no_spare_room() {
    let limits = Limits::default();
    let fonts = crate::test_support::fonts::collection(&limits);
    fonts
        .register(crate::test_support::fonts::LATIN.to_vec())
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Noto Sans".into())];
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "Item 1",
    );
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let data = &paragraph.data;

    // One style, one item and one run each sat in room grown for four.
    assert_eq!(data.styles.len(), 1);
    assert_eq!(data.styles.capacity(), data.styles.len());
    assert_eq!(data.items.capacity(), data.items.len());
    assert_eq!(data.shape_items.capacity(), data.shape_items.len());
    assert_eq!(data.runs.capacity(), data.runs.len());
    assert_eq!(data.units.capacity(), data.units.len());
    assert_eq!(data.text.capacity(), data.text.len());
    assert_eq!(data.glyphs.id.capacity(), data.glyphs.id.len());
    assert_eq!(
        data.breaks.opportunities.capacity(),
        data.breaks.opportunities.len()
    );
}
