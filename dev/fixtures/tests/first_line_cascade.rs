//! Real normal/alternate raikiri cascades, preserving identical DOM node IDs.
//! The pinned raikiri version does not resolve ::first-line selectors. The
//! alternate root rule below probes the explicit consumer boundary, not CSS
//! pseudo-element conformance; a production caller must supply that cascade.
use raikiri_html::{ParseOptions, UncascadedDocument, parse_html};
use raikiri_style::{CascadeResult, ComputedValues, Origin};
use raikiri_traits::{Dom, NodeId as DomId};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle, TextTransform};
use shodo::{AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
};

const HTML: &str = "<style>#root{font-family:'Shodo Fixture Latin';font-size:16px;color:blue}#equal{font-size:16px;color:blue}#different,#outer{font-size:20px}#nested,#relative{font-size:150%}</style><div id=root><span id=inherited>a</span><span id=equal>b</span><span id=different>c</span><span id=outer><span id=nested>d</span></span><span id=relative>e</span></div>";

struct Cascades {
    parsed: UncascadedDocument,
    normal: CascadeResult,
    first: CascadeResult,
}
fn document(extra: &[&str]) -> Cascades {
    let doc = parse_html(
        HTML.as_bytes(),
        &ParseOptions {
            extra_stylesheets: &[],
            network: None,
            base_url: None,
        },
    )
    .unwrap();
    let (parsed, normal) = doc.into_parts();
    let mut tree = raikiri_html::build_rule_tree(&parsed);
    for source in extra {
        tree.add_stylesheet(source, Origin::Author);
    }
    let first = raikiri_style::cascade(&parsed.dom, &tree).unwrap();
    Cascades {
        parsed,
        normal,
        first,
    }
}
fn style(cv: &ComputedValues) -> InlineStyle {
    assert_eq!(cv.font_family[0].as_str(), FONTS[0].family);
    InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[0].family.into())],
        font_size: cv.font_size.0,
        font_weight: cv.font_weight,
        text_transform: match cv.text_transform {
            raikiri_style::property::TextTransform::None => TextTransform::None,
            raikiri_style::property::TextTransform::Uppercase => TextTransform::Uppercase,
            _ => panic!("test converter only accepts none/uppercase"),
        },
        ..Default::default()
    }
}
fn walk(normal: &Cascades, id: usize, b: &mut ParagraphBuilder) {
    let n = normal.parsed.dom.get_node(id).unwrap();
    if let Some(text) = n.text_content() {
        b.push_text(
            TextSource::Dom {
                node: NodeId(id as u64),
                offset: 0,
            },
            text,
        );
        return;
    }
    b.open_inline_with_first_line(
        NodeId(id as u64),
        &style(&normal.normal.computed[id]),
        &style(&normal.first.computed[id]),
        Default::default(),
    );
    for child in normal.parsed.dom.child_ids(DomId::new(id as u64)) {
        walk(normal, child.0 as usize, b);
    }
    b.close_inline();
}
struct Pen(tiny_skia::PathBuilder);
impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x: f32, y: f32, z: f32, w: f32) {
        self.0.quad_to(x, y, z, w);
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.0.cubic_to(a, b, c, d, e, f);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

#[test]
fn five_resolved_child_cases_reach_actual_font_metrics_mapping_and_glyph_paint() {
    let normal = document(&["#root{font-size:32px;color:red;text-transform:uppercase}"]);
    assert_eq!(normal.normal.computed.len(), normal.first.computed.len());
    let root = (0..normal.parsed.dom.node_count())
        .find(|&id| normal.parsed.dom.get_node(id).unwrap().attribute("id") == Some("root"))
        .unwrap();
    let expected = [
        ("inherited", 16.0, 32.0),
        ("equal", 16.0, 16.0),
        ("different", 20.0, 20.0),
        ("nested", 30.0, 30.0),
        ("relative", 24.0, 48.0),
    ];
    for (name, n, a) in expected {
        let id = (0..normal.parsed.dom.node_count())
            .find(|&id| normal.parsed.dom.get_node(id).unwrap().attribute("id") == Some(name))
            .unwrap();
        assert_eq!(normal.normal.computed[id].font_size.0, n, "normal {name}");
        assert_eq!(normal.first.computed[id].font_size.0, a, "alternate {name}");
    }
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(&normal.normal.computed[root]),
            first_line: Some(style(&normal.first.computed[root])),
            ..Default::default()
        },
        &Limits::default(),
    );
    for child in normal.parsed.dom.child_ids(DomId::new(root as u64)) {
        walk(&normal, child.0 as usize, &mut builder);
    }
    let fonts = load_fonts(&Limits::default()).unwrap();
    let p = builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        500.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text(), "ABCDE");
    let sizes: Vec<_> = lines[0]
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r.font_size()),
            _ => None,
        })
        .collect();
    assert_eq!(sizes, vec![32.0, 16.0, 20.0, 30.0, 48.0]);
    assert!(lines[0].block_size() >= 48.0);
    let mut image = tiny_skia::Pixmap::new(220, 80).unwrap();
    image.fill(tiny_skia::Color::WHITE);
    let mut glyph_count = 0;
    for fragment in lines[0].fragments() {
        let Fragment::GlyphRun(run) = fragment else {
            continue;
        };
        assert!(!run.embolden() && run.skew().is_none());
        let data = run.font_data().unwrap();
        assert_eq!(data.data.as_ref(), FONTS[0].bytes);
        let font = FontRef::from_index(data.data.as_ref(), data.index).unwrap();
        let owner = run.node().unwrap().0 as usize;
        let cv = &normal.first.computed[owner];
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(cv.color.r, cv.color.g, cv.color.b, cv.color.a);
        for glyph in run.glyphs() {
            let outline = font.outline_glyphs().get(GlyphId::new(glyph.id)).unwrap();
            let mut pen = Pen(tiny_skia::PathBuilder::new());
            outline
                .draw(
                    DrawSettings::unhinted(
                        Size::new(run.font_size()),
                        LocationRef::new(run.normalized_coords()),
                    ),
                    &mut pen,
                )
                .unwrap();
            let path = pen.0.finish().unwrap();
            let x = 4.0 + glyph.inline_position;
            let y = 4.0 + lines[0].block_offset() + run.baseline() + glyph.block_offset;
            image.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                tiny_skia::Transform::from_row(1.0, 0.0, 0.0, -1.0, x, y),
                None,
            );
            let origin = lines[0]
                .offset_mapping()
                .unwrap()
                .text_to_dom(glyph.cluster, shodo::mapping::Affinity::Downstream)
                .unwrap();
            assert!(
                matches!(origin,shodo::mapping::TextOrigin::Dom{node,offset:0} if node==NodeId(owner as u64))
            );
            glyph_count += 1;
        }
    }
    assert_eq!(glyph_count, 5);
    assert!(
        image
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > p[1] && p[0] > p[2])
            .count()
            > 30
    );
    assert!(
        image
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[2] > p[0] && p[2] > p[1])
            .count()
            > 10
    );
    assert_eq!(&image.encode_png().unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    if let Some(path) = std::env::var_os("SHODO_FIRST_LINE_PNG") {
        image.save_png(path).unwrap();
    }
}

#[test]
fn pinned_raikiri_requires_a_real_first_line_cascade_provider() {
    let doc = document(&["#root::first-line{font-size:32px}"]);
    let root = (0..doc.parsed.dom.node_count())
        .find(|&id| doc.parsed.dom.get_node(id).unwrap().attribute("id") == Some("root"))
        .unwrap();
    assert_eq!(doc.first.computed[root].font_size.0, 16.0);
    assert!(
        doc.first.pseudo.is_empty(),
        "revisit this limitation if upstream exposes first-line"
    );
}
