//! Reference equivalence and operation-count guards for adjustment-only ruby
//! candidates (shodo-d77). The reference path (`cx.ruby_reference = true`)
//! measures every probe in full; every reuse path must match it exactly.
use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::geometry::{Direction, LayoutUnit, Saturation};
use crate::limits::{Limits, Warning, WarningKind};
use crate::node::{NodeId, OutOfFlowKind, TextSource};
use crate::ruby::*;
use crate::style::{
    FontFamily, InlineStyle, LineBreak, LineOptions, ParagraphStyle, VerticalAlign,
};
use crate::{
    AtomicIntrinsic, AtomicIntrinsics, AtomicSize, AtomicSizes, FloatClear, FloatIntrinsic,
    FloatSide, LayoutContext, Line, LineConstraint, LineResult, Paragraph, ParagraphBuilder,
};

const FAMILIES: [&str; 3] = [
    "Shodo Fixture CJK",
    "Shodo Fixture Arabic",
    "Shodo Fixture Latin",
];

/// `max_reshape_window_bytes` is charged per operation against this many
/// windows (`EDGE_RESHAPE_LINE_WINDOWS` in `line/windows.rs`).
const RESHAPE_WINDOWS: u64 = 64;

fn fonts() -> FontCollection {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for (bytes, family) in [
        (crate::test_support::fonts::CJK, FAMILIES[0]),
        (crate::test_support::fonts::ARABIC, FAMILIES[1]),
        (crate::test_support::fonts::LATIN, FAMILIES[2]),
    ] {
        fonts
            .register_face(
                bytes.to_vec(),
                0,
                FontFaceDescriptor {
                    family: family.into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    fonts
}

fn style(size: f32) -> InlineStyle {
    InlineStyle {
        font_size: size,
        font_families: FAMILIES
            .iter()
            .map(|family| FontFamily::Named((*family).into()))
            .collect(),
        ..Default::default()
    }
}

fn anywhere(size: f32) -> InlineStyle {
    InlineStyle {
        line_break: LineBreak::Anywhere,
        ..style(size)
    }
}

fn limits(window: Option<u64>, warnings: Option<u64>) -> Limits {
    Limits {
        max_reshape_window_bytes: window,
        max_warnings: warnings,
        ..Default::default()
    }
}

fn paragraph_style(first_line: bool) -> ParagraphStyle {
    ParagraphStyle {
        root: style(24.0),
        first_line: first_line.then(|| style(30.0)),
        ..Default::default()
    }
}

fn base_text(node: u64, text: &str, style: &InlineStyle, limits: &Limits) -> RubyContent {
    RubyContent::text(
        TextSource::Generated { node: NodeId(node) },
        text,
        style,
        limits,
    )
}

/// One level of annotations: one reading per base pairs them (`Auto`),
/// a single reading spans every base (`All`).
fn annotated(
    bases: Vec<RubyContent>,
    readings: &[&str],
    overhang: RubyOverhang,
    limits: &Limits,
) -> Ruby {
    let count = bases.len();
    Ruby::new(
        bases
            .into_iter()
            .enumerate()
            .map(|(i, content)| RubyBase {
                node: NodeId(10 + i as u64),
                content,
                align: RubyAlign::default(),
            })
            .collect(),
        vec![RubyLevel {
            annotations: readings
                .iter()
                .enumerate()
                .map(|(i, text)| RubyAnnotation {
                    node: NodeId(20 + i as u64),
                    content: base_text(20 + i as u64, text, &style(12.0), limits),
                    span: if readings.len() == count {
                        RubySpan::Auto
                    } else {
                        RubySpan::All
                    },
                    visibility: RubyVisibility::Visible,
                })
                .collect(),
            style: RubyStyle {
                overhang,
                ..Default::default()
            },
        }],
    )
    .unwrap()
}

fn finish(b: ParagraphBuilder) -> Paragraph {
    b.build(&mut LayoutContext::new(), &fonts()).unwrap()
}

/// `depth` rubies nested inside each other's single base. The innermost base
/// holds `text`; with default line breaking the outermost container's only
/// cuts are its own start and end, so every probe looks ahead to its end.
fn nested(depth: usize, limits: &Limits, text: &str, base: &InlineStyle) -> Paragraph {
    assert!(depth > 0);
    let mut content = base_text(30, text, base, limits);
    for level in 1..depth {
        let mut b = ParagraphBuilder::new(&paragraph_style(false), limits);
        b.push_ruby(
            NodeId(100 + level as u64),
            &style(24.0),
            annotated(vec![content], &["に"], RubyOverhang::None, limits),
        );
        content = RubyContent::from_builder(b);
    }
    let mut b = ParagraphBuilder::new(&paragraph_style(false), limits);
    b.push_ruby(
        NodeId(100),
        &style(24.0),
        annotated(vec![content], &["に"], RubyOverhang::None, limits),
    );
    finish(b)
}

fn siblings(limits: &Limits) -> Paragraph {
    let mut b = ParagraphBuilder::new(&paragraph_style(false), limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
    for i in 0..4u64 {
        b.push_ruby(
            NodeId(100 + i),
            &style(24.0),
            annotated(
                vec![base_text(30 + i, "日本", &style(24.0), limits)],
                &["にほんご"],
                RubyOverhang::None,
                limits,
            ),
        );
        b.push_text(
            TextSource::Generated {
                node: NodeId(200 + i),
            },
            "、",
        );
    }
    finish(b)
}

/// Cursive bases make every edge window unsafe, so candidates charge the
/// reshape budget; RTL paragraphs exercise mixed bidi.
fn arabic(limits: &Limits, direction: Direction) -> Paragraph {
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            direction,
            ..paragraph_style(false)
        },
        limits,
    );
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ab ");
    b.push_ruby(
        NodeId(100),
        &style(24.0),
        annotated(
            vec![
                base_text(30, "بببب", &style(24.0), limits),
                base_text(31, "ببب", &style(24.0), limits),
            ],
            &["に", "ほん"],
            RubyOverhang::Auto,
            limits,
        ),
    );
    b.push_text(TextSource::Generated { node: NodeId(2) }, "ببب cd");
    finish(b)
}

/// An atomic inline inside the base: without a caller size every selected
/// range reports `MissingAtomicSize`.
fn atomic_base(limits: &Limits, first_line: bool) -> Paragraph {
    let mut base = ParagraphBuilder::new(&paragraph_style(false), limits);
    base.push_text(TextSource::Generated { node: NodeId(30) }, "日");
    base.push_atomic(NodeId(99), &style(24.0), Default::default());
    base.push_text(TextSource::Generated { node: NodeId(31) }, "本");
    let mut b = ParagraphBuilder::new(&paragraph_style(first_line), limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
    b.push_ruby(
        NodeId(100),
        &style(24.0),
        annotated(
            vec![RubyContent::from_builder(base)],
            &["にほんごにほんご"],
            RubyOverhang::None,
            limits,
        ),
    );
    b.push_text(TextSource::Generated { node: NodeId(2) }, "本日");
    finish(b)
}

fn vertical_align(limits: &Limits) -> Paragraph {
    let mut base = ParagraphBuilder::new(&paragraph_style(false), limits);
    base.open_inline(
        NodeId(40),
        &InlineStyle {
            vertical_align: VerticalAlign::Top,
            ..style(48.0)
        },
        Default::default(),
    );
    base.push_text(TextSource::Generated { node: NodeId(41) }, "日本");
    base.close_inline();
    base.open_inline(
        NodeId(42),
        &InlineStyle {
            vertical_align: VerticalAlign::Bottom,
            ..style(36.0)
        },
        Default::default(),
    );
    base.push_text(TextSource::Generated { node: NodeId(43) }, "語");
    base.close_inline();
    let mut b = ParagraphBuilder::new(&paragraph_style(false), limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
    b.push_ruby(
        NodeId(100),
        &style(24.0),
        annotated(
            vec![RubyContent::from_builder(base)],
            &["にほんご"],
            RubyOverhang::None,
            limits,
        ),
    );
    b.push_text(TextSource::Generated { node: NodeId(2) }, "本");
    finish(b)
}

/// A reading wider than its base with plain-text neighbors on both sides:
/// `RubyOverhang::Auto` queries neighbor allowances.
fn overhang(limits: &Limits) -> Paragraph {
    let mut b = ParagraphBuilder::new(&paragraph_style(false), limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
    b.push_ruby(
        NodeId(100),
        &style(24.0),
        annotated(
            vec![base_text(30, "日", &style(24.0), limits)],
            &["にほんご"],
            RubyOverhang::Auto,
            limits,
        ),
    );
    b.push_text(TextSource::Generated { node: NodeId(2) }, "本日");
    finish(b)
}

/// Soft hyphens give `BreakClass::Hyphen`, the second per-unit call site in
/// `PartialLine::index`.
fn hyphenated(limits: &Limits) -> Paragraph {
    let mut b = ParagraphBuilder::new(&paragraph_style(false), limits);
    b.push_text(
        TextSource::Generated { node: NodeId(1) },
        "co\u{ad}op\u{ad}er\u{ad}a\u{ad}tion ",
    );
    b.push_ruby(
        NodeId(100),
        &style(24.0),
        annotated(
            vec![base_text(30, "日", &style(24.0), limits)],
            &["に"],
            RubyOverhang::None,
            limits,
        ),
    );
    b.push_text(TextSource::Generated { node: NodeId(2) }, " re\u{ad}use");
    finish(b)
}

/// A reading much wider than its one-glyph base.
const WIDE_READING: &str = "にほんごにほんご";

/// Max-content width of the leading float in `separated`.
const LEAD_FLOAT_MAX: f32 = 200.0;

#[derive(Clone, Copy, Debug)]
enum Separator {
    Forced,
    Block,
    Float,
}

/// A leading float (node 50), then a ruby with a wide reading next to a
/// separator unit (node 51): a forced break, a block-in-inline or a second
/// float. `ruby_first` puts the ruby before the separator, otherwise after.
fn separated(limits: &Limits, separator: Separator, ruby_first: bool) -> Paragraph {
    let mut b = ParagraphBuilder::new(&paragraph_style(false), limits);
    b.push_out_of_flow(NodeId(50), OutOfFlowKind::Float);
    let push_separator = |b: &mut ParagraphBuilder| {
        match separator {
            Separator::Forced => b.push_forced_break(NodeId(51)),
            Separator::Block => b.push_block_in_inline(NodeId(51)),
            Separator::Float => b.push_out_of_flow(NodeId(51), OutOfFlowKind::Float),
        };
    };
    if !ruby_first {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
        push_separator(&mut b);
    }
    b.push_ruby(
        NodeId(100),
        &style(24.0),
        annotated(
            vec![base_text(30, "日", &style(24.0), limits)],
            &[WIDE_READING],
            RubyOverhang::None,
            limits,
        ),
    );
    if ruby_first {
        push_separator(&mut b);
    }
    b.push_text(TextSource::Generated { node: NodeId(2) }, "本日");
    finish(b)
}

/// Intrinsic inputs for `separated`: the leading float is a wide left float;
/// a separating float is empty and clears with `clear`.
fn separated_intrinsics(clear: FloatClear) -> AtomicIntrinsics {
    let mut intrinsic = AtomicIntrinsics::default();
    intrinsic.insert_float(
        NodeId(50),
        FloatIntrinsic {
            min_content: 10.0,
            max_content: LEAD_FLOAT_MAX,
            side: FloatSide::Left,
            clear: FloatClear::None,
        },
    );
    intrinsic.insert_float(
        NodeId(51),
        FloatIntrinsic {
            min_content: 0.0,
            max_content: 0.0,
            side: FloatSide::Left,
            clear,
        },
    );
    intrinsic
}

fn sized(inline_size: f32) -> AtomicSizes {
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(99),
        AtomicSize {
            inline_size,
            ..Default::default()
        },
    );
    atomics
}

struct Fixture {
    name: String,
    paragraph: Paragraph,
    atomics: AtomicSizes,
    intrinsic: AtomicIntrinsics,
}

impl Fixture {
    fn new(name: impl Into<String>, paragraph: Paragraph) -> Self {
        Self {
            name: name.into(),
            paragraph,
            atomics: AtomicSizes::new(),
            intrinsic: AtomicIntrinsics::default(),
        }
    }
}

fn fixtures() -> Vec<Fixture> {
    let default = Limits::default();
    let mut out = Vec::new();
    for depth in [1, 3] {
        out.push(Fixture::new(
            format!("nested{depth}"),
            nested(depth, &default, "日", &style(24.0)),
        ));
    }
    out.push(Fixture::new(
        "nested-anywhere",
        nested(3, &default, "日本語", &anywhere(24.0)),
    ));
    out.push(Fixture::new("siblings", siblings(&default)));
    for window in [Some(2), Some(6), None] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            out.push(Fixture::new(
                format!("arabic-{window:?}-{direction:?}"),
                arabic(&limits(window, None), direction),
            ));
        }
    }
    for warnings in [Some(0), Some(1), Some(3), None] {
        out.push(Fixture::new(
            format!("atomic-missing-{warnings:?}"),
            atomic_base(&limits(None, warnings), false),
        ));
    }
    let mut sized_atomic = Fixture::new("atomic-sized", atomic_base(&default, false));
    sized_atomic.atomics = sized(30.0);
    out.push(sized_atomic);
    out.push(Fixture::new("vertical-align", vertical_align(&default)));
    let mut first_line = Fixture::new("first-line", atomic_base(&default, true));
    first_line.atomics = sized(30.0);
    first_line.intrinsic.insert_atomic(
        NodeId(99),
        AtomicIntrinsic {
            min_content: 4.0,
            max_content: 80.0,
        },
    );
    out.push(first_line);
    out.push(Fixture::new("overhang", overhang(&default)));
    out.push(Fixture::new("hyphens", hyphenated(&default)));
    for (separator, clear) in [
        (Separator::Forced, FloatClear::None),
        (Separator::Block, FloatClear::None),
        (Separator::Float, FloatClear::None),
        (Separator::Float, FloatClear::Left),
    ] {
        for ruby_first in [true, false] {
            let mut fixture = Fixture::new(
                format!("separated-{separator:?}-{clear:?}-{ruby_first}"),
                separated(&default, separator, ruby_first),
            );
            fixture.intrinsic = separated_intrinsics(clear);
            out.push(fixture);
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
struct PreState {
    /// Bytes already charged in the operation before the first probe.
    spent: u64,
    /// The context's warning sink is already suppressed.
    suppressed: bool,
}

#[derive(Debug, PartialEq)]
struct Observed {
    values: Vec<LayoutUnit>,
    warnings: Vec<Warning>,
    sat: Saturation,
    spent: u64,
    /// The same probes again after `begin_reshape_operation` on the same
    /// context.
    reset_values: Vec<LayoutUnit>,
    reset_warnings: Vec<Warning>,
    reset_spent: u64,
}

/// Every `(start, end)` probe with a growing `end` per start, then one
/// shrinking sweep, all inside one operation; then all of it again in a new
/// operation on the same context.
fn observe_candidates(
    p: &Paragraph,
    atomics: &AtomicSizes,
    reference: bool,
    pre: PreState,
) -> Observed {
    let data = &p.data;
    let mut cx = LayoutContext::new();
    cx.ruby_reference = reference;
    if pre.suppressed {
        cx.warnings.set_max(Some(0));
        cx.warnings.push(WarningKind::Unsupported, "pre-existing");
    } else {
        cx.warnings.set_max(data.limits.max_warnings);
    }
    cx.begin_reshape_operation();
    cx.edge_reshape_spent = pre.spent;
    let mut sat = Saturation::default();
    let values = sweep(data, atomics, &mut cx, &mut sat);
    let warnings = cx.warnings.as_slice().to_vec();
    let spent = cx.edge_reshape_spent;
    cx.begin_reshape_operation();
    let reset_values = sweep(data, atomics, &mut cx, &mut sat);
    Observed {
        values,
        warnings,
        sat,
        spent,
        reset_values,
        reset_warnings: cx.warnings.as_slice().to_vec(),
        reset_spent: cx.edge_reshape_spent,
    }
}

fn sweep(
    data: &crate::paragraph::ParagraphData,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Vec<LayoutUnit> {
    let mut values = Vec::new();
    let n = data.units.len();
    for start in 0..n {
        for end in start + 1..=n {
            values.push(crate::ruby::measure::candidate_adjustment(
                data, start, end, atomics, cx, sat,
            ));
        }
    }
    for end in (1..=n).rev() {
        values.push(crate::ruby::measure::candidate_adjustment(
            data, 0, end, atomics, cx, sat,
        ));
    }
    values
}

fn pre_states(p: &Paragraph) -> Vec<PreState> {
    let spents = match p.data.limits.max_reshape_window_bytes {
        Some(window) => {
            let limit = window.saturating_mul(RESHAPE_WINDOWS);
            vec![0, limit.saturating_sub(16), limit + 1]
        }
        None => vec![0],
    };
    spents
        .into_iter()
        .flat_map(|spent| {
            [false, true]
                .into_iter()
                .map(move |suppressed| PreState { spent, suppressed })
        })
        .collect()
}

fn signature(line: &Line) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        line.text_range(),
        line.break_reason(),
        (
            line.inline_size(),
            line.block_size(),
            line.baseline(crate::geometry::BaselineKind::Alphabetic)
        ),
        (line.hang_start(), line.hang_end()),
        line.fragments().collect::<Vec<_>>(),
        line.ruby_annotations()
            .map(|a| (
                a.container(),
                a.level(),
                a.base_text_range(),
                a.text_range(),
                a.transform(),
                a.line().text_range(),
                a.line().inline_size()
            ))
            .collect::<Vec<_>>()
    )
}

fn result_signature(result: LineResult) -> String {
    match result {
        LineResult::Line(line) => signature(&line),
        other => format!("{other:?}"),
    }
}

fn context(reference: bool) -> LayoutContext {
    let mut cx = LayoutContext::new();
    cx.ruby_reference = reference;
    cx
}

/// Fixtures whose scan is retained, so the 1000 -> 999 retry must index it.
/// `atomic-missing-*` scans warn, and a scan that warns is never retained.
fn retry_indexes(name: &str) -> bool {
    matches!(name, "hyphens" | "atomic-sized" | "first-line")
}

/// The first line of `p` at `width`, following every float encountered with
/// its cursor (each float result is recorded too).
fn first_line_through_floats(
    p: &Paragraph,
    cx: &mut LayoutContext,
    width: f32,
    atomics: &AtomicSizes,
    out: &mut Vec<String>,
) {
    let options = LineOptions::default();
    let mut constraint = LineConstraint::new(width);
    for _ in 0..8 {
        let result = p.next_line(cx, p.start_token(), &options, &constraint, atomics);
        let float_cursor = match &result {
            LineResult::FloatEncountered { float_cursor, .. } => Some(*float_cursor),
            _ => None,
        };
        out.push(format!("floats {width}: {}", result_signature(result)));
        out.push(format!("warnings: {:?}", cx.take_warnings()));
        match float_cursor {
            Some(cursor) => constraint.floats_placed_through = Some(cursor),
            None => return,
        }
    }
    panic!("too many floats");
}

/// `break_all` with warm and cold contexts, a wide-then-narrow retry of the
/// same token (drives `PartialLine::index`), floats followed through their
/// cursors, and intrinsic sizes.
fn observe_layout(fixture: &Fixture, reference: bool) -> Vec<String> {
    let p = &fixture.paragraph;
    let options = LineOptions::default();
    let mut out = Vec::new();
    let mut warm = context(reference);
    for width in [24.0f32, 48.0, 96.0, 1000.0] {
        let mut cold = context(reference);
        for cx in [&mut warm, &mut cold] {
            let lines = p.break_all(cx, &options, width, &fixture.atomics);
            out.push(format!(
                "break_all {width}: {:?}",
                lines.iter().map(signature).collect::<Vec<_>>()
            ));
            out.push(format!("warnings: {:?}", cx.take_warnings()));
        }
    }
    let mut retry = context(reference);
    for width in [1000.0f32, 999.0, 60.0, 30.0] {
        retry.cache_prepare_visits = 0;
        let result = p.next_line(
            &mut retry,
            p.start_token(),
            &options,
            &LineConstraint::new(width),
            &fixture.atomics,
        );
        if width == 999.0 && retry_indexes(&fixture.name) {
            assert!(
                retry.cache_prepare_visits > 0,
                "{}: the 999 retry must run PartialLine::index",
                fixture.name
            );
        }
        out.push(format!("next_line {width}: {}", result_signature(result)));
        out.push(format!("warnings: {:?}", retry.take_warnings()));
    }
    for width in [1000.0f32, 60.0] {
        let mut cx = context(reference);
        first_line_through_floats(p, &mut cx, width, &fixture.atomics, &mut out);
    }
    let mut intrinsic = context(reference);
    out.push(format!(
        "intrinsic: {:?}",
        p.intrinsic_sizes(&mut intrinsic, &options, &fixture.intrinsic)
    ));
    out.push(format!("warnings: {:?}", intrinsic.take_warnings()));
    out
}

/// One warm context across operations that `shrink_to` between them, and one
/// that alternates paragraphs. Both modes must share `all`: font ids differ
/// between font collections.
fn observe_warm(all: &[Fixture], reference: bool) -> Vec<String> {
    let options = LineOptions::default();
    let mut out = Vec::new();
    let operations = |cx: &mut LayoutContext, fixture: &Fixture, out: &mut Vec<String>| {
        let p = &fixture.paragraph;
        for width in [1000.0f32, 999.0, 48.0] {
            let result = p.next_line(
                cx,
                p.start_token(),
                &options,
                &LineConstraint::new(width),
                &fixture.atomics,
            );
            out.push(format!(
                "{} next_line {width}: {}",
                fixture.name,
                result_signature(result)
            ));
        }
        let lines = p.break_all(cx, &options, 48.0, &fixture.atomics);
        out.push(format!(
            "{} break_all: {:?}",
            fixture.name,
            lines.iter().map(signature).collect::<Vec<_>>()
        ));
        out.push(format!(
            "{} intrinsic: {:?}",
            fixture.name,
            p.intrinsic_sizes(cx, &options, &fixture.intrinsic)
        ));
        out.push(format!("warnings: {:?}", cx.take_warnings()));
    };
    for fixture in all {
        let mut cx = context(reference);
        for bytes in [usize::MAX, 4096, 0] {
            operations(&mut cx, fixture, &mut out);
            cx.shrink_to(bytes);
        }
        operations(&mut cx, fixture, &mut out);
    }
    let pick = |name: &str| all.iter().find(|f| f.name == name).unwrap();
    let mut cx = context(reference);
    for name in [
        "siblings",
        "nested3",
        "siblings",
        "nested-anywhere",
        "arabic-Some(2)-Ltr",
        "siblings",
    ] {
        operations(&mut cx, pick(name), &mut out);
    }
    out
}

#[test]
fn adjustment_only_candidates_match_reference_for_every_range() {
    for fixture in fixtures() {
        for pre in pre_states(&fixture.paragraph) {
            assert_eq!(
                observe_candidates(&fixture.paragraph, &fixture.atomics, false, pre),
                observe_candidates(&fixture.paragraph, &fixture.atomics, true, pre),
                "{}: {pre:?}",
                fixture.name
            );
        }
    }
}

#[test]
fn line_layout_paths_match_reference() {
    for fixture in fixtures() {
        assert_eq!(
            observe_layout(&fixture, false),
            observe_layout(&fixture, true),
            "{}",
            fixture.name
        );
    }
}

#[test]
fn warm_context_paths_match_reference() {
    let all = fixtures();
    assert_eq!(observe_warm(&all, false), observe_warm(&all, true));
}

/// The equivalence fixtures must reach the paths they claim to cover.
#[test]
fn equivalence_fixtures_reach_budget_and_warning_paths() {
    let budget = arabic(&limits(Some(2), None), Direction::Ltr);
    let observed = observe_candidates(
        &budget,
        &AtomicSizes::EMPTY,
        true,
        PreState {
            spent: 0,
            suppressed: false,
        },
    );
    assert!(
        observed
            .warnings
            .iter()
            .any(|w| w.message == "line edge reshape budget exceeded; keeping shared glyphs"),
        "{:?}",
        observed.warnings
    );
    let missing = atomic_base(&limits(None, None), false);
    let observed = observe_candidates(
        &missing,
        &AtomicSizes::EMPTY,
        true,
        PreState {
            spent: 0,
            suppressed: false,
        },
    );
    assert!(
        observed
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::MissingAtomicSize)
    );

    assert_separator_sites_reached();
    assert_hyphen_index_site_reached();
}

/// Results of `next_line` from the start token, following floats.
fn results_through_floats(p: &Paragraph, width: f32) -> Vec<LineResult> {
    let mut cx = LayoutContext::new();
    let mut constraint = LineConstraint::new(width);
    let mut out = Vec::new();
    loop {
        let result = p.next_line(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        );
        let cursor = match &result {
            LineResult::FloatEncountered { float_cursor, .. } => Some(*float_cursor),
            _ => None,
        };
        out.push(result);
        match cursor {
            Some(cursor) => constraint.floats_placed_through = Some(cursor),
            None => return out,
        }
    }
}

/// The `separated` fixtures reach the float site of `line::cache::resolve`
/// and the forced-break, block-in-inline and float-clear sites of
/// `intrinsic_sizes`, each with the wide ruby inside the probed range.
fn assert_separator_sites_reached() {
    let default = Limits::default();
    // The ruby line width: the line ending at the forced break holds only
    // the ruby, whose reading is wider than its one 24px glyph base.
    let forced = separated(&default, Separator::Forced, true);
    let results = results_through_floats(&forced, 1000.0);
    let [
        LineResult::FloatEncountered { node, .. },
        LineResult::Line(line),
    ] = &results[..]
    else {
        panic!("{:?}", results.len());
    };
    assert_eq!(*node, NodeId(50));
    assert_eq!(line.break_reason(), crate::BreakReason::Forced);
    let ruby_width = line.inline_size();
    assert!(ruby_width > 24.0, "{ruby_width}");

    // `resolve` reports the separating float after the ruby, positioned by
    // the ruby adjustment.
    let floated = separated(&default, Separator::Float, true);
    let results = results_through_floats(&floated, 1000.0);
    let LineResult::FloatEncountered {
        node,
        inline_position,
        ..
    } = &results[1]
    else {
        panic!("no second float");
    };
    assert_eq!(*node, NodeId(51));
    assert_eq!(*inline_position, ruby_width);

    // Only the separator site measures the leading float together with the
    // ruby: the forced break and the clearance reset the left float strip
    // before the paragraph-end site, which sees the ruby and "本日" alone.
    for (separator, clear) in [
        (Separator::Forced, FloatClear::None),
        (Separator::Block, FloatClear::None),
        (Separator::Float, FloatClear::Left),
    ] {
        let p = separated(&default, separator, true);
        let sizes = p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &separated_intrinsics(clear),
        );
        assert_eq!(
            sizes.max_content,
            LEAD_FLOAT_MAX + ruby_width,
            "{separator:?} {clear:?}"
        );
    }
}

/// After a 999 retry indexes the retained 1000 scan, a 30px retry is served
/// from the index alone (no scan, no new index) and ends at a soft hyphen:
/// only the hyphen site of `PartialLine::index` records such ends.
fn assert_hyphen_index_site_reached() {
    let p = hyphenated(&Limits::default());
    let options = LineOptions::default();
    let mut cx = LayoutContext::new();
    for width in [1000.0f32, 999.0] {
        let _ = p.next_line(
            &mut cx,
            p.start_token(),
            &options,
            &LineConstraint::new(width),
            &AtomicSizes::EMPTY,
        );
    }
    assert!(cx.cache_prepare_visits > 0);
    cx.cache_prepare_visits = 0;
    cx.cache_visits = 0;
    let LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &options,
        &LineConstraint::new(30.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("no line");
    };
    assert_eq!((cx.cache_prepare_visits, cx.cache_visits), (0, 0));
    assert!(
        p.data.text[..line.text_range().end].ends_with('\u{ad}'),
        "{:?}",
        line.text_range()
    );
}

#[derive(Clone, Copy, Debug)]
struct ScanCosts {
    width_calls: usize,
    scalar_calls: usize,
    columns: usize,
    walk: usize,
}

fn costs(cx: &LayoutContext) -> ScanCosts {
    ScanCosts {
        width_calls: cx.ruby_width_calls,
        scalar_calls: cx.ruby_scalar_calls,
        columns: cx.ruby_column_visits,
        walk: crate::ruby::index::take_visits(),
    }
}

/// One unbreakable nested line: the scan probes `candidate(0, i + 1)` for
/// every unit, and every probe looks ahead to the outermost end.
fn break_all_costs(depth: usize, reference: bool) -> ScanCosts {
    let p = nested(depth, &Limits::default(), "日", &style(24.0));
    let mut cx = context(reference);
    crate::ruby::index::take_visits();
    p.break_all(&mut cx, &LineOptions::default(), 96.0, &AtomicSizes::EMPTY);
    costs(&cx)
}

/// A narrower retry of a retained wide scan runs `PartialLine::index`, which
/// probes every unit again.
fn index_costs(depth: usize, reference: bool) -> ScanCosts {
    let p = nested(depth, &Limits::default(), "日", &style(24.0));
    let options = LineOptions::default();
    let mut cx = context(reference);
    let _ = p.next_line(
        &mut cx,
        p.start_token(),
        &options,
        &LineConstraint::new(100000.0),
        &AtomicSizes::EMPTY,
    );
    cx.ruby_width_calls = 0;
    cx.ruby_scalar_calls = 0;
    cx.ruby_column_visits = 0;
    cx.cache_prepare_visits = 0;
    crate::ruby::index::take_visits();
    let _ = p.next_line(
        &mut cx,
        p.start_token(),
        &options,
        &LineConstraint::new(99999.0),
        &AtomicSizes::EMPTY,
    );
    assert!(
        cx.cache_prepare_visits > 0,
        "the retry must run PartialLine::index"
    );
    costs(&cx)
}

/// Growth factor of each doubling of the nesting depth.
fn ratios(
    measure: fn(usize, bool) -> ScanCosts,
    reference: bool,
    pick: fn(&ScanCosts) -> usize,
) -> (Vec<ScanCosts>, Vec<f64>) {
    let all: Vec<_> = [8, 16, 32].iter().map(|d| measure(*d, reference)).collect();
    let growth = all
        .windows(2)
        .map(|pair| pick(&pair[1]) as f64 / pick(&pair[0]).max(1) as f64)
        .collect();
    (all, growth)
}

fn width_calls(c: &ScanCosts) -> usize {
    c.width_calls
}
fn scalar_calls(c: &ScanCosts) -> usize {
    c.scalar_calls
}
fn column_visits(c: &ScanCosts) -> usize {
    c.columns
}
fn walk_visits(c: &ScanCosts) -> usize {
    c.walk
}

type Pick = (&'static str, fn(&ScanCosts) -> usize);
const PICKS: [Pick; 4] = [
    ("width", width_calls),
    ("scalar", scalar_calls),
    ("columns", column_visits),
    ("walk", walk_visits),
];
type Path = (&'static str, fn(usize, bool) -> ScanCosts);
const PATHS: [Path; 2] = [("break_all", break_all_costs), ("index", index_costs)];

/// The guards below would pass vacuously if the counters missed the work.
/// The reference path must show the D^2 term on every counter.
#[test]
fn reference_nested_candidates_grow_quadratically() {
    for (path, measure) in PATHS {
        for (what, pick) in PICKS {
            let (all, growth) = ratios(measure, true, pick);
            assert!(
                growth.iter().all(|g| *g >= 3.0),
                "{path} {what}: {growth:?} {all:?}"
            );
        }
    }
}
