mod common;

use common::first_line;
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::geometry::BaselineKind;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, ParagraphBuilder};

/// The block size and alphabetic baseline of the first line of "ab" in 20px
/// text set in the Latin fixture, whose hhea extents are 1069/293 units:
/// 21.38px and 5.86px.
fn line_of_text(whole_pixel_metrics: bool) -> (f32, f32) {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            whole_pixel_metrics,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Fixture".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_size = 20.0;
    style.root.font_families = vec![FontFamily::Named("Fixture".into())];
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "ab",
    );
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let line = first_line(
        &paragraph,
        1000.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    (line.block_size(), line.baseline(BaselineKind::Alphabetic))
}

#[test]
fn a_line_keeps_the_exact_font_metrics_by_default() {
    let (size, baseline) = line_of_text(false);
    assert!((baseline - 21.38).abs() < 0.02, "{baseline}");
    assert!((size - 27.24).abs() < 0.02, "{size}");
}

#[test]
fn whole_pixel_metrics_put_the_baseline_on_a_whole_pixel() {
    // 21.38px rounds to 21px and 5.86px to 6px, like Blink's primary metrics.
    assert_eq!(line_of_text(true), (27.0, 21.0));
}
