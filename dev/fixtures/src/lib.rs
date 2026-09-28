//! Fixed, development-only fonts and original sample cases.
//!
//! Fonts are renamed Noto subsets under OFL-1.1; see assets/licenses.
//! Source/checksum and reproducible subset metadata live in assets/manifest.json.

use std::sync::OnceLock;

use serde::{Deserialize, Deserializer};
use shodo::font::{FontCollection, FontError, FontFaceDescriptor, FontId, FontOptions};
use shodo::geometry::Direction;
use shodo::limits::{LimitExceeded, Limits};
use shodo::style::{FontFamily, GenericFamily, InlineStyle, ParagraphStyle};
use shodo::{LayoutContext, Paragraph, RichText};

/// A checked-in static face. Its bytes do not depend on the host filesystem.
#[derive(Clone, Copy, Debug)]
pub struct FontFixture {
    pub id: &'static str,
    pub family: &'static str,
    pub bytes: &'static [u8],
    pub face_index: u32,
    pub sha256: &'static str,
}

/// Latin, Japanese/CJK and Arabic faces in stable registration order.
pub const FONTS: &[FontFixture] = &[
    FontFixture {
        id: "latin",
        family: "Shodo Fixture Latin",
        bytes: include_bytes!("../assets/fonts/latin.ttf"),
        face_index: 0,
        sha256: "7aa5c6687e9a8b72f71ea5abaded28d771ecb54c66a8a9ac49268537b2f94d25",
    },
    FontFixture {
        id: "cjk",
        family: "Shodo Fixture CJK",
        bytes: include_bytes!("../assets/fonts/cjk.otf"),
        face_index: 0,
        sha256: "d8b52a1ddcb511adc93bce9b2caff2d7dcadba609c3bfe25ade99343b7a5aa8b",
    },
    FontFixture {
        id: "arabic",
        family: "Shodo Fixture Arabic",
        bytes: include_bytes!("../assets/fonts/arabic.ttf"),
        face_index: 0,
        sha256: "e9885fbcf3bdbddea94d1679d636a80f7421c6b63632a35c6f884a9ee74067d9",
    },
];

/// Optional real color and monochrome emoji faces, in stable registration order.
/// Both faces share a family so presentation selection can choose either.
pub const EMOJI_FONTS: &[FontFixture] = &[
    FontFixture {
        id: "emoji-color",
        family: "Shodo Fixture Emoji",
        bytes: include_bytes!("../assets/fonts/emoji-color.ttf"),
        face_index: 0,
        sha256: "6047984e1da3b32e9629740378f2a143daa2c630f140edcf69b68696e3766362",
    },
    FontFixture {
        id: "emoji-mono",
        family: "Shodo Fixture Emoji",
        bytes: include_bytes!("../assets/fonts/emoji-mono.ttf"),
        face_index: 0,
        sha256: "76c91d377b0d3acb5e0feb696924d1fec7ea9cd4c7860dff52aa0cdd998f76f3",
    },
];

/// Look up a font by stable ID, never by an installed family name.
pub fn font(id: &str) -> Option<&'static FontFixture> {
    FONTS.iter().chain(EMOJI_FONTS).find(|font| font.id == id)
}

/// A case shared with the subset generator and development tools.
#[derive(Debug, Deserialize)]
pub struct FixtureCase {
    pub id: String,
    pub text: String,
    pub font_ids: Vec<String>,
    pub lang: Option<String>,
    #[serde(deserialize_with = "direction")]
    pub direction: Direction,
    pub font_size: f32,
    pub width: f32,
}

fn direction<'de, D: Deserializer<'de>>(de: D) -> Result<Direction, D::Error> {
    match String::deserialize(de)?.as_str() {
        "ltr" => Ok(Direction::Ltr),
        "rtl" => Ok(Direction::Rtl),
        value => Err(serde::de::Error::custom(format!(
            "unknown direction {value}"
        ))),
    }
}

/// The original corpus, parsed once from the exact subsetting input.
pub fn cases() -> &'static [FixtureCase] {
    static CASES: OnceLock<Vec<FixtureCase>> = OnceLock::new();
    CASES.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/cases.json"))
            .expect("checked-in fixture corpus is valid")
    })
}

/// Original emoji corpus, also used to reproduce only the optional emoji subsets.
pub fn emoji_cases() -> &'static [FixtureCase] {
    static CASES: OnceLock<Vec<FixtureCase>> = OnceLock::new();
    CASES.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/emoji-cases.json"))
            .expect("checked-in emoji corpus is valid")
    })
}

/// Look up a case by stable ID.
pub fn case(id: &str) -> Option<&'static FixtureCase> {
    cases().iter().find(|case| case.id == id)
}

/// A collection loaded with only the fixed faces, plus their registration IDs.
#[derive(Debug)]
pub struct FixtureFonts {
    pub collection: FontCollection,
    /// Matches the order in FONTS; IDs are scoped to this collection.
    pub ids: [FontId; 3],
}

/// Load fixed bytes with system discovery disabled and deterministic fallback.
pub fn load_fonts(limits: &Limits) -> Result<FixtureFonts, FontError> {
    let collection = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let mut ids = Vec::with_capacity(FONTS.len());
    for font in FONTS {
        ids.push(collection.register_face(
            font.bytes.to_vec(),
            font.face_index,
            FontFaceDescriptor {
                family: font.family.into(),
                ..Default::default()
            },
        )?);
    }
    collection.set_generic_families(
        GenericFamily::SansSerif,
        FONTS.iter().map(|font| font.family.to_owned()).collect(),
    );
    for (script, font) in [
        (*b"Latn", &FONTS[0]),
        (*b"Hani", &FONTS[1]),
        (*b"Arab", &FONTS[2]),
    ] {
        collection.set_fallback_families(script, None, vec![font.family.into()]);
    }
    Ok(FixtureFonts {
        collection,
        ids: ids.try_into().expect("exactly three fixture fonts"),
    })
}

/// Opt-in emoji collection; the original faces keep their registration IDs.
#[derive(Debug)]
pub struct EmojiFixtureFonts {
    pub base: FixtureFonts,
    /// Matches EMOJI_FONTS (color, then mono).
    pub emoji_ids: [FontId; 2],
}

/// Load the original three faces plus real emoji, with no system discovery.
pub fn load_emoji_fonts(limits: &Limits) -> Result<EmojiFixtureFonts, FontError> {
    let base = load_fonts(limits)?;
    let mut ids = Vec::with_capacity(2);
    for font in EMOJI_FONTS {
        ids.push(base.collection.register_face(
            font.bytes.to_vec(),
            font.face_index,
            FontFaceDescriptor {
                family: font.family.into(),
                // Match the pinned mono fvar range instead of fixing wght400.
                weight: if font.id == "emoji-mono" {
                    (300.0, 700.0)
                } else {
                    (400.0, 400.0)
                },
                ..Default::default()
            },
        )?);
    }
    let family = EMOJI_FONTS[0].family;
    base.collection.set_generic_families(
        GenericFamily::SansSerif,
        FONTS
            .iter()
            .map(|font| font.family.to_owned())
            .chain(std::iter::once(family.to_owned()))
            .collect(),
    );
    for (script, primary) in [
        (*b"Latn", FONTS[0].family),
        (*b"Hani", FONTS[1].family),
        (*b"Arab", FONTS[2].family),
    ] {
        base.collection
            .set_fallback_families(script, None, vec![primary.into(), family.into()]);
    }
    base.collection
        .set_fallback_families(*b"Zyyy", None, vec![family.into()]);
    Ok(EmojiFixtureFonts {
        base,
        emoji_ids: ids.try_into().expect("exactly two emoji fonts"),
    })
}

impl FixtureCase {
    /// Build through the public paragraph API with the fixed case settings.
    /// Paragraph glyph IDs belong to the actual fixed fonts retained by each run.
    pub fn build(
        &self,
        cx: &mut LayoutContext,
        fonts: &FixtureFonts,
        limits: &Limits,
    ) -> Result<Paragraph, LimitExceeded> {
        let inline = InlineStyle {
            font_families: self
                .font_ids
                .iter()
                .map(|id| {
                    FontFamily::Named(
                        font(id)
                            .expect("case refers to a known fixture font")
                            .family
                            .into(),
                    )
                })
                .collect(),
            font_size: self.font_size,
            lang: self.lang.clone(),
            direction: self.direction,
            ..Default::default()
        };
        let paragraph = ParagraphStyle {
            direction: self.direction,
            root: inline.clone(),
            ..Default::default()
        };
        RichText::with_limits(&paragraph, limits)
            .push(&self.text, &inline)
            .build(cx, &fonts.collection)
    }
}

/// Development-only saved browser comparisons.
pub mod browser;
