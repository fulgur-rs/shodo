//! Computed style values. Lengths are resolved px; percentages and `em` are
//! resolved by the caller, except values that depend on the font actually
//! used (for example `line-height: normal`), which shodo resolves.

use crate::geometry::{Direction, WritingMode};

#[derive(Clone, Debug, PartialEq)]
pub enum FontFamily {
    Named(String),
    Generic(GenericFamily),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
    SystemUi,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    /// Oblique angle in degrees.
    Oblique(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontVariation {
    pub tag: [u8; 4],
    pub value: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontFeature {
    pub tag: [u8; 4],
    pub value: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontKerning {
    #[default]
    Auto,
    Normal,
    None,
}

/// Resolved `font-variant-ligatures` components; `None` leaves that
/// component at the font's default. `none` disables all optional ligatures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FontVariantLigatures {
    pub none: bool,
    pub common: Option<bool>,
    pub discretionary: Option<bool>,
    pub historical: Option<bool>,
    pub contextual: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontVariantCaps {
    #[default]
    Normal,
    SmallCaps,
    AllSmallCaps,
    PetiteCaps,
    AllPetiteCaps,
    Unicase,
    TitlingCaps,
}

/// Computed numeric components. Mutually exclusive CSS keywords must be
/// resolved by the caller before constructing the style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FontVariantNumeric {
    pub lining_nums: bool,
    pub oldstyle_nums: bool,
    pub proportional_nums: bool,
    pub tabular_nums: bool,
    pub diagonal_fractions: bool,
    pub stacked_fractions: bool,
    pub ordinal: bool,
    pub slashed_zero: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontVariantEastAsianVariant {
    Jis78,
    Jis83,
    Jis90,
    Jis04,
    Simplified,
    Traditional,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontVariantEastAsianWidth {
    FullWidth,
    ProportionalWidth,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FontVariantEastAsian {
    pub variant: Option<FontVariantEastAsianVariant>,
    pub width: Option<FontVariantEastAsianWidth>,
    pub ruby: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontVariantPosition {
    #[default]
    Normal,
    Sub,
    Super,
}
/// Numeric alternates after the caller resolves CSS `@font-feature-values`.
/// Selectors apply directly to the chosen font, like `font_features`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontVariantAlternates {
    pub historical_forms: bool,
    pub stylistic: Option<u32>,
    /// OpenType ss01–ss20; values outside that range are ignored.
    pub styleset: Vec<u8>,
    /// OpenType cv01–cv99 and the selected alternate number.
    pub character_variant: Vec<(u8, u32)>,
    pub swash: Option<u32>,
    pub ornaments: Option<u32>,
    pub annotation: Option<u32>,
}

/// `font-synthesis`: which faces may be synthesized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontSynthesis {
    pub weight: bool,
    pub style: bool,
    pub small_caps: bool,
}

impl Default for FontSynthesis {
    fn default() -> Self {
        Self {
            weight: true,
            style: true,
            small_caps: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontMetricKind {
    ExHeight,
    CapHeight,
    ChWidth,
    IcWidth,
    IcHeight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontSizeAdjust {
    pub metric: FontMetricKind,
    pub value: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LineHeight {
    /// Resolved from the metrics of the font actually used.
    #[default]
    Normal,
    Px(f32),
    /// Multiple of the element's font size.
    Number(f32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WhiteSpaceCollapse {
    #[default]
    Collapse,
    Preserve,
    PreserveBreaks,
    PreserveSpaces,
    BreakSpaces,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWrapMode {
    #[default]
    Wrap,
    NoWrap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineBreak {
    #[default]
    Auto,
    Loose,
    Normal,
    Strict,
    Anywhere,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WordBreak {
    #[default]
    Normal,
    BreakAll,
    KeepAll,
    AutoPhrase,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverflowWrap {
    #[default]
    Normal,
    BreakWord,
    Anywhere,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hyphens {
    None,
    #[default]
    Manual,
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextTransform {
    #[default]
    None,
    Capitalize,
    Uppercase,
    Lowercase,
    FullWidth,
    FullSizeKana,
    CapitalizeFullWidth,
    UppercaseFullWidth,
    LowercaseFullWidth,
    CapitalizeFullSizeKana,
    UppercaseFullSizeKana,
    LowercaseFullSizeKana,
    FullWidthFullSizeKana,
    CapitalizeFullWidthFullSizeKana,
    UppercaseFullWidthFullSizeKana,
    LowercaseFullWidthFullSizeKana,
}

pub(crate) enum CaseTransform {
    None,
    Capitalize,
    Uppercase,
    Lowercase,
}

impl TextTransform {
    pub(crate) fn components(self) -> (CaseTransform, bool, bool) {
        use CaseTransform as C;
        use TextTransform::*;
        match self {
            None => (C::None, false, false),
            Capitalize => (C::Capitalize, false, false),
            Uppercase => (C::Uppercase, false, false),
            Lowercase => (C::Lowercase, false, false),
            FullWidth => (C::None, true, false),
            FullSizeKana => (C::None, false, true),
            CapitalizeFullWidth => (C::Capitalize, true, false),
            UppercaseFullWidth => (C::Uppercase, true, false),
            LowercaseFullWidth => (C::Lowercase, true, false),
            CapitalizeFullSizeKana => (C::Capitalize, false, true),
            UppercaseFullSizeKana => (C::Uppercase, false, true),
            LowercaseFullSizeKana => (C::Lowercase, false, true),
            FullWidthFullSizeKana => (C::None, true, true),
            CapitalizeFullWidthFullSizeKana => (C::Capitalize, true, true),
            UppercaseFullWidthFullSizeKana => (C::Uppercase, true, true),
            LowercaseFullWidthFullSizeKana => (C::Lowercase, true, true),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabSize {
    /// Multiple of the advance of the space character.
    Spaces(f32),
    Px(f32),
}

impl Default for TabSize {
    fn default() -> Self {
        Self::Spaces(8.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAutospace {
    #[default]
    Normal,
    NoAutospace,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextSpacingTrim {
    #[default]
    Normal,
    SpaceAll,
    TrimStart,
    SpaceFirst,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Sub,
    Super,
    TextTop,
    TextBottom,
    Middle,
    Top,
    Bottom,
    Length(f32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnicodeBidi {
    #[default]
    Normal,
    Embed,
    Isolate,
    BidiOverride,
    IsolateOverride,
    Plaintext,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextOrientation {
    #[default]
    Mixed,
    Upright,
    Sideways,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextCombineUpright {
    #[default]
    None,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEmphasisShape {
    Dot,
    Circle,
    DoubleCircle,
    Triangle,
    Sesame,
    Custom(char),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextEmphasisPosition {
    #[default]
    OverRight,
    UnderRight,
    OverLeft,
    UnderLeft,
}

/// `text-emphasis`; affects the line box height, so it is layout input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextEmphasis {
    pub shape: TextEmphasisShape,
    pub filled: bool,
    pub position: TextEmphasisPosition,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextBoxEdge {
    #[default]
    Auto,
    Text,
    Ideographic,
    Alphabetic,
    Cap,
    Ex,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxDecorationBreak {
    #[default]
    Slice,
    Clone,
}

/// Per-element style that affects shaping and line breaking.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineStyle {
    pub font_families: Vec<FontFamily>,
    pub font_size: f32,
    pub font_weight: f32,
    /// `font-width` as a percentage (100 = normal).
    pub font_width: f32,
    pub font_style: FontStyle,
    pub font_variations: Vec<FontVariation>,
    pub font_features: Vec<FontFeature>,
    pub font_kerning: FontKerning,
    pub font_variant_ligatures: FontVariantLigatures,
    pub font_variant_caps: FontVariantCaps,
    pub font_variant_numeric: FontVariantNumeric,
    pub font_variant_east_asian: FontVariantEastAsian,
    pub font_variant_position: FontVariantPosition,
    pub font_variant_alternates: FontVariantAlternates,
    pub font_optical_sizing: bool,
    pub font_synthesis: FontSynthesis,
    pub font_size_adjust: Option<FontSizeAdjust>,
    /// BCP 47 language tag.
    pub lang: Option<String>,
    pub line_height: LineHeight,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub white_space_collapse: WhiteSpaceCollapse,
    pub text_wrap_mode: TextWrapMode,
    pub line_break: LineBreak,
    pub word_break: WordBreak,
    pub overflow_wrap: OverflowWrap,
    pub hyphens: Hyphens,
    pub hyphenate_character: Option<String>,
    pub text_transform: TextTransform,
    pub tab_size: TabSize,
    pub text_autospace: TextAutospace,
    pub text_spacing_trim: TextSpacingTrim,
    pub vertical_align: VerticalAlign,
    pub direction: Direction,
    pub unicode_bidi: UnicodeBidi,
    pub text_orientation: TextOrientation,
    pub text_combine_upright: TextCombineUpright,
    pub text_emphasis: Option<TextEmphasis>,
    pub text_box_edge: TextBoxEdge,
    pub box_decoration_break: BoxDecorationBreak,
}

impl Default for InlineStyle {
    fn default() -> Self {
        Self {
            font_families: vec![FontFamily::Generic(GenericFamily::SansSerif)],
            font_size: 16.0,
            font_weight: 400.0,
            font_width: 100.0,
            font_style: FontStyle::default(),
            font_variations: Vec::new(),
            font_features: Vec::new(),
            font_kerning: FontKerning::default(),
            font_variant_ligatures: Default::default(),
            font_variant_caps: Default::default(),
            font_variant_numeric: Default::default(),
            font_variant_east_asian: Default::default(),
            font_variant_position: Default::default(),
            font_variant_alternates: Default::default(),
            font_optical_sizing: true,
            font_synthesis: FontSynthesis::default(),
            font_size_adjust: None,
            lang: None,
            line_height: LineHeight::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            white_space_collapse: WhiteSpaceCollapse::default(),
            text_wrap_mode: TextWrapMode::default(),
            line_break: LineBreak::default(),
            word_break: WordBreak::default(),
            overflow_wrap: OverflowWrap::default(),
            hyphens: Hyphens::default(),
            hyphenate_character: None,
            text_transform: TextTransform::default(),
            tab_size: TabSize::default(),
            text_autospace: TextAutospace::default(),
            text_spacing_trim: TextSpacingTrim::default(),
            vertical_align: VerticalAlign::default(),
            direction: Direction::default(),
            unicode_bidi: UnicodeBidi::default(),
            text_orientation: TextOrientation::default(),
            text_combine_upright: TextCombineUpright::default(),
            text_emphasis: None,
            text_box_edge: TextBoxEdge::default(),
            box_decoration_break: BoxDecorationBreak::default(),
        }
    }
}

/// Style of the block container. Changing it requires a rebuild.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParagraphStyle {
    pub writing_mode: WritingMode,
    pub direction: Direction,
    /// `unicode-bidi: plaintext` on the block container.
    pub unicode_bidi_plaintext: bool,
    /// Style of the root inline box; also the source of the strut.
    pub root: InlineStyle,
    /// Style applied by `::first-line`, if any.
    pub first_line: Option<InlineStyle>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
    JustifyAll,
    /// Unresolved parent alignment is treated as `Start`; resolve the
    /// parent's computed value before passing it for full CSS behavior.
    MatchParent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlignLast {
    #[default]
    Auto,
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextJustify {
    #[default]
    Auto,
    None,
    InterWord,
    InterCharacter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextIndent {
    pub length: f32,
    pub hanging: bool,
    pub each_line: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HangingPunctuation {
    pub first: bool,
    pub force_end: bool,
    pub allow_end: bool,
    pub last: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWrapStyle {
    #[default]
    Auto,
    Balance,
    Pretty,
    Stable,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextBoxTrim {
    #[default]
    None,
    TrimStart,
    TrimEnd,
    TrimBoth,
}

/// Options that only affect line layout; changing them needs no rebuild.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineOptions {
    pub text_align: TextAlign,
    pub text_align_last: TextAlignLast,
    pub text_justify: TextJustify,
    pub text_indent: TextIndent,
    pub hanging_punctuation: HangingPunctuation,
    pub text_wrap_style: TextWrapStyle,
    pub text_box_trim: TextBoxTrim,
}
