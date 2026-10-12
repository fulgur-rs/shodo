use super::*;
use crate::node::TextSource;

struct ReferenceGuard(bool);

impl ReferenceGuard {
    fn new(reference: bool) -> Self {
        Self(paint_reuse::FORCE_REFERENCE.with(|flag| flag.replace(reference)))
    }
}

impl Drop for ReferenceGuard {
    fn drop(&mut self) {
        paint_reuse::FORCE_REFERENCE.with(|flag| flag.set(self.0));
    }
}

fn input(style: &ParagraphStyle, limits: &Limits) -> ParagraphBuilder {
    let mut builder = ParagraphBuilder::new(style, limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 7,
        },
        "of",
    );
    let mut child = style.root.clone();
    child.font_weight = 600.0;
    let mut first = child.clone();
    first.paint.color = [255, 0, 0, 255];
    builder.open_inline_with_first_line(NodeId(2), &child, &first, Default::default());
    builder.push_text(
        TextSource::Dom {
            node: NodeId(3),
            offset: 4,
        },
        "fice  Straße\nＡ ｶﾞ مرحبا",
    );
    builder.close_inline();
    builder
}

fn prepared_snapshot(data: &PreparedBuild) -> String {
    format!(
        "{:?}",
        (
            (&data.style, &data.limits, data.glyph_budget, &data.styles),
            &data.processed.text,
            &data.processed.items,
            &data.processed.mapping,
            &data.processed.indivisible,
            data.processed
                .source_spans
                .iter()
                .map(|s| (&s.old, &s.new, s.kind))
                .collect::<Vec<_>>(),
            data.processed
                .width_origins
                .iter()
                .map(|s| (&s.text, &s.before_width))
                .collect::<Vec<_>>(),
            (
                &data.breaks.graphemes,
                &data.breaks.typographic_starts,
                &data.breaks.authored_bidi_controls,
                &data.breaks.caret_cuts,
                &data.breaks.opportunities
            ),
            (
                &data.bidi.levels,
                data.bidi
                    .paragraphs
                    .iter()
                    .map(|p| (&p.text, p.base_level, p.inline_level))
                    .collect::<Vec<_>>()
            ),
            (&data.combine_spans, data.used_direction),
        )
    )
}

fn analysis_snapshot(
    result: Result<ParagraphAnalysis, LimitExceeded>,
) -> Result<String, LimitExceeded> {
    result.map(|analysis| {
        let state = analysis.state;
        format!(
            "{:?}",
            (
                prepared_snapshot(&state.normal),
                state.alternate.as_ref().map(prepared_snapshot),
                state.source_cuts,
                state.warnings,
                state.alternate_warnings,
            )
        )
    })
}

fn paint_style() -> ParagraphStyle {
    let mut style = ParagraphStyle::default();
    let mut first = style.root.clone();
    first.paint.color = [0, 0, 255, 255];
    style.first_line = Some(first);
    style
}

#[test]
fn paint_only_analysis_uses_one_pass() {
    let style = paint_style();
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.with_offset_mapping(true);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "office affinity  first\nline",
    );
    ANALYSIS_PASSES.with(|count| count.set(0));
    let _analysis = builder.analyze().unwrap();
    assert_eq!(
        ANALYSIS_PASSES.with(std::cell::Cell::get),
        1,
        "paint-only first-line must reuse normal analysis"
    );
}

#[test]
fn reused_analysis_matches_reference_for_identical_transforms_and_style_maps() {
    for transformed in [false, true] {
        for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl] {
            let mut style = paint_style();
            style.writing_mode = mode;
            if transformed {
                style.root.text_transform = crate::style::TextTransform::UppercaseFullWidth;
                style.first_line.as_mut().unwrap().text_transform = style.root.text_transform;
            }
            ANALYSIS_PASSES.with(|count| count.set(0));
            let candidate = analysis_snapshot(input(&style, &Limits::default()).analyze());
            assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 1);
            let reference = {
                let _guard = ReferenceGuard::new(true);
                ANALYSIS_PASSES.with(|count| count.set(0));
                analysis_snapshot(input(&style, &Limits::default()).analyze())
            };
            assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 2);
            assert_eq!(candidate, reference);
        }
    }
}

#[test]
fn analysis_fallback_preserves_callbacks_warnings_and_limit_errors() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let style = paint_style();
    let run_callback = |reference| {
        let _guard = ReferenceGuard::new(reference);
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut builder = input(&style, &Limits::default());
        builder.with_line_break_override(move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
            crate::LineBreakOverride::Allow
        });
        ANALYSIS_PASSES.with(|count| count.set(0));
        let snapshot = analysis_snapshot(builder.analyze()).unwrap();
        assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 2);
        (snapshot, calls.load(Ordering::Relaxed))
    };
    let callback = run_callback(false);
    assert!(callback.1 > 0);
    assert_eq!(callback, run_callback(true));

    for cap in [0, 1, 8] {
        let limits = Limits {
            max_warnings: Some(cap),
            ..Limits::default()
        };
        let mut invalid = style.clone();
        invalid.root.font_size = f32::NAN;
        invalid.first_line.as_mut().unwrap().font_size = f32::NAN;
        ANALYSIS_PASSES.with(|count| count.set(0));
        let candidate = analysis_snapshot(input(&invalid, &limits).analyze()).unwrap();
        assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 2);
        let reference = {
            let _guard = ReferenceGuard::new(true);
            analysis_snapshot(input(&invalid, &limits).analyze()).unwrap()
        };
        assert_eq!(candidate, reference);
    }

    for cap in 0..120 {
        for text_cap in [false, true] {
            let mut limits = Limits::default();
            if text_cap {
                limits.max_text_bytes = Some(cap);
            } else {
                limits.max_items = Some(cap);
            }
            let candidate = analysis_snapshot(input(&style, &limits).analyze());
            let reference = {
                let _guard = ReferenceGuard::new(true);
                analysis_snapshot(input(&style, &limits).analyze())
            };
            assert_eq!(candidate, reference, "cap={cap}, text={text_cap}");
        }
    }
}
