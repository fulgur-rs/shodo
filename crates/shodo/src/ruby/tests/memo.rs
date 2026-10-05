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
pub(super) const RESHAPE_WINDOWS: u64 = 64;

pub(super) fn fonts() -> FontCollection {
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

pub(super) fn style(size: f32) -> InlineStyle {
    InlineStyle {
        font_size: size,
        font_families: FAMILIES
            .iter()
            .map(|family| FontFamily::Named((*family).into()))
            .collect(),
        ..Default::default()
    }
}

pub(super) fn anywhere(size: f32) -> InlineStyle {
    InlineStyle {
        line_break: LineBreak::Anywhere,
        ..style(size)
    }
}

pub(super) fn limits(window: Option<u64>, warnings: Option<u64>) -> Limits {
    Limits {
        max_reshape_window_bytes: window,
        max_warnings: warnings,
        ..Default::default()
    }
}

pub(super) fn paragraph_style(first_line: bool) -> ParagraphStyle {
    ParagraphStyle {
        root: style(24.0),
        first_line: first_line.then(|| style(30.0)),
        ..Default::default()
    }
}

pub(super) fn base_text(
    node: u64,
    text: &str,
    style: &InlineStyle,
    limits: &Limits,
) -> RubyContent {
    RubyContent::text(
        TextSource::Generated { node: NodeId(node) },
        text,
        style,
        limits,
    )
}

/// One level of annotations: one reading per base pairs them (`Auto`),
/// a single reading spans every base (`All`).
pub(super) fn annotated(
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

pub(super) fn finish(b: ParagraphBuilder) -> Paragraph {
    b.build(&mut LayoutContext::new(), &fonts()).unwrap()
}

/// `depth` rubies nested inside each other's single base. The innermost base
/// holds `text`; with default line breaking the outermost container's only
/// cuts are its own start and end, so every probe looks ahead to its end.
pub(super) fn nested(depth: usize, limits: &Limits, text: &str, base: &InlineStyle) -> Paragraph {
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

pub(super) fn siblings(limits: &Limits) -> Paragraph {
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
pub(super) fn arabic(limits: &Limits, direction: Direction) -> Paragraph {
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
pub(super) fn atomic_base(limits: &Limits, first_line: bool) -> Paragraph {
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

pub(super) fn vertical_align(limits: &Limits) -> Paragraph {
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
pub(super) fn overhang(limits: &Limits) -> Paragraph {
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

pub(super) struct Fixture {
    pub(super) name: String,
    pub(super) paragraph: Paragraph,
    pub(super) atomics: AtomicSizes,
    pub(super) intrinsic: AtomicIntrinsics,
}

impl Fixture {
    pub(super) fn new(name: impl Into<String>, paragraph: Paragraph) -> Self {
        Self {
            name: name.into(),
            paragraph,
            atomics: AtomicSizes::new(),
            intrinsic: AtomicIntrinsics::default(),
        }
    }
}

pub(super) fn fixtures() -> Vec<Fixture> {
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
pub(super) struct PreState {
    /// Bytes already charged in the operation before the first probe.
    pub(super) spent: u64,
    /// The context's warning sink is already suppressed.
    pub(super) suppressed: bool,
}

#[derive(Debug, PartialEq)]
pub(super) struct Observed {
    values: Vec<LayoutUnit>,
    warnings: Vec<Warning>,
    sat: Saturation,
    spent: u64,
    /// The same probes again after `begin_reshape_operation` on the same
    /// context.
    reset_values: Vec<LayoutUnit>,
    reset_warnings: Vec<Warning>,
    reset_spent: u64,
    /// Step-oracle mismatches over both sweeps.
    misses: Vec<String>,
}

/// Every `(start, end)` probe with a growing `end` per start, then one
/// shrinking sweep, all inside one operation; then all of it again in a new
/// operation on the same context. Also returns the memo hits.
pub(super) fn observe_candidates(
    p: &Paragraph,
    atomics: &AtomicSizes,
    reference: bool,
    pre: PreState,
) -> (Observed, usize) {
    let mode = if reference {
        Mode::Reference
    } else {
        Mode::Accumulate
    };
    let (observed, counters) = observe_candidates_in(p, atomics, mode, pre);
    (observed, counters.hits)
}

pub(super) fn observe_candidates_in(
    p: &Paragraph,
    atomics: &AtomicSizes,
    mode: Mode,
    pre: PreState,
) -> (Observed, Counters) {
    let data = &p.data;
    let mut cx = mode_context(mode);
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
    let observed = Observed {
        values,
        warnings,
        sat,
        spent,
        reset_values,
        reset_warnings: cx.warnings.as_slice().to_vec(),
        reset_spent: cx.edge_reshape_spent,
        misses: std::mem::take(&mut cx.ruby_oracle_misses),
    };
    (observed, counters(&cx))
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

pub(super) fn pre_states(p: &Paragraph) -> Vec<PreState> {
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

pub(super) fn signature(line: &Line) -> String {
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

pub(super) fn result_signature(result: LineResult) -> String {
    match result {
        LineResult::Line(line) => signature(&line),
        other => format!("{other:?}"),
    }
}

/// Which measurement path a context takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    /// Every probe measured in full (`cx.ruby_reference`).
    Reference,
    /// The per-through memo alone (the shodo-d77 path).
    Memo,
    /// The default path: the memo and the container accumulator.
    Accumulate,
    /// As `Accumulate`, with clean containers measured live and compared
    /// with their entries (the step oracle).
    Verify,
}

pub(super) fn mode_context(mode: Mode) -> LayoutContext {
    let mut cx = LayoutContext::new();
    match mode {
        Mode::Reference => cx.ruby_reference = true,
        Mode::Memo => cx.ruby_accumulate_disabled = true,
        Mode::Accumulate => {}
        Mode::Verify => cx.ruby_accumulate_verify = true,
    }
    cx
}

pub(super) fn context(reference: bool) -> LayoutContext {
    mode_context(if reference {
        Mode::Reference
    } else {
        Mode::Accumulate
    })
}

/// Test counters of one context.
#[derive(Clone, Debug, Default)]
pub(super) struct Counters {
    pub(super) hits: usize,
    pub(super) measures: usize,
    pub(super) replayed: usize,
    pub(super) dirty: [usize; 7],
    pub(super) resets: usize,
    pub(super) refusals: usize,
    /// Replays of runs with more profile calls than positions under a
    /// charging profile (`ruby_repeated_profile_replays`).
    pub(super) repeated_profile: usize,
    /// Detached profile measurements that warned.
    pub(super) profile_warnings: usize,
}

pub(super) fn counters(cx: &LayoutContext) -> Counters {
    Counters {
        hits: cx.ruby_memo_hits,
        measures: cx.ruby_container_measures,
        replayed: cx.ruby_replayed_containers,
        dirty: cx.ruby_dirty,
        resets: cx.ruby_accumulator_resets,
        refusals: cx.ruby_replay_refusals,
        repeated_profile: cx.ruby_repeated_profile_replays,
        profile_warnings: cx.ruby_profile_warnings,
    }
}

/// Step-oracle mismatches of `cx` since the last call (always empty on the
/// reference path), recorded next to the warnings they accompany.
fn oracle(cx: &mut LayoutContext) -> String {
    format!("oracle: {:?}", std::mem::take(&mut cx.ruby_oracle_misses))
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

pub(super) fn observe_layout(fixture: &Fixture, reference: bool) -> Vec<String> {
    observe_layout_in(
        fixture,
        if reference {
            Mode::Reference
        } else {
            Mode::Accumulate
        },
    )
}

/// `break_all` with warm and cold contexts, a wide-then-narrow retry of the
/// same token (drives `PartialLine::index`), floats followed through their
/// cursors, and intrinsic sizes.
pub(super) fn observe_layout_in(fixture: &Fixture, mode: Mode) -> Vec<String> {
    let p = &fixture.paragraph;
    let options = LineOptions::default();
    let mut out = Vec::new();
    let mut warm = mode_context(mode);
    for width in [24.0f32, 48.0, 96.0, 1000.0] {
        let mut cold = mode_context(mode);
        for cx in [&mut warm, &mut cold] {
            let lines = p.break_all(cx, &options, width, &fixture.atomics);
            out.push(format!(
                "break_all {width}: {:?}",
                lines.iter().map(signature).collect::<Vec<_>>()
            ));
            out.push(format!("warnings: {:?}", cx.take_warnings()));
            out.push(oracle(cx));
        }
    }
    let mut retry = mode_context(mode);
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
        out.push(oracle(&mut retry));
    }
    for width in [1000.0f32, 60.0] {
        let mut cx = mode_context(mode);
        first_line_through_floats(p, &mut cx, width, &fixture.atomics, &mut out);
        out.push(oracle(&mut cx));
    }
    let mut intrinsic = mode_context(mode);
    out.push(format!(
        "intrinsic: {:?}",
        p.intrinsic_sizes(&mut intrinsic, &options, &fixture.intrinsic)
    ));
    out.push(format!("warnings: {:?}", intrinsic.take_warnings()));
    out.push(oracle(&mut intrinsic));
    out
}

pub(super) fn observe_warm(all: &[Fixture], reference: bool) -> Vec<String> {
    observe_warm_in(
        all,
        if reference {
            Mode::Reference
        } else {
            Mode::Accumulate
        },
    )
}

/// One warm context across operations that `shrink_to` between them, and one
/// that alternates paragraphs. Both modes must share `all`: font ids differ
/// between font collections.
pub(super) fn observe_warm_in(all: &[Fixture], mode: Mode) -> Vec<String> {
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
        out.push(oracle(cx));
    };
    for fixture in all {
        let mut cx = mode_context(mode);
        for bytes in [usize::MAX, 4096, 0] {
            operations(&mut cx, fixture, &mut out);
            cx.shrink_to(bytes);
        }
        operations(&mut cx, fixture, &mut out);
    }
    let pick = |name: &str| all.iter().find(|f| f.name == name).unwrap();
    let mut cx = mode_context(mode);
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
            let (optimized, hits) =
                observe_candidates(&fixture.paragraph, &fixture.atomics, false, pre);
            let (reference, reference_hits) =
                observe_candidates(&fixture.paragraph, &fixture.atomics, true, pre);
            assert_eq!(optimized, reference, "{}: {pre:?}", fixture.name);
            assert_eq!(reference_hits, 0);
            // Equivalence must not hold vacuously: nested and sibling probes
            // share look-ahead endpoints and are answered from the memo.
            if fixture.name.starts_with("nested") || fixture.name == "siblings" {
                assert!(hits > 0, "{}: no memo hits", fixture.name);
            }
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
    let (observed, _) = observe_candidates(
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
    let (observed, _) = observe_candidates(
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

/// A present but empty slot (as left by an unwound query between take and
/// put-back) is rebuilt, never unwrapped.
#[test]
fn vacated_index_slots_are_rebuilt() {
    let p = overhang(&Limits::default());
    let n = p.data.units.len();
    let mut cx = LayoutContext::new();
    let measure = |cx: &mut LayoutContext| {
        format!(
            "{:?}",
            crate::ruby::measure::candidate(
                &p.data,
                0,
                n,
                &AtomicSizes::EMPTY,
                cx,
                &mut Saturation::default(),
            )
        )
    };
    let first = measure(&mut cx);
    assert!(
        cx.ruby_ranges.slots_filled(),
        "fixture must use both indexes"
    );
    cx.ruby_ranges.vacate_slots();
    assert_eq!(measure(&mut cx), first);
    assert!(cx.ruby_ranges.slots_filled());
}

/// All containers of one candidate resolve their columns against the same
/// selected range; its edge windows and line profile are measured once.
#[test]
fn one_candidate_selects_its_line_profile_once() {
    let p = nested(6, &Limits::default(), "日", &style(24.0));
    let n = p.data.units.len();
    let run = |reference: bool| {
        let mut cx = context(reference);
        let measure = crate::ruby::measure::candidate(
            &p.data,
            0,
            n,
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut Saturation::default(),
        );
        (format!("{measure:?}"), cx.ruby_profile_selects)
    };
    let (optimized, shared) = run(false);
    let (reference, separate) = run(true);
    assert_eq!(optimized, reference);
    // One `overhang::columns` query per container (overhang is `None`).
    assert_eq!(separate, 6);
    assert_eq!(shared, 1);
}

/// A shared profile whose recorded reshape charges no longer fit is measured
/// afresh, so the crossing charge is refused and warns exactly once, as on
/// the reference path. Sweeping the candidate range and the starting `spent`
/// across the limit reaches accepted, crossing and refused replays.
#[test]
fn shared_profile_recomputes_when_its_charges_cross_the_budget() {
    let limits = limits(Some(64), None);
    let p = nested(3, &limits, "بببب", &anywhere(24.0));
    let n = p.data.units.len();
    let run = |reference: bool, start: usize, end: usize, spent: u64| {
        let mut cx = context(reference);
        cx.edge_reshape_spent = spent;
        let mut sat = Saturation::default();
        let measure = crate::ruby::measure::candidate(
            &p.data,
            start,
            end,
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut sat,
        );
        (
            format!("{measure:?} {sat:?} {}", cx.edge_reshape_spent),
            cx.take_warnings(),
            cx.ruby_profile_selects,
            cx.ruby_replay_refusals,
        )
    };
    let (mut replayed, mut recomputed, mut refused) = (0, 0, 0);
    for start in (0..n).step_by(3) {
        for end in (start + 1..n).step_by(4).chain([n]) {
            for spent in (64 * 64 - 64..=64 * 64 + 4).chain([0, u64::MAX]) {
                let (optimized, warnings, shared, refusals) = run(false, start, end, spent);
                let (reference, reference_warnings, separate, _) = run(true, start, end, spent);
                refused += refusals;
                assert_eq!(optimized, reference, "{start}..{end} spent {spent}");
                assert_eq!(warnings, reference_warnings, "{start}..{end} spent {spent}");
                if shared < separate {
                    replayed += 1;
                }
                if shared > 1 && shared < separate {
                    recomputed += 1;
                }
            }
        }
    }
    // Refused replays prove by construction that the gate, not a warning
    // during recording, sent shared profiles back to measurement.
    assert!(
        replayed > 0 && recomputed > 0 && refused > 0,
        "{replayed} {recomputed} {refused}"
    );
}

/// Probes sharing `start..through` inside one operation measure the core
/// once; a new operation measures it again. A warm-up operation fills the
/// range caches first (since shodo-tj5 a measurement that fills them is
/// memoized too: `cold_cache_fills_are_memoized`).
#[test]
fn repeated_probe_in_one_operation_is_measured_once() {
    let p = nested(4, &Limits::default(), "日", &style(24.0));
    let data = &p.data;
    let outer = data.ruby.containers[0].units.clone();
    assert!(outer.start + 3 < outer.end);
    let mut cx = LayoutContext::new();
    let mut sat = Saturation::default();
    let probe = |cx: &mut LayoutContext, sat: &mut Saturation, end| {
        crate::ruby::measure::candidate_adjustment(
            data,
            outer.start,
            end,
            &AtomicSizes::EMPTY,
            cx,
            sat,
        )
    };
    cx.begin_reshape_operation();
    let warm_up = probe(&mut cx, &mut sat, outer.start + 2);
    let baseline = cx.ruby_column_visits;
    cx.begin_reshape_operation();
    let first = probe(&mut cx, &mut sat, outer.start + 2);
    let measured = cx.ruby_column_visits - baseline;
    assert!(measured > 0);
    let second = probe(&mut cx, &mut sat, outer.start + 3);
    assert_eq!(
        cx.ruby_column_visits - baseline,
        measured,
        "same through must replay"
    );
    cx.begin_reshape_operation();
    let third = probe(&mut cx, &mut sat, outer.start + 3);
    assert_eq!(
        cx.ruby_column_visits - baseline,
        2 * measured,
        "a new operation measures again"
    );
    let mut reference = context(true);
    let mut ref_sat = Saturation::default();
    reference.begin_reshape_operation();
    let ref_warm_up = probe(&mut reference, &mut ref_sat, outer.start + 2);
    let mut expected = Vec::new();
    for (end, new_operation) in [
        (outer.start + 2, true),
        (outer.start + 3, false),
        (outer.start + 3, true),
    ] {
        if new_operation {
            reference.begin_reshape_operation();
        }
        expected.push(probe(&mut reference, &mut ref_sat, end));
    }
    assert_eq!(warm_up, ref_warm_up);
    assert_eq!(vec![first, second, third], expected);
    assert_eq!(sat, ref_sat);
}

/// Width and scalar measurement calls grow linearly with nesting depth once
/// the core is memoized (the walk counter is guarded by
/// `incremental_walk_keeps_nested_probes_linear`).
#[test]
fn memoized_nested_candidates_measure_linearly() {
    let picks: [Pick; 3] = [PICKS[0], PICKS[1], PICKS[2]];
    for (path, measure) in PATHS {
        for (what, pick) in picks {
            let (all, growth) = ratios(measure, false, pick);
            assert!(
                growth.iter().all(|g| *g <= 2.6),
                "{path} {what}: {growth:?} {all:?}"
            );
        }
    }
}

type CrossingRun = (Vec<LayoutUnit>, Vec<Warning>, u64, Saturation);

/// Review focus 1: a core recorded within budget must be recomputed once a
/// replayed charge would cross the operation's limit.
#[test]
fn memo_recomputes_when_replay_would_cross_reshape_budget() {
    let window = 8;
    let limit = window * RESHAPE_WINDOWS;
    let p = arabic(&limits(Some(window), None), Direction::Ltr);
    let data = &p.data;
    let ruby = &data.ruby.containers[0];
    let start = ruby.units.start;
    // Both probes look ahead to the first paired cut after `start`, so they
    // share one memo key (a probe ending at the cut has no look-ahead and is
    // never stored).
    let cut = ruby.cuts[1].unit;
    assert!(cut - 2 > start);
    for end in [cut - 2, cut - 1] {
        let (through, _) = crate::ruby::measure::walk(data, start, end, &mut LayoutContext::new());
        assert_eq!(through, cut, "{start}..{end} must look ahead to the cut");
    }
    let run = |reference: bool, preset: u64| -> (CrossingRun, u64, usize, usize) {
        let mut cx = context(reference);
        cx.warnings.set_max(data.limits.max_warnings);
        let mut sat = Saturation::default();
        // Warm-up: fill the range caches in an earlier operation (kept from
        // before shodo-tj5, when cache fills were never memoized).
        cx.begin_reshape_operation();
        crate::ruby::measure::candidate_adjustment(
            data,
            start,
            cut - 2,
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut sat,
        );
        cx.begin_reshape_operation();
        let refusals = cx.ruby_replay_refusals;
        let first = crate::ruby::measure::candidate_adjustment(
            data,
            start,
            cut - 2,
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut sat,
        );
        let charged = cx.edge_reshape_spent;
        cx.edge_reshape_spent = preset;
        let columns = cx.ruby_column_visits;
        let second = crate::ruby::measure::candidate_adjustment(
            data,
            start,
            cut - 1,
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut sat,
        );
        (
            (
                vec![first, second],
                cx.warnings.as_slice().to_vec(),
                cx.edge_reshape_spent,
                sat,
            ),
            charged,
            cx.ruby_column_visits - columns,
            cx.ruby_replay_refusals - refusals,
        )
    };
    let (_, charged, _, _) = run(true, 0);
    assert!(charged > 0, "the fixture must charge the reshape budget");
    // Replay stays within the limit: reused without measuring.
    let (optimized, _, measured, refused) = run(false, 0);
    assert_eq!(optimized, run(true, 0).0);
    assert_eq!((measured, refused), (0, 0));
    // Replay would cross the limit: refused, measured again, and warns like
    // the reference.
    let (optimized, _, measured, refused) = run(false, limit - 1);
    let (reference, _, _, _) = run(true, limit - 1);
    assert_eq!(optimized, reference);
    assert!(measured > 0);
    assert!(refused > 0);
    assert!(
        reference
            .1
            .iter()
            .any(|w| w.message == "line edge reshape budget exceeded; keeping shared glyphs")
    );
}

/// `words` small cursive words, then a ruby with cursive bases. Inside a
/// word every unit is a prohibited break, which a fitting scan never probes
/// with an edge window, while `PartialLine::index` measures the edge windows
/// of every unit: the index charges far more of the reshape budget.
pub(super) fn cursive_words(limits: &Limits, words: usize) -> Paragraph {
    let mut b = ParagraphBuilder::new(&paragraph_style(false), limits);
    b.open_inline(NodeId(3), &style(6.0), Default::default());
    b.push_text(
        TextSource::Generated { node: NodeId(1) },
        &"بببببب ".repeat(words),
    );
    b.close_inline();
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
    finish(b)
}

/// Review focus 2: a speculative `PartialLine::index` that exhausts the
/// budget is rolled back (warnings and saturation, not spent bytes or memo
/// entries); the retried line must match the reference.
///
/// With a 12-byte window (limit 768 bytes) the wide scan of either fixture
/// stays within budget and is retained. The 999 retry indexes it: six words
/// fit, so the index serves the 998 retry too; eight words have the same
/// windows but cross the limit during the index, which is rolled back (its
/// warning with it), so the 998 retry has to index again.
#[test]
fn narrow_retry_after_budget_exhausting_index_matches_reference() {
    struct Run {
        out: Vec<String>,
        /// `cache_prepare_visits` per width.
        indexed: Vec<usize>,
        /// Edge windows shaped during the 999 retry.
        shaped: usize,
        /// Memo hits during the 998 retry, which follows the 999 retry's
        /// speculative index (rolled back for eight words).
        retry_hits: usize,
    }
    let observe = |p: &Paragraph, reference: bool| {
        let mut cx = context(reference);
        let mut run = Run {
            out: Vec::new(),
            indexed: Vec::new(),
            shaped: 0,
            retry_hits: 0,
        };
        for width in [1000.0f32, 999.0, 998.0, 120.0] {
            cx.cache_prepare_visits = 0;
            let (shaped, hits) = (
                p.data
                    .edge_shape_calls
                    .load(std::sync::atomic::Ordering::Relaxed),
                cx.ruby_memo_hits,
            );
            let result = p.next_line(
                &mut cx,
                p.start_token(),
                &LineOptions::default(),
                &LineConstraint::new(width),
                &AtomicSizes::EMPTY,
            );
            if width == 999.0 {
                run.shaped = p
                    .data
                    .edge_shape_calls
                    .load(std::sync::atomic::Ordering::Relaxed)
                    - shaped;
            }
            if width == 998.0 {
                run.retry_hits = cx.ruby_memo_hits - hits;
            }
            run.indexed.push(cx.cache_prepare_visits);
            run.out.push(result_signature(result));
            run.out.push(format!("{:?}", cx.take_warnings()));
        }
        run
    };
    for window in [8, 12, 16, 64] {
        for words in [6, 8] {
            let p = cursive_words(&limits(Some(window), None), words);
            let optimized = observe(&p, false);
            assert_eq!(
                optimized.out,
                observe(&p, true).out,
                "window {window} words {words}"
            );
            if window != 12 {
                continue;
            }
            assert_eq!(optimized.out[1], "[]", "the wide scan must be retained");
            assert!(optimized.indexed[1] > 0, "the 999 retry must index");
            assert!(optimized.shaped > 0, "the index must charge edge windows");
            if words == 6 {
                assert_eq!(optimized.indexed[2], 0, "a clean index serves 998");
            } else {
                assert!(optimized.indexed[2] > 0, "the rolled-back index is redone");
                assert!(
                    optimized.retry_hits > 0,
                    "the retry after the rolled-back index must replay memo entries"
                );
            }
        }
    }
}

/// Review focus 4: the min- and max-content atomics built by
/// `intrinsic_sizes` get distinct memo keys for the same range, so they can
/// never answer each other's probes. A hit pattern alone cannot show this: a
/// changed revision also moves the range cache epoch
/// (`RangeCache::begin`), which already refuses every older entry.
#[test]
fn intrinsic_min_and_max_atomics_have_distinct_memo_keys() {
    let p = atomic_base(&Limits::default(), true);
    let data = &p.data;
    let mut inputs = AtomicIntrinsics::default();
    inputs.insert_atomic(
        NodeId(99),
        AtomicIntrinsic {
            min_content: 4.0,
            max_content: 80.0,
        },
    );
    let mut cx = LayoutContext::new();
    let mut sat = Saturation::default();
    let (min, max) = crate::line::intrinsic::ruby_atomics(&inputs, &mut cx, &mut sat);
    let key = crate::ruby::memo::MemoKey::new;
    let n = data.units.len();
    for start in 0..n {
        for through in start + 1..=n {
            assert_ne!(
                key(data, &min, start, through),
                key(data, &max, start, through),
                "{start}..{through}"
            );
        }
    }
    // Each pass builds its atomics afresh: the next pass's keys differ too.
    let (next_min, next_max) = crate::line::intrinsic::ruby_atomics(&inputs, &mut cx, &mut sat);
    let all = [&min, &max, &next_min, &next_max].map(|atomics| key(data, atomics, 0, n));
    for i in 0..all.len() {
        for j in i + 1..all.len() {
            assert_ne!(all[i], all[j], "{i} {j}");
        }
    }
    // Without atomics both sets are empty and equal: sharing a key is exact.
    let (empty_min, empty_max) =
        crate::line::intrinsic::ruby_atomics(&AtomicIntrinsics::default(), &mut cx, &mut sat);
    assert_eq!(empty_min, empty_max);
}

/// Review focus 4, end to end: `intrinsic_sizes` with min/max atomics matches
/// the reference, and look-ahead probes of one range under both revisions in
/// one operation replay only within a revision.
///
/// `intrinsic_sizes` probes only at legal breaks (`through == end`), which
/// are never stored, so the second half probes a look-ahead range directly.
/// A changed revision resets the range caches (`RangeCache::begin`, a new
/// epoch), so the first probe after a switch is measured and recorded, and
/// the next two hit. Key separation itself is shown by
/// `intrinsic_min_and_max_atomics_have_distinct_memo_keys`.
#[test]
fn intrinsic_min_and_max_atomics_keep_separate_memo_entries() {
    let p = atomic_base(&Limits::default(), true);
    let mut inputs = AtomicIntrinsics::default();
    inputs.insert_atomic(
        NodeId(99),
        AtomicIntrinsic {
            min_content: 4.0,
            max_content: 80.0,
        },
    );
    let run = |reference: bool| {
        let mut cx = context(reference);
        let sizes = p.intrinsic_sizes(&mut cx, &LineOptions::default(), &inputs);
        (format!("{sizes:?}"), cx.take_warnings(), sizes)
    };
    let (optimized, warnings, sizes) = run(false);
    let (reference, reference_warnings, _) = run(true);
    assert_eq!((optimized, warnings), (reference, reference_warnings));
    assert!(sizes.max_content > sizes.min_content);

    let data = &p.data;
    let units = data.ruby.containers[0].units.clone();
    // The range must cover the atomic (so min and max differ) and look ahead.
    let end = units.end - 1;
    let (through, _) =
        crate::ruby::measure::walk(data, units.start, end, &mut LayoutContext::new());
    assert!(through > end, "{}..{end} must look ahead", units.start);
    let (min, max) = (sized(4.0), sized(80.0));
    let probes = |reference: bool| {
        let mut cx = context(reference);
        let mut sat = Saturation::default();
        let mut probe = |cx: &mut LayoutContext, atomics: &AtomicSizes| {
            let hits = cx.ruby_memo_hits;
            let value = crate::ruby::measure::candidate_adjustment(
                data,
                units.start,
                end,
                atomics,
                cx,
                &mut sat,
            );
            (value, cx.ruby_memo_hits - hits)
        };
        cx.begin_reshape_operation();
        let out: Vec<_> = [&min, &min, &min, &max, &max, &max, &min]
            .into_iter()
            .map(|atomics| probe(&mut cx, atomics))
            .collect();
        (out, sat)
    };
    let (optimized, sat) = probes(false);
    let (reference, ref_sat) = probes(true);
    let values = |run: &[(LayoutUnit, usize)]| run.iter().map(|(v, _)| *v).collect::<Vec<_>>();
    assert_eq!((values(&optimized), sat), (values(&reference), ref_sat));
    assert_ne!(optimized[0].0, optimized[3].0, "min and max must differ");
    let hits: Vec<_> = optimized.iter().map(|(_, hits)| *hits).collect();
    // A revision switch clears the range caches (a new epoch), so the
    // entries of the earlier revision are measured again; within a
    // revision every repeat replays, including the one after the probe
    // that filled the caches (shodo-tj5).
    assert_eq!(
        hits,
        [0, 1, 1, 0, 1, 1, 0],
        "repeats hit within a revision; a revision switch measures again"
    );
}

/// Review focus 5: entries never outlive their operation, and a changed
/// atomic revision never hits an older entry.
#[test]
fn memo_does_not_survive_operations_or_atomic_revisions() {
    let p = atomic_base(&Limits::default(), false);
    let (small, large) = (sized(4.0), sized(80.0));
    let order = [&small, &large, &small];
    let mut cx = LayoutContext::new();
    let shared: Vec<_> = order
        .iter()
        .map(|atomics| {
            result_signature(p.next_line(
                &mut cx,
                p.start_token(),
                &LineOptions::default(),
                &LineConstraint::new(1000.0),
                atomics,
            ))
        })
        .collect();
    let fresh: Vec<_> = order
        .iter()
        .map(|atomics| {
            result_signature(p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &LineOptions::default(),
                &LineConstraint::new(1000.0),
                atomics,
            ))
        })
        .collect();
    assert_eq!(shared, fresh);
    assert_ne!(shared[0], shared[1]);
    assert!(
        cx.ruby_memo_hits > 0,
        "the shared context must reuse memo entries"
    );
}

/// shodo-tj5: a measurement that fills the range caches is memoized. A
/// `blocks` hit replays the block's reshape charges, so the cold recording
/// charges what measuring again would (`siblings` charged 2952 bytes cold
/// against 2808 warm before), and the next probe replays it.
#[test]
fn cold_cache_fills_are_memoized() {
    let p = siblings(&Limits::default());
    let data = &p.data;
    // End inside the last sibling, so the probe looks ahead to its end (a
    // probe without look-ahead is never stored).
    let last = data
        .ruby
        .containers
        .iter()
        .max_by_key(|ruby| ruby.units.start)
        .unwrap()
        .units
        .clone();
    let end = last.start + 1;
    let (through, _) = crate::ruby::measure::walk(data, 0, end, &mut LayoutContext::new());
    assert!(through > end, "0..{end} must look ahead");
    let mut cx = LayoutContext::new();
    let mut sat = Saturation::default();
    cx.begin_reshape_operation();
    let mut probe = |cx: &mut LayoutContext| {
        let columns = cx.ruby_column_visits;
        let hits = cx.ruby_memo_hits;
        let value = crate::ruby::measure::candidate_adjustment(
            data,
            0,
            end,
            &AtomicSizes::EMPTY,
            cx,
            &mut sat,
        );
        (
            value,
            cx.ruby_column_visits - columns,
            cx.ruby_memo_hits - hits,
        )
    };
    let spent = |cx: &mut LayoutContext, probe: &mut dyn FnMut(&mut LayoutContext) -> _| {
        let before = cx.edge_reshape_spent;
        let out: (LayoutUnit, usize, usize) = probe(cx);
        (out, cx.edge_reshape_spent - before)
    };
    let ((cold, measured, hits), cold_spent) = spent(&mut cx, &mut probe);
    assert!(measured > 0 && hits == 0);
    let ((replayed, measured, hits), replayed_spent) = spent(&mut cx, &mut probe);
    assert_eq!((measured, hits), (0, 1), "a cold recording is replayed");
    assert_eq!((cold, cold_spent), (replayed, replayed_spent));
    let pre = PreState {
        spent: 0,
        suppressed: false,
    };
    let (optimized, hits) = observe_candidates(&p, &AtomicSizes::EMPTY, false, pre);
    let (reference, _) = observe_candidates(&p, &AtomicSizes::EMPTY, true, pre);
    assert!(hits > 0);
    assert_eq!(
        (optimized.spent, &optimized.warnings, optimized.sat),
        (reference.spent, &reference.warnings, reference.sat)
    );
    assert_eq!(optimized, reference);
}

fn walk_fixtures() -> Vec<(String, Paragraph)> {
    let default = Limits::default();
    let mut out = vec![
        (
            "nested4".to_string(),
            nested(4, &default, "日", &style(24.0)),
        ),
        (
            "nested-anywhere".to_string(),
            nested(4, &default, "日本語", &anywhere(24.0)),
        ),
        ("siblings".to_string(), siblings(&default)),
        ("arabic".to_string(), arabic(&default, Direction::Rtl)),
    ];
    // Outer container with two bases, each holding its own nested ruby, and
    // several paired cuts: clipped containers interleave with unclipped ones.
    let inner = |node: u64| {
        let mut b = ParagraphBuilder::new(&paragraph_style(false), &default);
        b.push_ruby(
            NodeId(node),
            &anywhere(24.0),
            annotated(
                vec![base_text(node + 1, "日本語", &anywhere(24.0), &default)],
                &["にほんご"],
                RubyOverhang::None,
                &default,
            ),
        );
        RubyContent::from_builder(b)
    };
    let mut b = ParagraphBuilder::new(&paragraph_style(false), &default);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
    b.push_ruby(
        NodeId(100),
        &anywhere(24.0),
        annotated(
            vec![
                inner(300),
                inner(400),
                base_text(32, "本日", &anywhere(24.0), &default),
            ],
            &["に", "ほん", "ご"],
            RubyOverhang::None,
            &default,
        ),
    );
    b.push_text(TextSource::Generated { node: NodeId(2) }, "語");
    out.push(("multi-base".to_string(), finish(b)));
    // Review focus 3: the first-line alternate dataset uses source-matched
    // cuts, which may omit a container's own end cut.
    let first_line = atomic_base(&default, true);
    out.push(("first-line normal".to_string(), first_line.clone()));
    assert!(first_line.data.first_line.is_some());
    let alternate = Paragraph {
        data: std::sync::Arc::clone(&first_line.data.first_line.as_ref().unwrap().data),
    };
    out.push(("first-line alternate".to_string(), alternate));
    out
}

#[test]
fn incremental_walk_matches_full_walk_for_every_range() {
    for (name, p) in walk_fixtures() {
        let data = &p.data;
        assert!(!data.ruby.containers.is_empty(), "{name}");
        let n = data.units.len();
        let full =
            |start, end| crate::ruby::measure::walk(data, start, end, &mut LayoutContext::new());
        for start in 0..n {
            let mut state = None;
            for end in start + 1..=n {
                let through = crate::ruby::memo::advance(&mut state, data, start, end);
                let (expected, visited) = full(start, end);
                assert_eq!(
                    (through, state.as_ref().unwrap().visited()),
                    (expected, &visited[..]),
                    "{name}: {start}..{end}"
                );
            }
            // A shrinking end restarts from a full walk.
            let through = crate::ruby::memo::advance(&mut state, data, start, start + 1);
            let (expected, visited) = full(start, start + 1);
            assert_eq!(
                (through, state.as_ref().unwrap().visited()),
                (expected, &visited[..]),
                "{name}: restart at {start}"
            );
            // A changed start restarts from a full walk as well.
            if start + 2 <= n {
                let through = crate::ruby::memo::advance(&mut state, data, start + 1, n);
                let (expected, visited) = full(start + 1, n);
                assert_eq!(
                    (through, state.as_ref().unwrap().visited()),
                    (expected, &visited[..]),
                    "{name}: changed start {}",
                    start + 1
                );
            }
        }
    }
}

#[test]
fn incremental_walk_keeps_nested_probes_linear() {
    for (path, measure) in PATHS {
        let (all, growth) = ratios(measure, false, PICKS[3].1);
        assert!(
            growth.iter().all(|g| *g <= 2.6),
            "{path} walk: {growth:?} {all:?}"
        );
    }
}

/// One two-base ruby followed by `n` plain CJK units: every probe past the
/// ruby has `through == end`.
fn one_ruby_then(n: usize) -> Paragraph {
    let default = Limits::default();
    let mut b = ParagraphBuilder::new(&paragraph_style(false), &default);
    b.push_ruby(
        NodeId(100),
        &style(24.0),
        annotated(
            vec![
                base_text(30, "日本", &style(24.0), &default),
                base_text(31, "語", &style(24.0), &default),
            ],
            &["にほん"],
            RubyOverhang::None,
            &default,
        ),
    );
    b.push_text(TextSource::Generated { node: NodeId(1) }, &"日".repeat(n));
    finish(b)
}

/// Entries are stored only for look-ahead probes (`through > end`), so a long
/// paragraph with one ruby keeps a handful of entries however long it is,
/// while nested and sibling rubies still replay their look-ahead cores.
#[test]
fn memo_stays_bounded_on_long_paragraphs() {
    let mut seen = Vec::new();
    for n in [1000, 4000, 16000] {
        let p = one_ruby_then(n);
        let mut cx = LayoutContext::new();
        p.intrinsic_sizes(
            &mut cx,
            &LineOptions::default(),
            &AtomicIntrinsics::default(),
        );
        let intrinsic = (cx.ruby_memo.len(), cx.ruby_memo.capacity());
        // `break_all` ends with an operation that finds no line and clears
        // the memo, so observe the single line's operation directly.
        let _ = p.next_line(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            &LineConstraint::new(1.0e6),
            &AtomicSizes::EMPTY,
        );
        let one_line = (cx.ruby_memo.len(), cx.ruby_memo.capacity());
        println!("n {n}: intrinsic {intrinsic:?} one line {one_line:?}");
        seen.push((n, intrinsic, one_line));
    }
    for (n, intrinsic, one_line) in seen {
        for (what, (len, capacity)) in [("intrinsic", intrinsic), ("one line", one_line)] {
            assert!(
                len <= 16 && capacity <= crate::ruby::memo::RETAINED_CAPACITY,
                "{what} n {n}: len {len} capacity {capacity}"
            );
        }
    }
}

/// `n` two-base sibling rubies: a one-line scan has about three look-ahead
/// endpoints per ruby, so 400 rubies overflow `MAX_ENTRIES`.
pub(super) fn many_siblings(n: usize) -> Paragraph {
    let default = Limits::default();
    let mut b = ParagraphBuilder::new(&paragraph_style(false), &default);
    for i in 0..n as u64 {
        b.push_ruby(
            NodeId(1000 + i),
            &style(24.0),
            annotated(
                vec![
                    base_text(30, "日本語", &style(24.0), &default),
                    base_text(31, "日本", &style(24.0), &default),
                ],
                &["にほんご"],
                RubyOverhang::None,
                &default,
            ),
        );
    }
    finish(b)
}

/// The memo bounds keep the look-ahead reuse: nested and sibling rubies still
/// replay, a one-line scan replays the same number of cores whether or not
/// it overflows `MAX_ENTRIES` (each look-ahead endpoint is asked for over one
/// contiguous run of ends), and the overflowing operation's map is released.
#[test]
fn memo_bounds_keep_look_ahead_hits() {
    let line = |p: &Paragraph, cx: &mut LayoutContext| {
        let _ = p.next_line(
            cx,
            p.start_token(),
            &LineOptions::default(),
            &LineConstraint::new(1.0e7),
            &AtomicSizes::EMPTY,
        );
    };
    for (name, p) in [
        ("nested", nested(16, &Limits::default(), "日", &style(24.0))),
        ("siblings", siblings(&Limits::default())),
    ] {
        let mut cx = LayoutContext::new();
        line(&p, &mut cx);
        assert!(cx.ruby_memo_hits > 0, "{name}");
        let mut cx = LayoutContext::new();
        p.break_all(&mut cx, &LineOptions::default(), 96.0, &AtomicSizes::EMPTY);
        assert!(cx.ruby_memo_hits > 0, "{name} break_all");
    }
    for (n, overflows) in [(100, false), (400, true)] {
        let p = many_siblings(n);
        let mut cx = LayoutContext::new();
        line(&p, &mut cx);
        assert_eq!(cx.ruby_memo.overflow_clears > 0, overflows, "n {n}");
        // 16 per sibling before shodo-tj5, when probes that filled the range
        // caches were not stored.
        assert_eq!(cx.ruby_memo_hits, 19 * n, "n {n}");
        assert!(cx.ruby_memo.len() <= crate::ruby::memo::MAX_ENTRIES);
        cx.begin_reshape_operation();
        assert!(cx.ruby_memo.capacity() <= crate::ruby::memo::RETAINED_CAPACITY);
    }
}
#[test]
#[ignore = "report for the shodo-d77 record"]
fn d77_operation_counts_report() {
    for (path, measure) in PATHS {
        for reference in [true, false] {
            for depth in [8, 16, 32, 64] {
                let c = measure(depth, reference);
                println!(
                    "{{\"path\":\"{path}\",\"reference\":{reference},\"depth\":{depth},\"width_calls\":{},\"scalar_calls\":{},\"columns\":{},\"walk\":{}}}",
                    c.width_calls, c.scalar_calls, c.columns, c.walk
                );
            }
        }
    }
}

/// A probe ending at a container's end has no look-ahead and is not stored,
/// but it still replays the entry that the look-ahead probe one unit earlier
/// recorded under the same `through`: every single-base sibling of an
/// unbreakable line replays 13 cores (12 before shodo-tj5, when the probe
/// that filled the range caches was not stored).
#[test]
fn exact_probes_replay_look_ahead_entries() {
    let default = Limits::default();
    for r in [25usize, 50] {
        let mut b = ParagraphBuilder::new(&paragraph_style(false), &default);
        for i in 0..r as u64 {
            b.push_ruby(
                NodeId(1000 + i),
                &style(24.0),
                annotated(
                    vec![base_text(30, "12", &style(24.0), &default)],
                    &["日"],
                    RubyOverhang::None,
                    &default,
                ),
            );
        }
        let p = finish(b);
        let mut cx = LayoutContext::new();
        p.break_all(&mut cx, &LineOptions::default(), 96.0, &AtomicSizes::EMPTY);
        assert_eq!(cx.ruby_memo_hits, 13 * r, "r {r}");
    }
}
