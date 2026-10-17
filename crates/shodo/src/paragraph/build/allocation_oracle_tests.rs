//! Checks the analysis byte model against measured allocations.
//!
//! Each phase reserves its retained payload plus an upper bound on dependency
//! scratch (`analysis_budget`); transform metadata is reserved in the mapping
//! budget. These tests run the matching producer on inputs
//! that drive its worst-case paths and assert that the real peak heap stays
//! within the reserved bytes, so a unicode-bidi or ICU update that grows scratch
//! beyond the model fails CI instead of silently weakening the fail-closed limit.
use super::*;
use crate::node::TextSource;
use crate::style::TextTransform;
use crate::test_support::alloc::measure_peak;

/// Input text is repeated to about this many bytes, so fixed headers are noise.
const TARGET_BYTES: usize = 32 * 1024;

struct Oracle {
    transform: (u64, u64),
    analysis: (u64, u64),
}

fn run(case: &str, unit: &str, root: InlineStyle, mode: WritingMode) -> Oracle {
    let text = unit.repeat(TARGET_BYTES / unit.len() + 1);
    let style = ParagraphStyle {
        root,
        writing_mode: mode,
        ..Default::default()
    };
    // Unlimited: charges are still computed but never reject the large inputs.
    let limits = Limits::unlimited();
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, &text);
    let ParagraphBuilder {
        style,
        text,
        items,
        styles,
        ..
    } = builder;
    let mut bases = crate::limits::ledger::LimitsLedger::new(&limits);
    let mut warnings = WarningSink::new(None);
    let processed =
        crate::analysis::whitespace::process(&text, &items, &styles, false, &limits).unwrap();
    let mut budget = analysis_cost::Cost::new();

    let before = budget.used;
    budget
        .transform(&processed, &styles, mode, &mut bases)
        .unwrap();
    // Transform spans and width origins are charged to the mapping budget.
    let mapping_before = processed.mapping_cost.used;
    let (processed, transform_peak) = measure_peak(|| {
        transform_input(processed, &styles, &limits, &mut warnings, mode, &mut bases).unwrap()
    });
    let transform_charge = budget.used - before + (processed.mapping_cost.used - mapping_before);

    let before = budget.used;
    budget
        .analysis(&processed, &styles, &style, false, &mut bases)
        .unwrap();
    let analysis_charge = budget.used - before;
    let analysis_limits = limits.clone();
    let (prepared, analysis_peak) = measure_peak(|| {
        analyze_data(
            style,
            analysis_limits,
            processed,
            styles,
            1,
            &mut warnings,
            true,
            None,
            budget,
        )
    });
    drop(prepared);
    // Analysis always allocates; zero would mean the counter saw nothing and
    // every bound below would pass vacuously.
    assert!(
        analysis_peak > 0,
        "{case}: counting allocator observed no analysis heap"
    );

    for (phase, peak, charge) in [
        ("transform", transform_peak, transform_charge),
        ("analysis", analysis_peak, analysis_charge),
    ] {
        // Visible with --nocapture: the headroom left for dependency updates.
        println!(
            "{case} {phase}: peak {peak} / charged {charge} = {:.3}",
            peak as f64 / charge.max(1) as f64
        );
        assert!(
            peak <= charge,
            "{case} {phase}: peak heap {peak} exceeds the {charge} bytes the analysis \
             model reserved; re-audit analysis_budget for the updated dependency"
        );
    }
    Oracle {
        transform: (transform_peak, transform_charge),
        analysis: (analysis_peak, analysis_charge),
    }
}

fn lang(tag: &str) -> InlineStyle {
    InlineStyle {
        lang: Some(tag.into()),
        ..Default::default()
    }
}

#[test]
fn bidi_brackets_and_isolates_stay_within_the_analysis_model() {
    // Dense bracket pairs and nested isolates over mixed Hebrew/Arabic/Latin.
    let unit = "ab (שלום [مرحبا {x} עולם]) \u{2067}עברית \u{2066}latin (x)\u{2069}\u{2069} ";
    let oracle = run(
        "bidi",
        unit,
        InlineStyle::default(),
        WritingMode::HorizontalTb,
    );
    assert!(oracle.analysis.0 > 0, "bidi analysis allocated nothing");
}

#[test]
fn complex_context_runs_stay_within_the_analysis_model() {
    // Thai and Khmer are LineBreak=SA: the auto LSTM path with complex-scripts.
    let thai = "ภาษาไทยเป็นภาษาที่ไม่มีการเว้นวรรคระหว่างคำในประโยค";
    run("thai", thai, lang("th"), WritingMode::HorizontalTb);
    let khmer = "ភាសាខ្មែរមិនដកឃ្លារវាងពាក្យទេ";
    run("khmer", khmer, lang("km"), WritingMode::HorizontalTb);
}

#[test]
fn cjk_runs_stay_within_the_analysis_model() {
    let japanese = "日本語の文章は単語の間に空白を入れないので辞書で区切る";
    run("japanese", japanese, lang("ja"), WritingMode::HorizontalTb);
    run(
        "japanese-vertical",
        japanese,
        lang("ja"),
        WritingMode::VerticalRl,
    );
    let chinese = "中文句子中词语之间没有空格需要使用词典进行分词";
    run("chinese", chinese, lang("zh"), WritingMode::HorizontalTb);
}

#[test]
fn case_transforms_stay_within_the_transform_model() {
    let capitalize = |tag: &str| InlineStyle {
        text_transform: TextTransform::Capitalize,
        ..lang(tag)
    };
    let uppercase = |tag: &str| InlineStyle {
        text_transform: TextTransform::Uppercase,
        ..lang(tag)
    };
    // WordSegmenter scratch for word starts in scripts without spaces.
    let thai = run(
        "capitalize-thai",
        "ภาษาไทยเป็นภาษาที่ไม่มีการเว้นวรรค abc ",
        capitalize("th"),
        WritingMode::HorizontalTb,
    );
    assert!(thai.transform.0 > 0, "capitalize allocated nothing");
    run(
        "capitalize-japanese",
        "日本語の文章 word 単語の間 ",
        capitalize("ja"),
        WritingMode::HorizontalTb,
    );
    // Expansions and language-sensitive mappings.
    run(
        "uppercase-german",
        "straße ß ŉ ﬃ ",
        uppercase("de"),
        WritingMode::HorizontalTb,
    );
    run(
        "uppercase-turkish",
        "istanbul ıi İI ",
        uppercase("tr"),
        WritingMode::HorizontalTb,
    );
    run(
        "lowercase-greek",
        "ΟΔΟΣ ΣΟΦΟΣ Σ ",
        InlineStyle {
            text_transform: TextTransform::Lowercase,
            ..lang("el")
        },
        WritingMode::HorizontalTb,
    );
}
