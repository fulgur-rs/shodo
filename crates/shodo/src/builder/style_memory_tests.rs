use super::*;
use crate::style::FontFamily;

#[test]
fn repeated_large_equal_styles_are_bounded_before_item_or_storage_limits() {
    let style = InlineStyle {
        font_families: vec![FontFamily::Named("x".repeat(256 * 1024))],
        ..Default::default()
    };
    for forced_break in [false, true] {
        let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        builder.push_atomic(NodeId(0), &style, InlineEdges::default());
        assert_eq!(builder.error(), None);
        let retained = builder.style_bytes;
        for index in 1..4096 {
            if forced_break {
                builder.push_forced_break_with_style(NodeId(index), &style);
            } else {
                builder.push_atomic(NodeId(index), &style, InlineEdges::default());
            }
        }
        assert!(
            builder.error().is_some(),
            "repeated comparison work must be bounded even when style storage does not grow"
        );
        assert_eq!(builder.error().unwrap().kind, LimitKind::StyleWork);
        assert_eq!(builder.styles.len(), 2);
        assert_eq!(builder.style_bytes, retained);
        assert!(builder.items.len() < 4096);
    }
}

#[test]
fn style_work_exact_boundary_is_terminal_and_survives_builder_clone() {
    // One operation plus a 24-byte family slot per root hash/comparison.
    let limits = Limits {
        max_style_work: Some(50),
        ..Default::default()
    };
    let paragraph = ParagraphStyle::default();
    let mut builder = ParagraphBuilder::new(&paragraph, &limits);
    builder.push_atomic(NodeId(1), &paragraph.root, InlineEdges::default());
    assert_eq!(builder.error(), None);
    assert_eq!(builder.items.len(), 1);
    let mut builder = builder.clone();
    let bytes = builder.style_bytes;
    builder.push_forced_break_with_style(NodeId(2), &paragraph.root);
    let error = builder.error().unwrap();
    assert_eq!(
        error,
        LimitExceeded {
            kind: LimitKind::StyleWork,
            limit: 50,
            actual: 51
        }
    );
    builder.push_text(TextSource::Generated { node: NodeId(3) }, "ignored");
    builder.open_inline(NodeId(4), &paragraph.root, InlineEdges::default());
    assert_eq!(builder.error(), Some(error));
    assert_eq!(builder.items.len(), 1);
    assert_eq!(builder.styles.len(), 1);
    assert_eq!(builder.style_bytes, bytes);
    assert!(builder.text.is_empty() && builder.stack.is_empty());
    assert_eq!(builder.analyze().err(), Some(error));

    let limits = Limits {
        max_style_work: Some(24),
        ..Default::default()
    };
    let rejected = ParagraphBuilder::new(&paragraph, &limits);
    assert_eq!(rejected.error().unwrap().kind, LimitKind::StyleWork);
    assert!(rejected.styles.is_empty() && rejected.style_index.is_empty());
}

#[test]
fn style_work_charges_last_hit_hash_bucket_and_each_collision_candidate() {
    let a = InlineStyle {
        font_size: 20.0,
        ..Default::default()
    };
    let c = InlineStyle {
        font_size: 30.0,
        ..Default::default()
    };
    let limits = Limits {
        max_style_work: Some(150),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    assert_eq!(builder.intern_styles(&a, None), Some(1));
    assert_eq!(builder.intern_styles(&a, None), Some(1));
    assert_eq!(
        builder.error(),
        None,
        "root + first insert + last hit costs 150"
    );
    assert_eq!(builder.intern_styles(&a, None), None);
    assert_eq!(builder.error().unwrap().actual, 151);

    for limit in [274, 275] {
        let limits = Limits {
            max_style_work: Some(limit),
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        assert_eq!(builder.intern_styles(&a, None), Some(1));
        assert_eq!(builder.intern_styles(&c, None), Some(2));
        assert_eq!(builder.intern_styles(&a, None), (limit == 275).then_some(1));
        assert_eq!(builder.error().is_none(), limit == 275);
        assert_eq!(builder.styles.len(), 3);
    }

    let limits = Limits {
        max_style_work: Some(150),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    let hash = style_key::fingerprint((&a, None), builder.style_index.hasher());
    builder.style_index.insert(hash, vec![0; 12]);
    assert_eq!(builder.intern_styles(&a, None), None);
    assert_eq!(builder.error().unwrap().actual, 151);
    assert_eq!(
        builder.styles.len(),
        1,
        "reject during the collision walk, before retaining a style"
    );
}

#[test]
fn style_work_reserves_every_variable_field_and_first_line_before_comparison() {
    use crate::style::{FontFeature, FontVariantAlternates, FontVariation, GenericFamily};
    let cases = [
        InlineStyle {
            font_families: vec![FontFamily::Named("x".repeat(4096))],
            ..Default::default()
        },
        InlineStyle {
            font_families: vec![FontFamily::Generic(GenericFamily::Serif); 4096],
            ..Default::default()
        },
        InlineStyle {
            font_families: vec![FontFamily::Named(String::new()); 4096],
            ..Default::default()
        },
        InlineStyle {
            lang: Some("x".repeat(4096)),
            ..Default::default()
        },
        InlineStyle {
            hyphenate_character: Some("x".repeat(4096)),
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
            font_variant_alternates: FontVariantAlternates {
                styleset: vec![1; 4096],
                ..Default::default()
            },
            ..Default::default()
        },
        InlineStyle {
            font_variant_alternates: FontVariantAlternates {
                character_variant: vec![(1, 1); 4096],
                ..Default::default()
            },
            ..Default::default()
        },
    ];
    for style in cases {
        for first_line in [false, true] {
            let limits = Limits {
                max_style_work: Some(128),
                ..Default::default()
            };
            let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
            if first_line {
                builder.open_inline_with_first_line(
                    NodeId(1),
                    &InlineStyle::default(),
                    &style,
                    InlineEdges::default(),
                );
            } else {
                builder.push_atomic(NodeId(1), &style, InlineEdges::default());
            }
            assert_eq!(builder.error().unwrap().kind, LimitKind::StyleWork);
            assert_eq!(builder.styles.len(), 1);
            assert!(builder.first_line_styles.is_empty() && builder.items.is_empty());
        }
    }
}

#[test]
fn style_work_can_be_disabled_without_disabling_storage_limits() {
    let limits = Limits {
        max_style_work: None,
        max_style_bytes: Some(12_000),
        ..Default::default()
    };
    let style = InlineStyle {
        font_families: vec![FontFamily::Named("x".repeat(4096))],
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    for index in 0..1000 {
        builder.push_forced_break_with_style(NodeId(index), &style);
    }
    assert_eq!(builder.error(), None);
    assert_eq!(builder.styles.len(), 2);
    let other = InlineStyle {
        font_size: 20.0,
        ..style
    };
    for font_size in [21.0, 22.0, 23.0] {
        builder.push_atomic(
            NodeId(1001),
            &InlineStyle {
                font_size,
                ..other.clone()
            },
            InlineEdges::default(),
        );
    }
    assert_eq!(builder.error().unwrap().kind, LimitKind::StyleBytes);
    assert_eq!(Limits::unlimited().max_style_work, None);
}

#[test]
fn default_style_work_allows_the_default_item_budget_with_small_reused_styles() {
    let paragraph = ParagraphStyle::default();
    let limits = Limits::default();
    let mut builder = ParagraphBuilder::new(&paragraph, &limits);
    // Reuse through the actual key path, without allocating a million items.
    for _ in 0..limits.max_items.unwrap() {
        assert_eq!(builder.intern_styles(&paragraph.root, None), Some(0));
    }
    assert_eq!(builder.error(), None);
    assert_eq!(builder.styles.len(), 1);
}

#[test]
fn style_work_covers_first_line_root_and_ruby_restore_and_import() {
    let large = InlineStyle {
        font_families: vec![FontFamily::Named("x".repeat(4096))],
        ..Default::default()
    };
    let limits = Limits {
        max_style_work: Some(128),
        ..Default::default()
    };
    let paragraph = ParagraphStyle {
        first_line: Some(large.clone()),
        ..Default::default()
    };
    let builder = ParagraphBuilder::new(&paragraph, &limits);
    assert_eq!(builder.error().unwrap().kind, LimitKind::StyleWork);
    assert!(builder.styles.is_empty());

    let source = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
    let mut input = source.into_ruby_content();
    input.limits.max_style_work = Some(24);
    let error = ParagraphBuilder::from_ruby_content(&input, NodeId(1))
        .err()
        .unwrap();
    assert_eq!(error.kind, LimitKind::StyleWork);

    use crate::ruby::{Ruby, RubyAlign, RubyBase, RubyContent};
    let content = RubyContent::text(
        TextSource::Generated { node: NodeId(2) },
        "base",
        &large,
        &Limits::unlimited(),
    );
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(2),
            content,
            align: RubyAlign::default(),
        }],
        vec![],
    )
    .unwrap();
    let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    parent.push_ruby(NodeId(3), &InlineStyle::default(), ruby);
    assert_eq!(parent.error().unwrap().kind, LimitKind::StyleWork);
}

#[test]
fn escaped_names_fit_the_owned_data_budget_without_a_string_key() {
    let paragraph = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("\0".repeat(4096))],
            ..Default::default()
        },
        ..Default::default()
    };
    let limits = Limits {
        max_style_bytes: Some(12_000),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&paragraph, &limits);
    assert_eq!(b.error(), None, "two owned copies fit the budget");
    let alternate = InlineStyle {
        font_size: 20.0,
        ..paragraph.root.clone()
    };
    b.open_inline(NodeId(1), &alternate, InlineEdges::default());
    assert_eq!(b.error().unwrap().kind, LimitKind::StyleBytes);
    assert_eq!(b.styles.len(), 1, "reject the third copy before retention");
}

#[test]
fn signed_zero_style_identity_does_not_depend_on_reuse_order() {
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    let negative = InlineStyle {
        letter_spacing: -0.0,
        ..Default::default()
    };
    assert_eq!(b.intern_styles(&negative, None), Some(1));
    assert_eq!(b.intern_styles(&InlineStyle::default(), None), Some(0));
    let other = InlineStyle {
        font_weight: 600.0,
        ..Default::default()
    };
    assert_eq!(b.intern_styles(&other, None), Some(2));
    assert_eq!(b.intern_styles(&negative, None), Some(1));
    assert_eq!(b.styles.len(), 3);
    assert_eq!(b.styles[1].letter_spacing.to_bits(), (-0.0_f32).to_bits());
}

#[test]
fn nan_payloads_share_normal_and_first_line_style_pairs() {
    let normal = InlineStyle {
        font_style: crate::style::FontStyle::Oblique(f32::from_bits(0x7fc0_0001)),
        ..Default::default()
    };
    let first = InlineStyle {
        paint: crate::style::PaintStyle {
            underline: Some(crate::style::TextDecoration {
                offset: Some(f32::from_bits(0xffc0_1234)),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    let index = b.intern_styles(&normal, Some(&first)).unwrap();
    let mut same_normal = normal.clone();
    same_normal.font_style = crate::style::FontStyle::Oblique(f32::from_bits(0xffc0_0002));
    let mut same_first = first.clone();
    same_first.paint.underline.as_mut().unwrap().offset = Some(f32::NAN);
    assert_eq!(
        b.intern_styles(&same_normal, Some(&same_first)),
        Some(index)
    );
    assert_ne!(b.intern_styles(&normal, None), Some(index));
    assert_ne!(
        b.intern_styles(&normal, Some(&InlineStyle::default())),
        Some(index)
    );
    assert_eq!(b.styles.len(), 4);
}

#[test]
fn paint_changes_cannot_multiply_unbounded_family_storage() {
    let mut inline = InlineStyle {
        font_families: vec![FontFamily::Named("x".repeat(256 * 1024))],
        ..Default::default()
    };
    // With no duplicated Debug key, 192 copies retain about 48 MiB. Choose
    // a smaller explicit cap so this still exercises rejection before cloning.
    let limits = Limits {
        max_style_bytes: Some(32 * 1024 * 1024),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
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
fn oversized_root_data_is_bounded_before_retention() {
    let paragraph = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("\0".repeat(16 * 1024 * 1024))],
            ..Default::default()
        },
        ..Default::default()
    };
    let limits = Limits {
        max_style_bytes: Some(16 * 1024 * 1024),
        ..Default::default()
    };
    let b = ParagraphBuilder::new(&paragraph, &limits);
    assert!(
        b.error().is_some(),
        "owned style copies must have a byte cap"
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
    let hash = super::style_key::fingerprint(pair, b.style_index.hasher());
    b.style_index.entry(hash).or_default().push(0);
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
    let fonts = crate::test_support::fonts::collection(&Limits::default());
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
    let fonts = crate::test_support::fonts::collection(&Limits::default());
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
    let fonts = crate::test_support::fonts::collection(&Limits::default());
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
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    let error = b.build(&mut LayoutContext::new(), &fonts).err().unwrap();
    assert_eq!((error.kind, error.limit), (LimitKind::StyleBytes, 32_768));
}
