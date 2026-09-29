# Complex-script segmentation diagnostics

Investigation of shodo-0ce, using icu_segmenter 2.3.0 with the existing
`compiled_data` and `auto` features. Positions below are UTF-8 byte offsets.
No extra data provider or dependency feature is needed.

## Reproduction and cause

With `complex-scripts` enabled, `LineSegmenter::new_auto` already loads the
models. The warning reproduced in text transformation: `transform_context`
selected `WordSegmenter::new_for_non_complex_scripts` unconditionally. A plain
paragraph bypassed that pass; uppercasing or capitalizing the same uncased text
entered it and emitted `No segmentation model for complex script`.

Use `WordSegmenter::new_auto` when `complex-scripts` is enabled, matching the
line-analysis feature boundary. The non-complex constructor remains in use
when the feature is disabled.

## Measured results

For line segmentation, set `content_locale` to the indicated locale,
`strictness` to Normal, and `word_option` to Normal. For word segmentation,
use default options. Each cell contains every returned boundary, including
start/end. The non-complex results are a diagnostic control, not the previous
feature-enabled line-analysis behavior.

| Locale / text | Bytes | Line auto | Word auto | Word non-complex |
| --- | ---: | --- | --- | --- |
| ja / こんにちは世界 | 21 | [0,3,6,9,12,15,18,21] | [0,15,21] | [0,21] |
| zh / 中文文本 | 12 | [0,3,6,9,12] | [0,6,12] | [0,12] |
| km / ភាសាខ្មែរជាភាសាជាតិ | 57 | [0,12,27,33,57] | [0,12,27,33,57] | [0,57] |
| th / ภาษาไทย | 21 | [0,12,21] | [0,12,21] | [0,21] |

Auto constructors emitted zero model diagnostics for these samples.
Word non-complex emitted one per sample. Line non-complex emitted none for
Japanese/Chinese, and one each for Khmer/Thai; its Khmer/Thai boundaries were
[0,57] / [0,21]. Thus Khmer has three internal line opportunities with auto
versus zero without models. Japanese line opportunities do not require the
word dictionary in this sample. This fix changes word context, not line
segmenter configuration or these feature-enabled line boundaries.

For real `ParagraphBuilder` calls, Japanese/Khmer/Thai inputs with
`TextTransform::None` emitted zero model diagnostics before the fix.
`Uppercase` and `Capitalize` emitted one each: six total for six transformed
paragraphs. After the fix they emit zero; their uncased processed text stays
identical. Internal capitalization word heads change from [0] to [0,15],
[0,12,27,33], and [0,12] respectively.

## Regression commands and limits

```sh
cargo test --test complex_script_segmentation -- --nocapture
cargo test --test text_transform transformed_paragraphs_load_complex_models -- --nocapture
cargo test --lib context_word_heads_use_complex_models -- --nocapture
```

The subprocess regression isolates real paragraph construction and stderr.
ICU provider's debug fallback prints to stderr when its `logging` feature is
disabled. A normal passing test's captured output can hide diagnostics.
With `logging` enabled, visibility depends on the application's installed
logger; in release builds the default stderr fallback is compiled out.
The literal word-head regression also detects missing word models when
stderr is unavailable. Existing cased-script, source-split and mapping tests
cover transformation compatibility.

`complex-scripts` disabled intentionally retains the existing limited-model
behavior. These measurements establish the core cause and repair; they do not
claim that the entire raikiri WPT batch was rerun. Both integration spikes
remain separate. No missing core model data was reproduced, so this
investigation does not add a provider-data dependency to shodo-p2m.6.
