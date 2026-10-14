//! Overflow bounds regression and outline work contracts.
use crate::font::{FontCollection, FontFaceDescriptor};
use crate::geometry::LogicalRect;
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use crate::{AtomicSizes, LayoutContext, Line, LineConstraint, LineResult, ParagraphBuilder};

thread_local! {
    static BOUNDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
pub(super) fn record_bounds() {
    BOUNDS.with(|n| n.set(n.get() + 1));
}

fn fonts(bytes: &[u8]) -> FontCollection {
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    fonts
        .register_face(
            bytes.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Outline".into(),
                ..Default::default()
            },
        )
        .unwrap();
    fonts
}

fn style() -> ParagraphStyle {
    ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("Outline".into())],
            font_size: 16.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn line(fonts: &FontCollection, style: &ParagraphStyle, text: &str) -> Line {
    let mut builder = ParagraphBuilder::new(style, &Limits::default());
    builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
    built_line(fonts, builder)
}

fn built_line(fonts: &FontCollection, builder: ParagraphBuilder) -> Line {
    let mut cx = LayoutContext::new();
    let paragraph = builder.build(&mut cx, fonts).unwrap();
    let LineResult::Line(line) = paragraph.next_line(
        &mut cx,
        paragraph.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(1e6),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("fixture must produce a line")
    };
    line
}

fn reference(line: &Line) -> [u32; 4] {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            crate::font::outline::BYPASS.with(|v| v.set(self.0));
        }
    }
    let _reset = Reset(crate::font::outline::BYPASS.with(|v| v.replace(true)));
    bits(line.overflow_rect())
}

fn check(line: &Line) {
    let expected = reference(line);
    for _ in 0..3 {
        assert_eq!(bits(line.overflow_rect()), expected);
    }
}

fn bits(rect: LogicalRect) -> [u32; 4] {
    [
        rect.inline_start.to_bits(),
        rect.block_start.to_bits(),
        rect.inline_size.to_bits(),
        rect.block_size.to_bits(),
    ]
}

#[test]
fn outlines_are_reused_across_glyphs_lines_and_paragraphs() {
    // Removing the cache must repeat real skrifa outline traversal here.
    for (bytes, text) in [
        (crate::test_support::fonts::LATIN, "aaaa"),
        (crate::test_support::fonts::CJK, "日日日日"),
    ] {
        let fonts = fonts(bytes);
        let first = line(&fonts, &style(), text);
        let other = line(&fonts, &style(), text);
        BOUNDS.with(|n| n.set(0));
        let expected = bits(first.overflow_rect());
        assert_eq!(
            BOUNDS.with(std::cell::Cell::get),
            1,
            "repeated glyphs traverse once"
        );
        for _ in 0..4 {
            assert_eq!(bits(first.overflow_rect()), expected);
            assert_eq!(bits(other.overflow_rect()), expected);
        }
        assert_eq!(
            BOUNDS.with(std::cell::Cell::get),
            1,
            "paragraphs share outline results"
        );
    }
}

#[test]
fn outline_rect_bits_match_uncached_for_directions_emphasis_and_ellipsis() {
    use crate::geometry::{Direction, WritingMode};
    use crate::style::{FontStyle, TextEmphasis, TextEmphasisPosition, TextEmphasisShape};
    for (bytes, text) in [
        (crate::test_support::fonts::LATIN, "office text space "),
        (crate::test_support::fonts::CJK, "日本語文章。日本語文章。"),
        (
            crate::test_support::fonts::ARABIC,
            "العربية العربية العربية",
        ),
    ] {
        let fonts = fonts(bytes);
        for direction in [Direction::Ltr, Direction::Rtl] {
            for mode in [
                WritingMode::HorizontalTb,
                WritingMode::VerticalRl,
                WritingMode::VerticalLr,
            ] {
                for emphasis in [false, true] {
                    let mut style = style();
                    style.direction = direction;
                    style.root.direction = direction;
                    style.writing_mode = mode;
                    style.root.font_style = FontStyle::Oblique(12.0);
                    if emphasis {
                        style.root.text_emphasis = Some(TextEmphasis {
                            shape: TextEmphasisShape::Dot,
                            filled: true,
                            position: TextEmphasisPosition::OverRight,
                        });
                    }
                    let line = line(&fonts, &style, text);
                    check(&line);
                    let before = bits(line.overflow_rect());
                    let mut truncated = line.clone();
                    assert!(
                        truncated
                            .truncate_with_ellipsis(
                                &mut LayoutContext::new(),
                                line.inline_size() / 2.0
                            )
                            .is_some()
                    );
                    check(&truncated);
                    assert_ne!(bits(truncated.overflow_rect()), before);
                    assert_eq!(
                        bits(line.overflow_rect()),
                        before,
                        "clone mutation cannot change its source"
                    );
                }
            }
        }
    }
}

#[test]
fn variable_outline_locations_and_actual_sizes_match_uncached() {
    use crate::style::{FontMetricKind, FontSizeAdjust, FontVariation};
    let bytes = include_bytes!("../../../../dev/fixtures/assets/fonts/emoji-mono.ttf");
    let fonts = fonts(bytes);
    let face = skrifa::FontRef::new(bytes).unwrap();
    use skrifa::{MetadataProvider, raw::TableProvider};
    assert!(
        face.fvar().is_ok() && face.gvar().is_ok(),
        "fixture has real variable outlines"
    );
    for size in [10.0, 16.0, 32.25] {
        for weight in [300.0, 400.0, 700.0] {
            let mut style = style();
            style.root.font_size = size;
            style.root.font_variations = vec![FontVariation {
                tag: *b"wght",
                value: weight,
            }];
            style.root.font_size_adjust = Some(FontSizeAdjust {
                metric: FontMetricKind::ExHeight,
                value: 0.5,
            });
            let line = line(&fonts, &style, "☺︎☺︎☺︎☺︎");
            let run = line
                .fragments()
                .find_map(|f| match f {
                    crate::Fragment::GlyphRun(run) => Some(run),
                    _ => None,
                })
                .unwrap();
            // skrifa represents the default (all-zero) location as an empty slice.
            if weight != 400.0 {
                assert!(!run.normalized_coords().is_empty());
            }
            assert!(!face.axes().is_empty());
            check(&line);
        }
    }
    // Emoji has no usable x-height; Latin exercises the difference between
    // authored size and the actual size used for the cached ink.
    let fonts = self::fonts(crate::test_support::fonts::LATIN);
    let mut adjusted = style();
    adjusted.root.font_size_adjust = Some(FontSizeAdjust {
        metric: FontMetricKind::ExHeight,
        value: 0.75,
    });
    let line = line(&fonts, &adjusted, "office text");
    let run = line
        .fragments()
        .find_map(|f| match f {
            crate::Fragment::GlyphRun(run) => Some(run),
            _ => None,
        })
        .unwrap();
    assert_ne!(run.font_size().to_bits(), adjusted.root.font_size.to_bits());
    check(&line);
}

#[test]
fn ruby_recursive_bounds_match_uncached_after_warming() {
    use crate::{
        Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle,
        RubyVisibility,
    };
    let fonts = fonts(crate::test_support::fonts::CJK);
    let style = style();
    let content = |node, text| {
        RubyContent::text(
            TextSource::Generated { node: NodeId(node) },
            text,
            &style.root,
            &Limits::default(),
        )
    };
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(2),
            content: content(3, "日本語"),
            align: RubyAlign::Center,
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(4),
                content: content(5, "文章"),
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.push_ruby(NodeId(1), &style.root, ruby);
    let line = built_line(&fonts, builder);
    assert!(
        line.fragments()
            .any(|f| matches!(f, crate::Fragment::RubyAnnotation(_))),
        "line={line:?} text={:?} containers={}",
        line.text(),
        line.data.ruby.containers.len()
    );
    check(&line);
}

#[test]
fn shared_fonts_cross_documents_and_lines_remain_thread_safe_without_extra_owners() {
    let fonts = fonts(crate::test_support::fonts::LATIN);
    let observer = fonts.layer_handle();
    let first = line(&fonts, &style(), "aaaa");
    let document = FontCollection::for_document(&fonts, &Limits::default());
    let other = line(&document, &style(), "aaaa");
    let font = first
        .fragments()
        .find_map(|f| match f {
            crate::Fragment::GlyphRun(run) => Some(run.font()),
            _ => None,
        })
        .unwrap();
    let blob = fonts.font_data(font).unwrap().data;
    let blob_observer = blob.downgrade();
    let blob_owners = blob.strong_count();
    BOUNDS.with(|n| n.set(0));
    let expected = bits(first.overflow_rect());
    assert_eq!(bits(other.overflow_rect()), expected);
    assert_eq!(
        BOUNDS.with(std::cell::Cell::get),
        1,
        "shared face routes to its owner layer"
    );
    assert_eq!(
        blob.strong_count(),
        blob_owners,
        "outline results retain no font blob owner"
    );
    let cold = line(&fonts, &style(), "office text ");
    let cold_expected = reference(&cold);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for _ in 0..16 {
                    assert_eq!(bits(cold.overflow_rect()), cold_expected);
                }
            });
        }
    });
    drop(cold);
    drop(document);
    drop(fonts);
    assert!(observer.is_alive(), "lines retain their original owner");
    check(&first);
    drop(other);
    drop(first);
    assert!(
        !observer.is_alive(),
        "cache cannot retain its layer after lines drop"
    );
    drop(blob);
    assert!(
        blob_observer.upgrade().is_none(),
        "font blobs are released with their owner"
    );
}
