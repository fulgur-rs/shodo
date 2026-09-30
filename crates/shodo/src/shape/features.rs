//! CSS feature components followed by explicit author settings (last wins).
use crate::style::*;

#[cfg(test)]
std::thread_local! {
    pub(super) static STYLE_FEATURE_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn for_item(
    style: &InlineStyle,
    item: &crate::analysis::itemize::ShapeItem,
) -> Vec<harfrust::Feature> {
    let mut result = for_orientation(style, item.orientation);
    if let Some(tag) = item.width_feature {
        // Automatic composition features precede explicit author settings.
        result.insert(0, harfrust::Feature::new(harfrust::Tag::new(&tag), 1, ..));
    }
    result
}

pub(super) fn for_orientation(
    style: &InlineStyle,
    orientation: super::orientation::RunOrientation,
) -> Vec<harfrust::Feature> {
    if orientation != super::orientation::RunOrientation::Upright {
        return features(style);
    }
    let explicit_vert = style.font_features.iter().any(|f| f.tag == *b"vert");
    let explicit_vrt2 = style
        .font_features
        .iter()
        .rfind(|f| f.tag == *b"vrt2")
        .is_some_and(|f| f.value != 0);
    let mut result = vec![
        harfrust::Feature::new(
            harfrust::Tag::new(b"vert"),
            u32::from(explicit_vert || !explicit_vrt2),
            ..,
        ),
        harfrust::Feature::new(harfrust::Tag::new(b"vrt2"), 0, ..),
    ];
    match style.font_kerning {
        FontKerning::Auto => {}
        FontKerning::Normal => {
            result.push(harfrust::Feature::new(harfrust::Tag::new(b"vkrn"), 1, ..));
        }
        FontKerning::None => {
            result.push(harfrust::Feature::new(harfrust::Tag::new(b"vkrn"), 0, ..));
        }
    }
    // Explicit settings come last, including an explicit vert/vrt2 choice.
    append_features(style, &mut result);
    result
}

pub(super) fn features(s: &InlineStyle) -> Vec<harfrust::Feature> {
    let mut result = Vec::new();
    append_features(s, &mut result);
    result
}

fn append_features(s: &InlineStyle, result: &mut Vec<harfrust::Feature>) {
    #[cfg(test)]
    STYLE_FEATURE_BUILDS.with(|count| count.set(count.get() + 1));
    let mut push = |tag: &[u8; 4], value| {
        result.push(harfrust::Feature::new(harfrust::Tag::new(tag), value, ..))
    };
    match s.font_kerning {
        FontKerning::Auto => {}
        FontKerning::Normal => push(b"kern", 1),
        FontKerning::None => push(b"kern", 0),
    }
    let lig = s.font_variant_ligatures;
    if lig.none {
        for tag in [b"liga", b"clig", b"dlig", b"hlig", b"calt"] {
            push(tag, 0);
        }
    } else {
        if let Some(v) = lig.common {
            push(b"liga", u32::from(v));
            push(b"clig", u32::from(v));
        }
        for (tag, value) in [
            (b"dlig", lig.discretionary),
            (b"hlig", lig.historical),
            (b"calt", lig.contextual),
        ] {
            if let Some(v) = value {
                push(tag, u32::from(v));
            }
        }
    }
    use FontVariantCaps::*;
    match s.font_variant_caps {
        Normal => {}
        SmallCaps => push(b"smcp", 1),
        AllSmallCaps => {
            push(b"smcp", 1);
            push(b"c2sc", 1);
        }
        PetiteCaps => push(b"pcap", 1),
        AllPetiteCaps => {
            push(b"pcap", 1);
            push(b"c2pc", 1);
        }
        Unicase => push(b"unic", 1),
        TitlingCaps => push(b"titl", 1),
    }
    let n = s.font_variant_numeric;
    for (tag, on) in [
        (b"lnum", n.lining_nums),
        (b"onum", n.oldstyle_nums),
        (b"pnum", n.proportional_nums),
        (b"tnum", n.tabular_nums),
        (b"frac", n.diagonal_fractions),
        (b"afrc", n.stacked_fractions),
        (b"ordn", n.ordinal),
        (b"zero", n.slashed_zero),
    ] {
        if on {
            push(tag, 1);
        }
    }
    if let Some(variant) = s.font_variant_east_asian.variant {
        use FontVariantEastAsianVariant::*;
        push(
            match variant {
                Jis78 => b"jp78",
                Jis83 => b"jp83",
                Jis90 => b"jp90",
                Jis04 => b"jp04",
                Simplified => b"smpl",
                Traditional => b"trad",
            },
            1,
        );
    }
    if let Some(width) = s.font_variant_east_asian.width {
        push(
            match width {
                FontVariantEastAsianWidth::FullWidth => b"fwid",
                FontVariantEastAsianWidth::ProportionalWidth => b"pwid",
            },
            1,
        );
    }
    if s.font_variant_east_asian.ruby {
        push(b"ruby", 1);
    }
    match s.font_variant_position {
        FontVariantPosition::Normal => {}
        FontVariantPosition::Sub => push(b"subs", 1),
        FontVariantPosition::Super => push(b"sups", 1),
    }
    let a = &s.font_variant_alternates;
    if a.historical_forms {
        push(b"hist", 1);
    }
    for (tag, value) in [
        (b"salt", a.stylistic),
        (b"swsh", a.swash),
        (b"cswh", a.swash),
        (b"ornm", a.ornaments),
        (b"nalt", a.annotation),
    ] {
        if let Some(v) = value {
            push(tag, v);
        }
    }
    for n in &a.styleset {
        if (1..=20).contains(n) {
            push(&[b's', b's', b'0' + n / 10, b'0' + n % 10], 1);
        }
    }
    for (n, v) in &a.character_variant {
        if (1..=99).contains(n) {
            push(&[b'c', b'v', b'0' + n / 10, b'0' + n % 10], *v);
        }
    }
    if s.letter_spacing != 0.0 {
        for tag in [b"liga", b"clig", b"dlig", b"hlig"] {
            push(tag, 0);
        }
    }
    for feature in &s.font_features {
        push(&feature.tag, feature.value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item_features(
        style: &InlineStyle,
        orientation: super::super::orientation::RunOrientation,
        width_feature: Option<[u8; 4]>,
    ) -> Vec<([u8; 4], u32)> {
        let item = crate::analysis::itemize::ShapeItem {
            segment: 0,
            scalars: Vec::new(),
            end: 0,
            style: 0,
            level: 0,
            script: *b"Latn",
            font: None,
            orientation,
            combine: None,
            width_feature,
            before: String::new(),
            after: String::new(),
        };
        STYLE_FEATURE_BUILDS.with(|count| count.set(0));
        let features = for_item(style, &item);
        assert_eq!(STYLE_FEATURE_BUILDS.with(|count| count.get()), 1);
        features
            .iter()
            .map(|f| {
                assert_eq!(f.start, 0);
                assert_eq!(f.end, u32::MAX);
                (f.tag.to_be_bytes(), f.value)
            })
            .collect()
    }

    #[test]
    fn item_feature_order_preserves_orientation_width_and_author_overrides() {
        use super::super::orientation::RunOrientation::*;
        let style = InlineStyle {
            font_kerning: FontKerning::None,
            font_variant_ligatures: FontVariantLigatures {
                common: Some(false),
                ..Default::default()
            },
            letter_spacing: 1.0,
            font_features: vec![
                FontFeature {
                    tag: *b"vert",
                    value: 0,
                },
                FontFeature {
                    tag: *b"vrt2",
                    value: 1,
                },
                FontFeature {
                    tag: *b"hwid",
                    value: 0,
                },
                FontFeature {
                    tag: *b"kern",
                    value: 1,
                },
                FontFeature {
                    tag: *b"liga",
                    value: 1,
                },
                FontFeature {
                    tag: *b"vert",
                    value: 1,
                },
            ],
            ..Default::default()
        };
        let tail = vec![
            (*b"kern", 0),
            (*b"liga", 0),
            (*b"clig", 0),
            (*b"liga", 0),
            (*b"clig", 0),
            (*b"dlig", 0),
            (*b"hlig", 0),
            (*b"vert", 0),
            (*b"vrt2", 1),
            (*b"hwid", 0),
            (*b"kern", 1),
            (*b"liga", 1),
            (*b"vert", 1),
        ];
        for orientation in [
            Horizontal,
            Upright,
            Combined,
            SidewaysClockwise,
            SidewaysCounterClockwise,
        ] {
            for width in [None, Some(*b"hwid"), Some(*b"twid"), Some(*b"qwid")] {
                let mut expected = Vec::new();
                if let Some(tag) = width {
                    expected.push((tag, 1));
                }
                if orientation == Upright {
                    expected.extend([(*b"vert", 1), (*b"vrt2", 0), (*b"vkrn", 0)]);
                }
                expected.extend_from_slice(&tail);
                assert_eq!(
                    item_features(&style, orientation, width),
                    expected,
                    "{orientation:?}/{width:?}"
                );
            }
        }
    }

    #[test]
    fn upright_defaults_preserve_vrt2_and_vertical_kerning_choices() {
        use super::super::orientation::RunOrientation::*;
        assert!(item_features(&InlineStyle::default(), Horizontal, None).is_empty());
        assert!(item_features(&InlineStyle::default(), Combined, None).is_empty());
        assert_eq!(
            item_features(&InlineStyle::default(), Upright, None),
            [(*b"vert", 1), (*b"vrt2", 0)]
        );
        let style = InlineStyle {
            font_kerning: FontKerning::Normal,
            font_features: vec![FontFeature {
                tag: *b"vrt2",
                value: 1,
            }],
            ..Default::default()
        };
        assert_eq!(
            item_features(&style, Upright, None),
            [
                (*b"vert", 0),
                (*b"vrt2", 0),
                (*b"vkrn", 1),
                (*b"kern", 1),
                (*b"vrt2", 1)
            ]
        );
    }

    fn value(style: &InlineStyle, tag: &[u8; 4]) -> Option<u32> {
        features(style)
            .iter()
            .rev()
            .find(|f| f.tag == harfrust::Tag::new(tag))
            .map(|f| f.value)
    }

    #[test]
    fn variants_map_to_features_and_explicit_settings_win() {
        let style = InlineStyle {
            font_variant_ligatures: FontVariantLigatures {
                common: Some(false),
                discretionary: Some(true),
                contextual: Some(false),
                ..Default::default()
            },
            font_variant_caps: FontVariantCaps::AllSmallCaps,
            font_variant_numeric: FontVariantNumeric {
                oldstyle_nums: true,
                tabular_nums: true,
                diagonal_fractions: true,
                ordinal: true,
                slashed_zero: true,
                ..Default::default()
            },
            font_variant_east_asian: FontVariantEastAsian {
                variant: Some(FontVariantEastAsianVariant::Jis04),
                width: Some(FontVariantEastAsianWidth::ProportionalWidth),
                ruby: true,
            },
            font_variant_position: FontVariantPosition::Super,
            font_variant_alternates: FontVariantAlternates {
                historical_forms: true,
                stylistic: Some(2),
                styleset: vec![1, 20],
                character_variant: vec![(3, 2)],
                swash: Some(4),
                ornaments: Some(5),
                annotation: Some(6),
            },
            font_features: vec![
                FontFeature {
                    tag: *b"liga",
                    value: 1,
                },
                FontFeature {
                    tag: *b"smcp",
                    value: 0,
                },
            ],
            ..Default::default()
        };
        for (tag, want) in [
            (b"liga", 1),
            (b"clig", 0),
            (b"dlig", 1),
            (b"calt", 0),
            (b"smcp", 0),
            (b"c2sc", 1),
            (b"onum", 1),
            (b"tnum", 1),
            (b"frac", 1),
            (b"ordn", 1),
            (b"zero", 1),
            (b"jp04", 1),
            (b"pwid", 1),
            (b"ruby", 1),
            (b"sups", 1),
            (b"hist", 1),
            (b"salt", 2),
            (b"ss01", 1),
            (b"ss20", 1),
            (b"cv03", 2),
            (b"swsh", 4),
            (b"cswh", 4),
            (b"ornm", 5),
            (b"nalt", 6),
        ] {
            assert_eq!(value(&style, tag), Some(want), "{tag:?}");
        }
    }

    #[test]
    fn caps_numeric_and_east_asian_dispatch_all_modes() {
        for (caps, tags) in [
            (FontVariantCaps::SmallCaps, vec![*b"smcp"]),
            (FontVariantCaps::PetiteCaps, vec![*b"pcap"]),
            (FontVariantCaps::AllPetiteCaps, vec![*b"pcap", *b"c2pc"]),
            (FontVariantCaps::Unicase, vec![*b"unic"]),
            (FontVariantCaps::TitlingCaps, vec![*b"titl"]),
        ] {
            let s = InlineStyle {
                font_variant_caps: caps,
                ..Default::default()
            };
            for tag in tags {
                assert_eq!(value(&s, &tag), Some(1));
            }
        }
        for (variant, tag) in [
            (FontVariantEastAsianVariant::Jis78, *b"jp78"),
            (FontVariantEastAsianVariant::Jis83, *b"jp83"),
            (FontVariantEastAsianVariant::Jis90, *b"jp90"),
            (FontVariantEastAsianVariant::Simplified, *b"smpl"),
            (FontVariantEastAsianVariant::Traditional, *b"trad"),
        ] {
            let s = InlineStyle {
                font_variant_east_asian: FontVariantEastAsian {
                    variant: Some(variant),
                    width: Some(FontVariantEastAsianWidth::FullWidth),
                    ..Default::default()
                },
                ..Default::default()
            };
            assert_eq!(value(&s, &tag), Some(1));
            assert_eq!(value(&s, b"fwid"), Some(1));
        }
        let s = InlineStyle {
            font_variant_numeric: FontVariantNumeric {
                lining_nums: true,
                proportional_nums: true,
                stacked_fractions: true,
                ..Default::default()
            },
            font_variant_position: FontVariantPosition::Sub,
            ..Default::default()
        };
        for tag in [b"lnum", b"pnum", b"afrc", b"subs"] {
            assert_eq!(value(&s, tag), Some(1));
        }
    }

    #[test]
    fn ligatures_none_spacing_and_kerning_are_tailored() {
        let s = InlineStyle {
            font_variant_ligatures: FontVariantLigatures {
                none: true,
                ..Default::default()
            },
            font_kerning: FontKerning::None,
            ..Default::default()
        };
        for tag in [b"liga", b"clig", b"dlig", b"hlig", b"calt", b"kern"] {
            assert_eq!(value(&s, tag), Some(0));
        }
        let s = InlineStyle {
            letter_spacing: 2.0,
            ..Default::default()
        };
        assert_eq!(value(&s, b"liga"), Some(0));
        let s = InlineStyle {
            font_features: vec![FontFeature {
                tag: *b"liga",
                value: 1,
            }],
            ..s
        };
        assert_eq!(value(&s, b"liga"), Some(1));
    }
}
