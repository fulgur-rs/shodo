use crate::font::{FontCollection, FontOptions};
use crate::limits::Limits;
use crate::node::{InlineEdges, NodeId};
use crate::style::{LineOptions, ParagraphStyle, TextWrapStyle};
use crate::{
    AtomicSize, AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
};

#[test]
fn balance_releases_each_trial_line_before_constructing_next() {
    // Keeping all greedy/trial Lines makes the same immutable root acquire one
    // owner per retained Line. Streamed count/end results need only fixed owners.
    let limits = Limits {
        max_balance_iterations: Some(1),
        ..Default::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register(crate::test_support::fonts::LATIN.to_vec())
        .unwrap();
    let style = ParagraphStyle::default();
    let mut builder = ParagraphBuilder::new(&style, &limits);
    let mut atomics = AtomicSizes::new();
    for id in 1..=32 {
        builder.push_atomic(NodeId(id), &style.root, InlineEdges::default());
        atomics.insert(
            NodeId(id),
            AtomicSize {
                inline_size: 16.,
                block_size: 10.,
                ..Default::default()
            },
        );
        if id < 32 {
            builder.push_forced_break(NodeId(100 + id));
        }
    }
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let options = LineOptions {
        text_wrap_style: TextWrapStyle::Balance,
        ..Default::default()
    };
    let mut cx = LayoutContext::new();
    crate::output::construction_probe::reset();
    crate::output::clone_probe::reset();
    let (plan, owner_peak) = crate::output::owner_probe::measure(&p.data, || {
        p.plan_breaks(&mut cx, &options, 32., &atomics)
    });
    assert_eq!(
        plan.ends.len(),
        32,
        "forced boundaries fix the greedy and trial count"
    );
    assert_eq!(
        crate::output::construction_probe::count(),
        64,
        "initial32 plus one complete32-line trial; post-scan is not short-circuited"
    );
    assert_eq!(crate::output::clone_probe::count(), 0);
    assert_eq!(
        cx.raw_scan_clones, 2,
        "one final scan per complete32-line pass"
    );
    assert!(cx.take_warnings().is_empty());
    assert!(owner_peak > 0, "actual constructor observation must run");
    assert!(
        owner_peak <= 4,
        "retained root owners at actual Line construction: {owner_peak}; trial geometry must be released incrementally"
    );
    eprintln!("actual root owner peak={owner_peak}; complete constructor count=64; clones=0");

    // Literal public output independently checks that all32 atomics still exist.
    let mut accepted = LayoutContext::new();
    let lines: Vec<_> = p
        .lines(
            &mut accepted,
            p.start_token(),
            &options,
            |_, offset| {
                let mut c = LineConstraint::new(32.);
                c.block_offset = offset;
                c.break_plan = Some(&plan);
                c
            },
            &atomics,
        )
        .filter_map(|r| {
            if let LineResult::Line(l) = r {
                Some(l)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(lines.len(), 32);
    for (i, line) in lines.iter().enumerate() {
        let nodes: Vec<_> = line
            .fragments()
            .filter_map(|f| {
                if let Fragment::Atomic(a) = f {
                    assert_eq!(a.border_rect.inline_size, 16.);
                    assert_eq!(a.border_rect.block_size, 10.);
                    Some(a.node.0)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(nodes, vec![i as u64 + 1]);
    }
    assert!(accepted.take_warnings().is_empty());
}

#[test]
fn infeasible_balance_trial_still_scans_tail_resource_warnings() {
    let limits = Limits {
        max_balance_iterations: Some(1),
        max_warnings: None,
        ..Default::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register(crate::test_support::fonts::LATIN.to_vec())
        .unwrap();
    let style = ParagraphStyle::default();
    let mut builder = ParagraphBuilder::new(&style, &limits);
    let mut atomics = AtomicSizes::new();
    for id in 1..=4 {
        builder.push_atomic(NodeId(id), &style.root, InlineEdges::default());
        atomics.insert(
            NodeId(id),
            AtomicSize {
                inline_size: 16.,
                block_size: 10.,
                ..Default::default()
            },
        );
    }
    builder.push_atomic(NodeId(99), &style.root, InlineEdges::default());
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let options = LineOptions {
        text_wrap_style: TextWrapStyle::Balance,
        ..Default::default()
    };
    // Four16px atoms need two32px lines or four16px lines. The final missing
    // atom has zero fallback width and still contributes a real ordered warning.
    let mut oracle = LayoutContext::new();
    assert_eq!(p.break_all(&mut oracle, &options, 32., &atomics).len(), 2);
    assert_eq!(p.break_all(&mut oracle, &options, 16., &atomics).len(), 4);
    let warnings = oracle.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.kind == crate::limits::WarningKind::MissingAtomicSize)
    );
    let mut cx = LayoutContext::new();
    crate::output::construction_probe::reset();
    let plan = p.plan_breaks(&mut cx, &options, 32., &atomics);
    assert_eq!(plan.ends.len(), 2);
    assert_eq!(
        crate::output::construction_probe::count(),
        6,
        "the infeasible four-line trial is fully evaluated after exceeding the two-line target"
    );
    assert_eq!(cx.take_warnings(), warnings);
}
