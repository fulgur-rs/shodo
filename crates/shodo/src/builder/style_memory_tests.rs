use super::*;
use crate::style::FontFamily;

#[test]
fn paint_changes_cannot_multiply_unbounded_family_storage() {
    let mut inline = InlineStyle {
        font_families: vec![FontFamily::Named("x".repeat(256 * 1024))],
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    for i in 0..192u64 {
        inline.paint.color = [i as u8, 0, 0, 255];
        b.open_inline(NodeId(i), &inline, InlineEdges::default())
            .close_inline();
    }
    assert!(
        b.error().is_some(),
        "large inherited payload must exhaust a byte budget"
    );
    let retained = b.styles.len();
    b.open_inline(NodeId(200), &inline, InlineEdges::default());
    assert_eq!(b.styles.len(), retained);
}

#[test]
fn escaped_root_key_is_bounded_before_retention() {
    let paragraph = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("\0".repeat(16 * 1024 * 1024))],
            ..Default::default()
        },
        ..Default::default()
    };
    let b = ParagraphBuilder::new(&paragraph, &Limits::default());
    assert!(
        b.error().is_some(),
        "escaped Debug keys must also have a byte cap"
    );
}

#[test]
fn every_variable_style_field_is_checked_before_cloning() {
    use crate::style::{FontFeature, FontVariation};
    let cases = vec![
        InlineStyle {
            font_families: vec![FontFamily::Named("x".repeat(16_384))],
            ..Default::default()
        },
        InlineStyle {
            lang: Some("x".repeat(16_384)),
            ..Default::default()
        },
        InlineStyle {
            hyphenate_character: Some("x".repeat(16_384)),
            ..Default::default()
        },
        InlineStyle {
            font_features: vec![
                FontFeature {
                    tag: *b"liga",
                    value: 1
                };
                4096
            ],
            ..Default::default()
        },
        InlineStyle {
            font_variations: vec![
                FontVariation {
                    tag: *b"wght",
                    value: 400.0
                };
                4096
            ],
            ..Default::default()
        },
        InlineStyle {
            font_variant_alternates: crate::style::FontVariantAlternates {
                styleset: vec![1; 16_384],
                ..Default::default()
            },
            ..Default::default()
        },
        InlineStyle {
            font_variant_alternates: crate::style::FontVariantAlternates {
                character_variant: vec![(1, 1); 4096],
                ..Default::default()
            },
            ..Default::default()
        },
    ];
    let limits = Limits {
        max_style_bytes: Some(8192),
        ..Default::default()
    };
    for inline in cases {
        for first in [false, true] {
            let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
            assert_eq!(b.error(), None);
            if first {
                b.open_inline_with_first_line(
                    NodeId(1),
                    &InlineStyle::default(),
                    &inline,
                    InlineEdges::default(),
                );
            } else {
                b.push_atomic(NodeId(1), &inline, InlineEdges::default());
            }
            assert_eq!(b.error().unwrap().kind, LimitKind::StyleBytes);
            assert_eq!(b.styles.len(), 1);
            assert!(b.first_line_styles.is_empty() && b.items.is_empty());
        }
        for first in [false, true] {
            let paragraph = if first {
                ParagraphStyle {
                    first_line: Some(inline.clone()),
                    ..Default::default()
                }
            } else {
                ParagraphStyle {
                    root: inline.clone(),
                    ..Default::default()
                }
            };
            let b = ParagraphBuilder::new(&paragraph, &limits);
            assert_eq!(b.error().unwrap().kind, LimitKind::StyleBytes);
            assert!(b.styles.is_empty() && b.style_index.is_empty());
        }
    }
}

#[test]
fn unlimited_and_nonconsecutive_reuse_preserve_interning() {
    let limits = Limits {
        max_style_bytes: Some(12_000),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    let a = InlineStyle {
        font_weight: 600.0,
        lang: Some("en".into()),
        ..Default::default()
    };
    let c = InlineStyle {
        font_size: 20.0,
        ..Default::default()
    };
    for _ in 0..100 {
        b.open_inline(NodeId(1), &a, InlineEdges::default())
            .close_inline();
        b.open_inline(NodeId(2), &c, InlineEdges::default())
            .close_inline();
    }
    assert_eq!(b.error(), None);
    assert_eq!(b.styles.len(), 3);
    let bytes = b.style_bytes;
    b.open_inline(NodeId(1), &a, InlineEdges::default());
    assert_eq!(b.style_bytes, bytes);
    let paragraph = ParagraphStyle {
        root: InlineStyle {
            lang: Some("x".repeat(16_384)),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        ParagraphBuilder::new(&paragraph, &Limits::unlimited()).error(),
        None
    );
}

#[test]
fn hash_collisions_and_nan_keys_preserve_debug_equality() {
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    let a = InlineStyle {
        font_weight: 600.0,
        ..Default::default()
    };
    let pair = (&a, None);
    let (hash, _) = super::style_key::fingerprint(pair, b.style_index.hasher(), None).unwrap();
    b.style_index
        .entry(hash)
        .or_default()
        .push(("different key".into(), 0));
    b.open_inline(NodeId(1), &a, InlineEdges::default())
        .close_inline();
    assert_eq!(b.styles.len(), 2);
    let nan = InlineStyle {
        font_size: f32::NAN,
        ..Default::default()
    };
    for _ in 0..3 {
        b.open_inline(NodeId(2), &nan, InlineEdges::default())
            .close_inline();
    }
    assert_eq!(b.styles.len(), 3);
    assert_eq!(b.error(), None);
}

#[test]
fn first_line_inheritance_after_sanitization_checks_generated_bytes() {
    // A malformed language is dropped from normal input. All children then
    // inherit the huge, still-raw first-line language before its sanitization.
    let normal = InlineStyle {
        lang: Some("!".into()),
        ..Default::default()
    };
    let first = InlineStyle {
        lang: Some("x".repeat(12_000)),
        ..Default::default()
    };
    let paragraph = ParagraphStyle {
        root: normal,
        first_line: Some(first),
        ..Default::default()
    };
    let limits = Limits {
        max_style_bytes: Some(64_000),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&paragraph, &limits);
    for i in 0..5 {
        let style = InlineStyle {
            font_size: 20.0 + i as f32,
            lang: Some("?".into()),
            ..Default::default()
        };
        b.open_inline(NodeId(i), &style, InlineEdges::default())
            .close_inline();
    }
    assert_eq!(b.error(), None);
    let fonts = FontCollection::with_options(
        &Limits::default(),
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let error = b.build(&mut LayoutContext::new(), &fonts).err().unwrap();
    assert_eq!(error.kind, LimitKind::StyleBytes);
}

#[test]
fn ruby_snapshot_occurrences_are_aggregated_even_when_hidden() {
    use crate::ruby::*;
    let inline = InlineStyle {
        lang: Some("x".repeat(1024)),
        ..Default::default()
    };
    let reading = RubyContent::text(
        TextSource::Generated { node: NodeId(2) },
        "b",
        &inline,
        &Limits::unlimited(),
    );
    for visibility in [
        RubyVisibility::Visible,
        RubyVisibility::Hidden,
        RubyVisibility::Collapse,
    ] {
        let base = RubyContent::text(
            TextSource::Generated { node: NodeId(1) },
            "a",
            &InlineStyle::default(),
            &Limits::default(),
        );
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(1),
                content: base,
                align: RubyAlign::default(),
            }],
            (0..16)
                .map(|i| RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(i + 10),
                        content: reading.clone(),
                        span: RubySpan::All,
                        visibility,
                    }],
                    style: RubyStyle::default(),
                })
                .collect(),
        )
        .unwrap();
        let limits = Limits {
            max_style_bytes: Some(16_000),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        b.push_ruby(NodeId(3), &InlineStyle::default(), ruby);
        assert_eq!(b.error().unwrap().kind, LimitKind::StyleBytes);
        assert!(b.rubies.is_empty());
    }
}

#[test]
fn nested_base_bytes_follow_remapped_styles_not_unrelated_parent_styles() {
    use crate::ruby::*;
    let small = Limits {
        max_style_bytes: Some(8192),
        ..Default::default()
    };
    let content = RubyContent::text(
        TextSource::Generated { node: NodeId(1) },
        "a",
        &InlineStyle::default(),
        &small,
    );
    let inner = Ruby::new(
        vec![RubyBase {
            node: NodeId(1),
            content,
            align: RubyAlign::default(),
        }],
        vec![],
    )
    .unwrap();
    let mut middle = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    middle.push_ruby(NodeId(2), &InlineStyle::default(), inner);
    assert_eq!(middle.error(), None);
    let outer = Ruby::new(
        vec![RubyBase {
            node: NodeId(3),
            content: RubyContent::from_builder(middle),
            align: RubyAlign::default(),
        }],
        vec![],
    )
    .unwrap();
    let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    let unrelated = InlineStyle {
        font_families: vec![FontFamily::Named("x".repeat(20_000))],
        ..Default::default()
    };
    parent.push_atomic(NodeId(4), &unrelated, InlineEdges::default());
    parent.push_ruby(NodeId(5), &InlineStyle::default(), outer);
    assert_eq!(parent.error(), None);
    let fonts = FontCollection::with_options(
        &Limits::default(),
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    assert!(
        parent.build(&mut LayoutContext::new(), &fonts).is_ok(),
        "nested base must use its imported style IDs"
    );
}

#[test]
fn imported_base_keeps_its_own_first_line_byte_cap() {
    use crate::ruby::*;
    let small = Limits {
        max_style_bytes: Some(8192),
        ..Default::default()
    };
    let content = RubyContent::text(
        TextSource::Generated { node: NodeId(1) },
        "a",
        &InlineStyle::default(),
        &small,
    );
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(1),
            content,
            align: RubyAlign::default(),
        }],
        vec![],
    )
    .unwrap();
    let paragraph = ParagraphStyle {
        first_line: Some(InlineStyle {
            font_families: vec![FontFamily::Named("x".repeat(20_000))],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&paragraph, &Limits::default());
    b.push_ruby(NodeId(2), &InlineStyle::default(), ruby);
    assert_eq!(b.error(), None);
    let fonts = FontCollection::with_options(
        &Limits::default(),
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let error = b.build(&mut LayoutContext::new(), &fonts).err().unwrap();
    assert_eq!((error.kind, error.limit), (LimitKind::StyleBytes, 8192));
}

#[test]
fn generated_annotation_style_copies_share_the_parent_byte_cap() {
    use crate::ruby::*;
    let reading_style = InlineStyle {
        font_families: vec![FontFamily::Named("x".repeat(4096))],
        ..Default::default()
    };
    let reading = RubyContent::text(
        TextSource::Generated { node: NodeId(2) },
        "b",
        &reading_style,
        &Limits::unlimited(),
    );
    let base = RubyContent::text(
        TextSource::Generated { node: NodeId(1) },
        "a",
        &InlineStyle::default(),
        &Limits::default(),
    );
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(1),
            content: base,
            align: RubyAlign::default(),
        }],
        (0..3)
            .map(|i| RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(10 + i),
                    content: reading.clone(),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            })
            .collect(),
    )
    .unwrap();
    let limits = Limits {
        max_style_bytes: Some(32_768),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_ruby(NodeId(3), &InlineStyle::default(), ruby);
    assert_eq!(
        b.error(),
        None,
        "raw shared inputs fit; generated lane datasets must still be capped"
    );
    let fonts = FontCollection::with_options(
        &Limits::default(),
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let error = b.build(&mut LayoutContext::new(), &fonts).err().unwrap();
    assert_eq!((error.kind, error.limit), (LimitKind::StyleBytes, 32_768));
}
