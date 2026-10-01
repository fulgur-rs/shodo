use shodo::font::{FontCollection, FontOptions};
use shodo::limits::Limits;
use shodo::mapping::{Affinity, MappingKind, TextOrigin};
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{InlineStyle, ParagraphStyle, TextTransform, WhiteSpaceCollapse};
use shodo::{LayoutContext, Paragraph, ParagraphBuilder};

fn build(text: &str, transform: TextTransform, lang: Option<&str>) -> Paragraph {
    let limits = Limits::default();
    let style = ParagraphStyle {
        root: InlineStyle {
            text_transform: transform,
            lang: lang.map(str::to_owned),
            ..InlineStyle::default()
        },
        ..ParagraphStyle::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 10,
        },
        text,
    );
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..FontOptions::default()
        },
    );
    b.build(&mut LayoutContext::new(), &fonts).unwrap()
}

fn split(
    first: &str,
    second: &str,
    transform: TextTransform,
    lang: &str,
    mapping: bool,
) -> Paragraph {
    let limits = Limits::default();
    let s = InlineStyle {
        text_transform: transform,
        lang: Some(lang.to_owned()),
        ..InlineStyle::default()
    };
    let style = ParagraphStyle {
        root: s.clone(),
        ..ParagraphStyle::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.with_offset_mapping(mapping)
        .push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            first,
        )
        .open_inline(NodeId(2), &s, InlineEdges::default())
        .push_text(
            TextSource::Dom {
                node: NodeId(3),
                offset: 0,
            },
            second,
        )
        .close_inline();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..FontOptions::default()
        },
    );
    b.build(&mut LayoutContext::new(), &fonts).unwrap()
}

#[test]
fn turkic_case_cross_node_dot() {
    assert_eq!(
        split("I", "\u{0307} ıi", TextTransform::Lowercase, "tr-TR", true).text(),
        "i ıi"
    );
    assert_eq!(
        build("I\u{0307} ıi", TextTransform::Uppercase, Some("az")).text(),
        "I\u{0307} Iİ"
    );
}

#[test]
fn lithuanian_above_marks() {
    assert_eq!(
        build("I\u{0301} J\u{0300}", TextTransform::Lowercase, Some("lt")).text(),
        "i\u{0307}\u{0301} j\u{0307}\u{0300}"
    );
    assert_eq!(
        build("i\u{0307}\u{0301}", TextTransform::Uppercase, Some("lt")).text(),
        "I\u{0301}"
    );
}

#[test]
fn greek_final_sigma_and_tonos() {
    assert_eq!(
        split("Ο", "Σ ΟΣΑ", TextTransform::Lowercase, "el", true).text(),
        "ος οσα"
    );
    assert_eq!(
        build("άι ά ή", TextTransform::Uppercase, Some("el")).text(),
        "ΑΪ Ά Ή"
    );
}

#[test]
fn dutch_capitalize_ij_cross_node() {
    assert_eq!(
        split("i", "jSSEL", TextTransform::Capitalize, "nl-NL", true).text(),
        "IJSSEL"
    );
}

#[test]
fn capitalize_preserves_tail_and_word_context() {
    assert_eq!(
        split("h", "ELLo world", TextTransform::Capitalize, "en", true).text(),
        "HELLo World"
    );
    assert_eq!(
        build("'hello' 42foo élan", TextTransform::Capitalize, None).text(),
        "'Hello' 42foo Élan"
    );
}

#[test]
fn halfwidth_kana_voicing() {
    assert_eq!(
        split("ｶ", "ﾞ ｱA", TextTransform::FullWidth, "ja", true).text(),
        "ガ　アＡ"
    );
}

#[test]
fn full_width_maps_symbols_and_hangul_without_compatibility_normalization() {
    let input = "\u{2985}\u{2986}¢£¬¯¦¥₩\u{ffe8}\u{ffe9}\u{ffea}\u{ffeb}\u{ffec}\u{ffed}\u{ffee}\u{ffa0}\u{ffa1}\u{ffc2}\u{ffca}\u{ffd2}\u{ffda}①ﬀ";
    let expected = "\u{ff5f}\u{ff60}\u{ffe0}\u{ffe1}\u{ffe2}\u{ffe3}\u{ffe4}\u{ffe5}\u{ffe6}│←↑→↓■○\u{3164}\u{3131}\u{314f}\u{3155}\u{315b}\u{3161}①ﬀ";
    assert_eq!(
        build(input, TextTransform::FullWidth, None).text(),
        expected
    );
}

#[test]
fn full_width_expansion_keeps_dom_byte_origins_across_inline_boundaries() {
    let p = split("¢", "\u{ffa1}¥", TextTransform::FullWidth, "ko", true);
    assert_eq!(p.text(), "\u{ffe0}\u{3131}\u{ffe5}");
    let m = p.offset_mapping().unwrap();
    for (node, dom, text, affinity) in [
        (NodeId(1), 0, 0, Affinity::Downstream),
        (NodeId(1), 1, 0, Affinity::Downstream),
        (NodeId(1), 2, 3, Affinity::Upstream),
        (NodeId(3), 0, 3, Affinity::Downstream),
        (NodeId(3), 3, 6, Affinity::Downstream),
        (NodeId(3), 5, 9, Affinity::Upstream),
    ] {
        assert_eq!(m.dom_to_text(node, dom), Some((text, affinity)));
    }
    for (text, node, offset) in [
        (0, NodeId(1), 0),
        (1, NodeId(1), 0),
        (3, NodeId(3), 0),
        (6, NodeId(3), 3),
    ] {
        assert_eq!(
            m.text_to_dom(text, Affinity::Downstream),
            Some(TextOrigin::Dom { node, offset })
        );
    }
}

#[test]
fn full_size_kana_supplementary_mapping() {
    assert_eq!(
        build("ぁㇰｧ\u{1B132}", TextTransform::FullSizeKana, Some("ja")).text(),
        "あクｱこ"
    );
}

#[test]
fn expanded_scalar_dom_roundtrip() {
    let p = build("aßb", TextTransform::Uppercase, None);
    assert_eq!(p.text(), "ASSB");
    let m = p.offset_mapping().unwrap();
    let expanded = m
        .units()
        .iter()
        .find(|u| u.kind == MappingKind::Expanded)
        .unwrap();
    assert_eq!(expanded.dom, 11..13);
    assert_eq!(expanded.text, 1..3);
    assert_eq!(
        m.dom_to_text(NodeId(1), 12),
        Some((1, Affinity::Downstream))
    );
    assert_eq!(
        m.text_to_dom(2, Affinity::Downstream),
        Some(TextOrigin::Dom {
            node: NodeId(1),
            offset: 11
        })
    );
    assert_eq!(
        m.text_to_dom(3, Affinity::Downstream),
        Some(TextOrigin::Dom {
            node: NodeId(1),
            offset: 13
        })
    );
}

#[test]
fn transform_without_mapping() {
    let p = split("ß", "a", TextTransform::Uppercase, "de", false);
    assert_eq!(p.text(), "SSA");
    assert!(p.offset_mapping().is_none());
}

#[test]
fn invalid_language_warns() {
    let p = build("abc", TextTransform::Uppercase, Some("%%%bad%%%"));
    assert_eq!(p.text(), "ABC");
    assert!(!p.warnings().is_empty());
}

#[test]
fn transform_limits_before_append() {
    let limits = Limits {
        max_text_bytes: Some(2),
        ..Limits::default()
    };
    let style = ParagraphStyle {
        root: InlineStyle {
            text_transform: TextTransform::FullWidth,
            ..InlineStyle::default()
        },
        ..ParagraphStyle::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..FontOptions::default()
        },
    );
    assert!(b.build(&mut LayoutContext::new(), &fonts).is_err());
}

#[test]
fn preserved_controls_keep_origins_after_expansion() {
    let limits = Limits::default();
    let style = ParagraphStyle {
        root: InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            text_transform: TextTransform::Uppercase,
            ..InlineStyle::default()
        },
        ..ParagraphStyle::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "ß\tß\n",
    );
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..FontOptions::default()
        },
    );
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    assert_eq!(p.text(), "SS\tSS\n");
    assert_eq!(
        p.offset_mapping().unwrap().dom_to_text(NodeId(1), 2),
        Some((2, Affinity::Downstream))
    );
}

#[test]
fn width_kana_combinations() {
    use TextTransform::*;
    for (transform, expected) in [
        (CapitalizeFullWidth, "ＡＢｃ　ァガ"),
        (UppercaseFullWidth, "ＡＢＣ　ァガ"),
        (LowercaseFullWidth, "ａｂｃ　ァガ"),
        (CapitalizeFullSizeKana, "ABc アｶﾞ"),
        (UppercaseFullSizeKana, "ABC アｶﾞ"),
        (LowercaseFullSizeKana, "abc アｶﾞ"),
        (FullWidthFullSizeKana, "ａＢｃ　アガ"),
        (CapitalizeFullWidthFullSizeKana, "ＡＢｃ　アガ"),
        (UppercaseFullWidthFullSizeKana, "ＡＢＣ　アガ"),
        (LowercaseFullWidthFullSizeKana, "ａｂｃ　アガ"),
    ] {
        assert_eq!(build("aBc ァｶﾞ", transform, Some("ja")).text(), expected);
    }
}

#[test]
fn language_extensions_preserve_locale() {
    assert_eq!(
        build("I", TextTransform::Lowercase, Some("tr-TR-u-co-search")).text(),
        "ı"
    );
}

#[test]
fn transformed_nodes_keep_distinct_origins() {
    let p = split("ß", "ß", TextTransform::Uppercase, "de", true);
    let m = p.offset_mapping().unwrap();
    assert_eq!(p.text(), "SSSS");
    assert_eq!(
        m.text_to_dom(1, Affinity::Downstream),
        Some(TextOrigin::Dom {
            node: NodeId(1),
            offset: 0
        })
    );
    assert_eq!(
        m.text_to_dom(3, Affinity::Downstream),
        Some(TextOrigin::Dom {
            node: NodeId(3),
            offset: 0
        })
    );
}

#[test]
fn dutch_second_letter_requires_capitalized_head() {
    let limits = Limits::default();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    let style = InlineStyle {
        text_transform: TextTransform::Capitalize,
        lang: Some("nl".to_owned()),
        ..InlineStyle::default()
    };
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "i",
    )
    .open_inline(NodeId(2), &style, InlineEdges::default())
    .push_text(
        TextSource::Dom {
            node: NodeId(3),
            offset: 0,
        },
        "jssel",
    )
    .close_inline();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..FontOptions::default()
        },
    );
    assert_eq!(
        b.build(&mut LayoutContext::new(), &fonts).unwrap().text(),
        "ijssel"
    );
}

#[test]
fn full_width_kana_across_transparent_controls() {
    use shodo::style::UnicodeBidi;
    let limits = Limits::default();
    let style = InlineStyle {
        text_transform: TextTransform::FullWidth,
        ..InlineStyle::default()
    };
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style.clone(),
            ..ParagraphStyle::default()
        },
        &limits,
    );
    let isolate = InlineStyle {
        unicode_bidi: UnicodeBidi::Isolate,
        ..style
    };
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "ｶ",
    )
    .open_inline(NodeId(2), &isolate, InlineEdges::default())
    .push_text(
        TextSource::Dom {
            node: NodeId(3),
            offset: 0,
        },
        "ﾞ",
    )
    .close_inline();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..FontOptions::default()
        },
    );
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    assert_eq!(p.text(), "ガ\u{2066}\u{2069}");
    assert_eq!(
        p.offset_mapping().unwrap().dom_to_text(NodeId(3), 0),
        Some((6, Affinity::Downstream))
    );
}

#[test]
fn oof_does_not_split_case_context() {
    use shodo::node::OutOfFlowKind;
    let limits = Limits::default();
    let style = ParagraphStyle {
        root: InlineStyle {
            text_transform: TextTransform::Lowercase,
            ..InlineStyle::default()
        },
        ..ParagraphStyle::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "Ο",
    )
    .push_out_of_flow(NodeId(2), OutOfFlowKind::Absolute)
    .push_text(
        TextSource::Dom {
            node: NodeId(3),
            offset: 0,
        },
        "Σ",
    );
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..FontOptions::default()
        },
    );
    assert_eq!(
        b.build(&mut LayoutContext::new(), &fonts).unwrap().text(),
        "ο\u{FFFC}ς"
    );
}

#[test]
fn long_transparent_word_keeps_capitalization_context() {
    let limits = Limits::default();
    let inline = InlineStyle {
        text_transform: TextTransform::Capitalize,
        ..InlineStyle::default()
    };
    let style = ParagraphStyle {
        root: inline.clone(),
        ..ParagraphStyle::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    for i in 0..4096 {
        b.open_inline(NodeId(i + 1), &inline, InlineEdges::default())
            .push_text(
                TextSource::Dom {
                    node: NodeId(i + 10000),
                    offset: 0,
                },
                "a",
            )
            .close_inline();
    }
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..FontOptions::default()
        },
    );
    assert_eq!(
        b.build(&mut LayoutContext::new(), &fonts).unwrap().text(),
        format!("A{}", "a".repeat(4095))
    );
}

#[test]
#[cfg(all(
    feature = "complex-scripts",
    debug_assertions,
    not(target_arch = "wasm32")
))]
fn transformed_paragraphs_load_complex_models() {
    const CHILD: &str = "SHODO_COMPLEX_TRANSFORM_PROBE";
    if std::env::var_os(CHILD).is_some() {
        for (lang, text) in [
            ("ja", "こんにちは世界"),
            ("km", "ភាសាខ្មែរជាភាសាជាតិ"),
            ("th", "ภาษาไทย"),
        ] {
            for transform in [
                TextTransform::None,
                TextTransform::Uppercase,
                TextTransform::Capitalize,
            ] {
                assert_eq!(build(text, transform, Some(lang)).text(), text);
            }
        }
        println!("complex transform probe completed");
        return;
    }
    // --nocapture lets ICU's debug eprintln fallback reach child stderr;
    // the parent isolates it from the surrounding harness's capture.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "transformed_paragraphs_load_complex_models",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(
        stdout.contains("complex transform probe completed"),
        "{stdout}"
    );
    assert!(!stderr.contains("No segmentation model"), "{stderr}");
}
