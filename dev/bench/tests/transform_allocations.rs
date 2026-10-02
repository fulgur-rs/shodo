#![cfg(feature = "allocation-counting")]
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle, TextTransform};
use shodo::{LayoutContext, ParagraphBuilder};
use shodo_bench::allocator::CountingAllocator;
#[global_allocator]
static ALLOC: CountingAllocator<std::alloc::System> = CountingAllocator::new(std::alloc::System);

#[test]
fn width_and_kana_builds_do_not_allocate_a_string_per_scalar_per_stage() {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let text = "a".repeat(1024);
    let ps = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
            text_transform: TextTransform::FullWidthFullSizeKana,
            ..Default::default()
        },
        ..Default::default()
    };
    let prepare = || {
        let mut b = ParagraphBuilder::new(&ps, &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, &text);
        b
    };
    let mut cx = LayoutContext::new();
    drop(prepare().build(&mut cx, &fonts.collection).unwrap());
    let b = prepare();
    let scope = ALLOC.begin().unwrap();
    let paragraph = b.build(&mut cx, &fonts.collection).unwrap();
    let counts = scope.finish();
    assert_eq!(paragraph.text(), "ａ".repeat(1024));
    // All other build work is included. Three transient strings per scalar
    // already exceed this ceiling, independently of font/cache implementation.
    assert!(
        counts.calls < 2048,
        "width/kana build allocated {} blocks for 1024 scalars",
        counts.calls
    );
}
