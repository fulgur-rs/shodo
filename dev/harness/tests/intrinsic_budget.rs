use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, LineOptions, ParagraphStyle};
use shodo::{AtomicIntrinsics, LayoutContext, Paragraph, ParagraphBuilder};

fn latin_intrinsic_paragraph(text: &str, limits: &Limits, first_line: bool) -> Paragraph {
    let fonts = shodo_fixtures::load_fonts(limits).unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Shodo Fixture Latin".into())];
    style.root.font_size = 16.0;
    style.root.lang = Some("en".into());
    if first_line {
        style.first_line = Some(style.root.clone());
    }
    let mut b = ParagraphBuilder::new(&style, limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, text);
    b.build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
}

const LATIN_SPACING: &str = "One  two\tthree\nFour\u{a0}five soft\u{ad}hyphen.";

#[test]
fn intrinsic_reshape_budget_does_not_accumulate_between_calls() {
    for first_line in [false, true] {
        let p = latin_intrinsic_paragraph(LATIN_SPACING, &Limits::default(), first_line);
        let options = LineOptions::default();
        let inputs = AtomicIntrinsics::default();
        let fresh = p.intrinsic_sizes(&mut LayoutContext::new(), &options, &inputs);
        assert_eq!(fresh.min_content, 65.421875);
        assert_eq!(f64::from(fresh.max_content), 271.140625);
        let mut cx = LayoutContext::new();
        // The old implementation changes min-content on call 11,398, even
        // with a warm cache. Exercise the default cap without clearing it.
        for call in 1..=30_000 {
            let actual = p.intrinsic_sizes(&mut cx, &options, &inputs);
            assert_eq!(
                actual.min_content, fresh.min_content,
                "call {call}, first_line={first_line}"
            );
            assert_eq!(
                actual.max_content, fresh.max_content,
                "call {call}, first_line={first_line}"
            );
        }
        assert!(cx.take_warnings().is_empty());
    }
}

/// Line layout defers the edge reshapes of break candidates, so an ordinary
/// line no longer exhausts the per-operation budget. A first-line intrinsic
/// pass under a small window cap still does; the next operation must start
/// with a fresh budget.
#[test]
fn intrinsic_reshape_budget_is_independent_of_previous_operation() {
    let mut cx = LayoutContext::new();
    let limits = Limits {
        max_reshape_window_bytes: Some(16),
        ..Default::default()
    };
    let text = format!("{} ", LATIN_SPACING).repeat(32);
    let previous = latin_intrinsic_paragraph(&text, &limits, true);
    previous.intrinsic_sizes(
        &mut cx,
        &LineOptions::default(),
        &AtomicIntrinsics::default(),
    );
    let warnings = cx.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("edge reshape budget"))
    );
    let p = latin_intrinsic_paragraph(LATIN_SPACING, &Limits::default(), false);
    let actual = p.intrinsic_sizes(
        &mut cx,
        &LineOptions::default(),
        &AtomicIntrinsics::default(),
    );
    assert_eq!(actual.min_content, 65.421875);
    assert_eq!(f64::from(actual.max_content), 271.140625);
    assert!(cx.take_warnings().is_empty());
}

#[test]
fn intrinsic_first_line_passes_share_one_reshape_budget() {
    let limits = Limits {
        max_reshape_window_bytes: Some(16),
        ..Default::default()
    };
    let text = format!("{} ", LATIN_SPACING).repeat(32);
    let options = LineOptions::default();
    let inputs = AtomicIntrinsics::default();
    let plain = latin_intrinsic_paragraph(&text, &limits, false);
    let mut cx = LayoutContext::new();
    plain.intrinsic_sizes(&mut cx, &options, &inputs);
    assert!(cx.take_warnings().is_empty(), "one pass fits the budget");

    let first_line = latin_intrinsic_paragraph(&text, &limits, true);
    let mut cold = LayoutContext::new();
    let expected = first_line.intrinsic_sizes(&mut cold, &options, &inputs);
    assert!(
        cold.take_warnings()
            .iter()
            .any(|w| w.message.contains("edge reshape budget")),
        "two passes must share the operation's cap"
    );
    for _ in 0..3 {
        let actual = first_line.intrinsic_sizes(&mut cold, &options, &inputs);
        assert_eq!(actual.min_content, expected.min_content);
        assert_eq!(actual.max_content, expected.max_content);
        assert!(
            cold.take_warnings()
                .iter()
                .any(|w| w.message.contains("edge reshape budget"))
        );
    }
}
