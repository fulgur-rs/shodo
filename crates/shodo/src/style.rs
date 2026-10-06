//! Computed style values. Lengths are resolved px; percentages and `em` are
//! resolved by the caller, except values that depend on the font actually
//! used (for example `line-height: normal` or a `word-spacing` percentage),
//! which shodo resolves.

use crate::geometry::{Direction, WritingMode};

pub(crate) mod memory;

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
    Manual,
    AutoPhrase,
    /// Legacy keyword equivalent to `Normal` plus `OverflowWrap::Anywhere`.
    BreakWord,
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

/// CSS Text 4 `word-space-transform`.
///
/// The `auto-phrase` variants preserve the CSS value and transform explicit
/// zero-width-space opportunities, but automatic phrase segmentation is not
/// currently available and produces an [`Unsupported`](crate::limits::WarningKind::Unsupported)
/// warning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WordSpaceTransform {
    #[default]
    None,
    Space,
    IdeographicSpace,
    SpaceAutoPhrase,
    IdeographicSpaceAutoPhrase,
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
/// Character-class-based spacing, applied after bidi reordering without
/// modifying paragraph text or its source mapping.
///
/// All values use CSS `insert` behavior: any existing Unicode separator
/// suppresses automatic spacing. Replacing authored spaces is not supported.
pub enum TextAutospace {
    /// Insert 0.125ic between ideographs and non-ideographic letters/numerals.
    #[default]
    Normal,
    /// Platform policy; currently uses the same spacing as [`Self::Normal`].
    Auto,
    /// Disable automatic spacing.
    NoAutospace,
    /// Select independent boundary classes. An empty set disables spacing.
    Custom {
        /// Space ideograph/non-ideographic letter boundaries.
        ideograph_alpha: bool,
        /// Space ideograph/non-ideographic decimal numeral boundaries.
        ideograph_numeric: bool,
        /// Apply French non-breaking punctuation spacing when the boundary's
        /// innermost containing element has a French content language.
        punctuation: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextSpacingTrim {
    /// Collapse adjacent punctuation blanks; trim a closing line end only
    /// when it would not otherwise fit before justification.
    #[default]
    Normal,
    /// Preserve the font's fullwidth punctuation spacing everywhere.
    SpaceAll,
    /// Trim opening punctuation at every line start; otherwise use Normal.
    TrimStart,
    /// Preserve opening spacing on first/forced heads, trim soft heads.
    SpaceFirst,
    /// Always trim opening line starts and closing line ends.
    TrimBoth,
    /// Trim punctuation blanks at every position, including middle dots.
    TrimAll,
    /// Deterministic high-quality policy: the same behavior as TrimBoth.
    Auto,
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

/// `text-emphasis`. Lines grow where emphasized text and its marks overflow
/// the line box, as in Chromium 152; see
/// [`crate::GlyphRunView::emphasis_mark`] for drawing them.
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

/// One resolved solid decoration. Unspecified values use the source color and
/// actual font instance's underline/strike-through metrics. Offsets are signed
/// px toward line-under from the alphabetic (upright: central) baseline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextDecoration {
    pub color: Option<[u8; 4]>,
    pub offset: Option<f32>,
    /// Nonnegative px. Zero suppresses painting.
    pub thickness: Option<f32>,
}

/// Resolved source paint, independent of shaping. Colors are non-premultiplied
/// sRGB RGBA8. CSS inheritance/decoration propagation is resolved by the caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintStyle {
    pub color: [u8; 4],
    pub underline: Option<TextDecoration>,
    pub strikethrough: Option<TextDecoration>,
}

impl Default for PaintStyle {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 255],
            underline: None,
            strikethrough: None,
        }
    }
}

/// Per-element resolved shaping, line-breaking and paint style.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineStyle {
    pub paint: PaintStyle,
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
    /// Absolute `word-spacing` term in px.
    pub word_spacing: f32,
    /// Percentage `word-spacing` term (25 = 25%), resolved against the
    /// U+0020 advance of the font selected for this style and added to
    /// [`Self::word_spacing`]. `calc(25% + 2px)` is `25.0` here and `2.0`
    /// there.
    pub word_spacing_percent: f32,
    pub white_space_collapse: WhiteSpaceCollapse,
    pub text_wrap_mode: TextWrapMode,
    pub line_break: LineBreak,
    pub word_break: WordBreak,
    pub overflow_wrap: OverflowWrap,
    pub hyphens: Hyphens,
    pub hyphenate_character: Option<String>,
    pub text_transform: TextTransform,
    pub word_space_transform: WordSpaceTransform,
    pub tab_size: TabSize,
    pub text_autospace: TextAutospace,
    pub text_spacing_trim: TextSpacingTrim,
    /// Computed `hanging-punctuation` of this inline box. `None` uses
    /// [`LineOptions::hanging_punctuation`]; `Some` replaces it for the
    /// characters of this box, so `Some(HangingPunctuation::default())` is
    /// CSS `none` even when the paragraph enables hanging. Each edge
    /// character uses the value of the box it belongs to.
    /// `ParagraphStyle::root.hanging_punctuation = Some(..)` therefore
    /// overrides `LineOptions` for root-level text. The value is ignored in
    /// [`ParagraphStyle::first_line`]: `hanging-punctuation` does not apply
    /// to `::first-line`.
    pub hanging_punctuation: Option<HangingPunctuation>,
    pub vertical_align: VerticalAlign,
    pub direction: Direction,
    pub unicode_bidi: UnicodeBidi,
    pub text_orientation: TextOrientation,
    pub text_combine_upright: TextCombineUpright,
    pub text_emphasis: Option<TextEmphasis>,
    pub text_box_edge: TextBoxEdge,
    pub box_decoration_break: BoxDecorationBreak,
}

impl InlineStyle {
    /// Used `word-spacing` in px, given the U+0020 advance of this style's
    /// selected font.
    pub(crate) fn used_word_spacing(&self, space: f32) -> f32 {
        self.word_spacing + self.word_spacing_percent / 100.0 * space
    }
}

impl Default for InlineStyle {
    fn default() -> Self {
        Self {
            paint: PaintStyle::default(),
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
            word_spacing_percent: 0.0,
            white_space_collapse: WhiteSpaceCollapse::default(),
            text_wrap_mode: TextWrapMode::default(),
            line_break: LineBreak::default(),
            word_break: WordBreak::default(),
            overflow_wrap: OverflowWrap::default(),
            hyphens: Hyphens::default(),
            hyphenate_character: None,
            text_transform: TextTransform::default(),
            word_space_transform: WordSpaceTransform::default(),
            tab_size: TabSize::default(),
            text_autospace: TextAutospace::default(),
            text_spacing_trim: TextSpacingTrim::default(),
            hanging_punctuation: None,
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
    /// Resolved root style for `::first-line`, if any. Clone `root` and
    /// change its first-line properties to express a partial override.
    /// Paint, font, language, line height, spacing, transform and emphasis values
    /// inherit into descendants whose normal value equals the root value;
    /// differing descendant values are preserved. Other properties retain
    /// their normal values.
    /// Use `ParagraphBuilder::open_inline_with_first_line` or
    /// `RichText::push_with_first_line` for caller-resolved descendant styles;
    /// those explicit inputs override the value-based fallback.
    pub first_line: Option<InlineStyle>,
    /// The line height calculation quirk of quirks and limited-quirks mode
    /// (Quirks Mode Standard §3.3-3.4; CSS Inline 3 §5.3). When set, on each
    /// line an inline box, the root inline box included, contributes its
    /// strut only if that line holds text it directly contains, its own
    /// inline-start or inline-end border or padding, a forced break with
    /// nothing else in the box on that line, or (root only) ruby. Its
    /// descendants still align to its metrics. As in Chromium, a cloned
    /// `box-decoration-break` edge repeated on a continuation line does not
    /// count, and margins never count.
    pub line_height_quirk: bool,
    /// Keep the root inline box's strut on every nonempty line, including
    /// continuations, when [`Self::line_height_quirk`] is enabled. Set this
    /// for CSS list-item paragraphs to match browser quirks-mode layout.
    /// Child inline boxes still follow the quirk's per-fragment rules.
    /// With the quirk disabled the root strut already contributes normally.
    pub force_root_strut: bool,
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

/// CSS `hanging-punctuation` flags; set them paragraph-wide via
/// [`LineOptions::hanging_punctuation`] or per box via
/// [`InlineStyle::hanging_punctuation`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HangingPunctuation {
    /// Hangs one eligible opening mark on the first formatted line.
    pub first: bool,
    /// Excludes a comma/stop's remaining advance even if it fits.
    pub force_end: bool,
    /// Excludes only the part of a comma/stop that exceeds the available width.
    pub allow_end: bool,
    /// Hangs one eligible ending mark on the final line.
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
    /// Paragraph-wide `hanging-punctuation`, used for characters whose
    /// [`InlineStyle::hanging_punctuation`] is `None`.
    pub hanging_punctuation: HangingPunctuation,
    pub text_wrap_style: TextWrapStyle,
    pub text_box_trim: TextBoxTrim,
}
