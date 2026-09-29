//! Characterization of the model availability assumption in shodo-0ce.
#![cfg(feature = "complex-scripts")]

use icu_segmenter::LineSegmenter;
use icu_segmenter::options::{LineBreakOptions, LineBreakStrictness, LineBreakWordOption};

#[test]
fn auto_line_models_with_content_locale() {
    // All expected positions are literal UTF-8 byte boundaries. Japanese
    // line breaks are character-based; Khmer must retain internal word cuts.
    for (lang, text, expected) in [
        ("ja", "こんにちは世界", &[0, 3, 6, 9, 12, 15, 18, 21][..]),
        ("km", "ភាសាខ្មែរជាភាសាជាតិ", &[0, 12, 27, 33, 57][..]),
    ] {
        let locale = lang.parse().unwrap();
        let mut options = LineBreakOptions::default();
        options.content_locale = Some(&locale);
        options.strictness = Some(LineBreakStrictness::Normal);
        options.word_option = Some(LineBreakWordOption::Normal);
        let cuts: Vec<_> = LineSegmenter::new_auto(options).segment_str(text).collect();
        assert_eq!(cuts, expected, "{lang}: {text:?}");
    }
}
