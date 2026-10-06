mod common;

use common::{first_line, glyphs};
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, FontFeature, InlineStyle, ParagraphStyle, TextSpacingTrim};
use shodo::{AtomicSizes, LayoutContext, ParagraphBuilder};

#[test]
#[ignore = "requires the original WPT fonts and SHODO_181_WPT_ROOT"]
fn original_wpt_fonts_preserve_normal_and_match_explicit_trim_all() {
    let root = std::path::PathBuf::from(std::env::var_os("SHODO_181_WPT_ROOT").unwrap());
    for variant in ["halt", "chws"] {
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        for font_variant in ["halt", "chws"] {
            fonts
                .register_face(
                    std::fs::read(root.join(format!(
                        "fonts/noto/cjk/NotoSansCJKjp-Regular-subset-{font_variant}.otf"
                    )))
                    .unwrap(),
                    0,
                    FontFaceDescriptor {
                        family: format!("WPT CJK {font_variant}"),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        for punctuation in ['（', '）', '、', '・', '。', '「', '」'] {
            let mut results = Vec::new();
            for (label, trim, explicit) in [
                ("test", TextSpacingTrim::Normal, false),
                ("reference", TextSpacingTrim::SpaceAll, true),
                ("diagnostic-trim-all", TextSpacingTrim::TrimAll, false),
            ] {
                // The original reference always uses the halt font, including
                // the chws variant of the test document.
                let font_variant = if explicit { "halt" } else { variant };
                let root = InlineStyle {
                    font_families: vec![FontFamily::Named(format!("WPT CJK {font_variant}"))],
                    font_size: 20.0,
                    text_spacing_trim: trim,
                    ..Default::default()
                };
                let mut builder = ParagraphBuilder::new(
                    &ParagraphStyle {
                        root: root.clone(),
                        ..Default::default()
                    },
                    &limits,
                );
                builder.push_text(TextSource::Generated { node: NodeId(1) }, "国");
                let mut inner = root;
                if explicit {
                    inner.font_features = vec![FontFeature {
                        tag: *b"halt",
                        value: 1,
                    }];
                }
                builder.open_inline(NodeId(2), &inner, InlineEdges::default());
                builder.push_text(
                    TextSource::Generated { node: NodeId(3) },
                    &punctuation.to_string(),
                );
                builder.close_inline();
                builder.push_text(TextSource::Generated { node: NodeId(4) }, "国");
                let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
                let line = first_line(&paragraph, 800.0, &Default::default(), &AtomicSizes::EMPTY);
                println!(
                    "{variant} {punctuation} {label}: width={} glyphs={:?}",
                    line.inline_size(),
                    glyphs(&line)
                );
                results.push((line.inline_size(), glyphs(&line)));
            }
            assert_eq!(results[0].0, 60.0, "{variant} {punctuation}");
            assert_eq!(results[1].0, 50.0, "{variant} {punctuation}");
            assert_eq!(results[2], results[1], "{variant} {punctuation}");
        }
    }
}
