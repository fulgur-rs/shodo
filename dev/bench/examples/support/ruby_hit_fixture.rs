// Shared public-API reproduction: real Latin glyphs and alternating visible readings.
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::ruby::*;
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, Line, ParagraphBuilder};

pub fn fixture(depth: usize, limits: &Limits) -> Result<(Vec<Line>, String), String> {
    fn leaf(node: u64, style: &InlineStyle, limits: &Limits) -> RubyContent {
        RubyContent::text(
            TextSource::Dom {
                node: NodeId(node),
                offset: 0,
            },
            if node.is_multiple_of(2) { "b" } else { "a" },
            style,
            limits,
        )
    }
    fn ruby(depth: usize, style: &InlineStyle, limits: &Limits) -> Ruby {
        let reading = if depth == 1 {
            leaf(1000, style, limits)
        } else {
            let pstyle = ParagraphStyle {
                root: style.clone(),
                ..Default::default()
            };
            let mut b = ParagraphBuilder::new(&pstyle, limits);
            b.with_offset_mapping(true);
            b.push_ruby(
                NodeId(300 + depth as u64 - 1),
                style,
                ruby(depth - 1, style, limits),
            );
            RubyContent::from_builder(b)
        };
        Ruby::new(
            vec![RubyBase {
                node: NodeId(10 + depth as u64),
                content: leaf(100 + depth as u64, style, limits),
                align: RubyAlign::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(200 + depth as u64),
                    content: reading,
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            }],
        )
        .unwrap()
    }
    assert!(depth > 0);
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            include_bytes!("../../../fixtures/assets/fonts/latin.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = InlineStyle {
        font_size: 20.0,
        font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
        ..Default::default()
    };
    let pstyle = ParagraphStyle {
        root: style.clone(),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&pstyle, limits);
    b.with_offset_mapping(true);
    b.push_ruby(NodeId(9999), &style, ruby(depth, &style, limits));
    let mut cx = LayoutContext::new();
    let paragraph = b.build(&mut cx, &fonts).map_err(|e| format!("{e:?}"))?;
    let build_warnings = format!("{:?}", paragraph.warnings());
    let lines = paragraph.break_all(&mut cx, &Default::default(), 1000.0, &AtomicSizes::EMPTY);
    Ok((lines, format!("{build_warnings}/{:?}", cx.take_warnings())))
}

pub fn actual_depth(lines: &[Line]) -> usize {
    lines
        .iter()
        .flat_map(|l| l.ruby_annotations())
        .filter(|a| a.visibility() == RubyVisibility::Visible)
        .map(|a| 1 + actual_depth(std::slice::from_ref(a.line())))
        .max()
        .unwrap_or(0)
}

pub fn assert_real_glyphs(lines: &[Line]) {
    for l in lines {
        for f in l.fragments() {
            if let shodo::Fragment::GlyphRun(r) = f {
                assert!(r.glyphs().all(|g| g.id != 0), "missing fixture glyph");
            }
        }
        for a in l.ruby_annotations() {
            assert_real_glyphs(std::slice::from_ref(a.line()));
        }
    }
}
