//! Reference equivalence and operation-count guards for adjustment-only ruby
//! candidates (shodo-d77). The reference path (`cx.ruby_reference = true`)
//! measures every probe in full; every reuse path must match it exactly.
use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::geometry::{Direction, LayoutUnit, Saturation};
use crate::limits::{Limits, Warning, WarningKind};
use crate::node::{NodeId, TextSource};
use crate::ruby::*;
use crate::style::{
    FontFamily, InlineStyle, LineBreak, LineOptions, ParagraphStyle, VerticalAlign,
};
use crate::{
    AtomicIntrinsic, AtomicIntrinsics, AtomicSize, AtomicSizes, LayoutContext, Line,
    LineConstraint, LineResult, Paragraph, ParagraphBuilder,
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
}

/// Every `(start, end)` probe with a growing `end` per start, then one
/// shrinking sweep, all inside one operation.
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
    let mut values = Vec::new();
    let n = data.units.len();
    for start in 0..n {
        for end in start + 1..=n {
            values.push(crate::ruby::measure::candidate_adjustment(
                data, start, end, atomics, &mut cx, &mut sat,
            ));
        }
    }
    for end in (1..=n).rev() {
        values.push(crate::ruby::measure::candidate_adjustment(
            data, 0, end, atomics, &mut cx, &mut sat,
        ));
    }
    Observed {
        values,
        warnings: cx.warnings.as_slice().to_vec(),
        sat,
        spent: cx.edge_reshape_spent,
    }
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

/// `break_all` with warm and cold contexts, a wide-then-narrow retry of the
/// same token (drives `PartialLine::index`), and intrinsic sizes.
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
        let result = p.next_line(
            &mut retry,
            p.start_token(),
            &options,
            &LineConstraint::new(width),
            &fixture.atomics,
        );
        out.push(format!("next_line {width}: {}", result_signature(result)));
        out.push(format!("warnings: {:?}", retry.take_warnings()));
    }
    let mut intrinsic = context(reference);
    out.push(format!(
        "intrinsic: {:?}",
        p.intrinsic_sizes(&mut intrinsic, &options, &fixture.intrinsic)
    ));
    out.push(format!("warnings: {:?}", intrinsic.take_warnings()));
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
