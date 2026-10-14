//! Exercise indexed paint against standalone geometry, then return the indexed
//! spans for the fixed-font, literal output assertions in the consumer tests.
use shodo::hit::LineLayout;
use shodo::{Line, PaintSpan};

pub fn paint_spans(line: &Line) -> Vec<PaintSpan<'_>> {
    let standalone = line.paint_spans();
    let indexed = {
        let layout = LineLayout::new(std::slice::from_ref(line));
        assert!(layout.paint_spans(1).is_none());
        layout.paint_spans(0).unwrap()
    };
    // The layout has already dropped: styles must borrow the accepted line.
    assert_eq!(indexed.len(), standalone.len());
    for (shared, direct) in indexed.iter().zip(&standalone) {
        assert_eq!(shared.node, direct.node);
        assert_eq!(shared.text_range, direct.text_range);
        assert!(std::ptr::eq(shared.style, direct.style));
        assert_eq!(shared.rect, direct.rect);
        assert_eq!(shared.font, direct.font);
        assert_eq!(shared.font_size, direct.font_size);
        assert_eq!(shared.metrics, direct.metrics);
        assert_eq!(shared.underline(), direct.underline());
        assert_eq!(shared.strikethrough(), direct.strikethrough());
    }
    for annotation in line.ruby_annotations() {
        paint_spans(annotation.line());
    }
    indexed
}
