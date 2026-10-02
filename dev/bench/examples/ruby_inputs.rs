//! Fixed-font ruby build probe; input creation and output hashing are unmeasured.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{
    AtomicSizes, LayoutContext, ParagraphBuilder, Ruby, RubyAnnotation, RubyBase, RubyContent,
    RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn content(node: u64, text: &str, style: &InlineStyle, limits: &Limits) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 7,
        },
        text,
        style,
        limits,
    )
}
fn builder(first: bool, nested: bool, limits: &Limits) -> ParagraphBuilder {
    let root = InlineStyle {
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[1].family.into())],
        ..Default::default()
    };
    let reading = InlineStyle {
        font_size: 8.0,
        ..root.clone()
    };
    let style = ParagraphStyle {
        root: root.clone(),
        first_line: first.then(|| InlineStyle {
            font_size: 20.0,
            ..root.clone()
        }),
        ..Default::default()
    };
    let bases = (0..16)
        .map(|i| RubyBase {
            node: NodeId(100 + i),
            content: content(100 + i, "日", &root, limits),
            align: Default::default(),
        })
        .collect();
    let levels = (0..2)
        .map(|level| RubyLevel {
            style: RubyStyle::default(),
            annotations: (0..16)
                .map(|i| {
                    let node = 200 + level * 16 + i;
                    let text = if nested && level == 0 && i == 0 {
                        let mut inner = ParagraphBuilder::new(
                            &ParagraphStyle {
                                root: reading.clone(),
                                ..Default::default()
                            },
                            limits,
                        );
                        inner.push_ruby(
                            NodeId(500),
                            &reading,
                            Ruby::new(
                                vec![RubyBase {
                                    node: NodeId(501),
                                    content: content(501, "に", &reading, limits),
                                    align: Default::default(),
                                }],
                                vec![RubyLevel {
                                    annotations: vec![RubyAnnotation {
                                        node: NodeId(502),
                                        content: content(502, "ほん", &reading, limits),
                                        span: RubySpan::All,
                                        visibility: RubyVisibility::Visible,
                                    }],
                                    style: Default::default(),
                                }],
                            )
                            .unwrap(),
                        );
                        RubyContent::from_builder(inner)
                    } else {
                        content(node, "に", &reading, limits)
                    };
                    RubyAnnotation {
                        node: NodeId(node),
                        content: text,
                        span: RubySpan::Columns(i as usize..i as usize + 1),
                        visibility: RubyVisibility::Visible,
                    }
                })
                .collect(),
        })
        .collect();
    let mut b = ParagraphBuilder::new(&style, limits);
    b.push_ruby(NodeId(1), &root, Ruby::new(bases, levels).unwrap());
    b
}
fn main() {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    for (name, first, nested) in [
        ("normal", false, false),
        ("first-line", true, false),
        ("nested", false, true),
    ] {
        let rejected_limits = Limits {
            max_shaped_glyphs: Some(if first { 32 } else { 16 }),
            ..Default::default()
        };
        let rejected = builder(first, nested, &rejected_limits)
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap_err();
        assert_eq!(rejected.kind, shodo::limits::LimitKind::ShapedGlyphs);
        for sample in 0..11 {
            let input = builder(first, nested, &limits);
            let mut cx = LayoutContext::new();
            #[cfg(feature = "allocation-counting")]
            let scope = ALLOC.begin().unwrap();
            #[cfg(not(feature = "allocation-counting"))]
            let start = std::time::Instant::now();
            let p = std::hint::black_box(input.build(&mut cx, &fonts.collection).unwrap());
            #[cfg(feature = "allocation-counting")]
            let counts = scope.finish();
            #[cfg(not(feature = "allocation-counting"))]
            let ns = start.elapsed().as_nanos();
            let build_warnings = format!("{:?}", p.warnings());
            let lines = p.break_all(&mut cx, &Default::default(), 160.0, &AtomicSizes::EMPTY);
            let layout_warnings = format!("{:?}", cx.take_warnings());
            let output: Vec<_> = lines.iter().map(snapshot::line).collect();
            let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
            #[cfg(feature = "allocation-counting")]
            let measurement = serde_json::json!({"counts":counts});
            #[cfg(not(feature = "allocation-counting"))]
            let measurement = serde_json::json!({"ns":ns});
            println!(
                "{}",
                serde_json::json!({"case":name,"sample":sample,"digest":digest,"build_warnings":build_warnings,"layout_warnings":layout_warnings,"rejected":format!("{rejected:?}"),"measurement":measurement})
            );
        }
    }
}
