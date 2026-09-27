use shodo::{LayoutContext, limits::Limits};
use shodo_bench::checked::{CheckedBuild, CheckedRun};
use shodo_bench::{Operation, Workload, digest, layout};
#[test]
fn deferred_validation_rejects_dropped_work_after_the_clock() {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let w = Workload::named("latin-short", 1).unwrap();
    let mut cx = LayoutContext::new();
    let ps = w.build(&mut cx, &fonts, &limits).unwrap();
    let mut run = layout(&w, &ps, &mut cx, &fonts, &limits, Operation::AllLines).unwrap();
    let expected = digest(&run, &fonts).unwrap();
    run.lines.clear();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(CheckedRun::new(
            run, &fonts, &expected
        ))))
        .is_err()
    );
}
#[test]
fn deferred_build_validation_uses_the_actual_built_paragraphs() {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let w = Workload::named("latin-short", 1).unwrap();
    let mut cx = LayoutContext::new();
    let ps = w.build(&mut cx, &fonts, &limits).unwrap();
    let expected = digest(
        &layout(&w, &ps, &mut cx, &fonts, &limits, Operation::AllLines).unwrap(),
        &fonts,
    )
    .unwrap();
    let different = Workload::named("latin-spacing", 1)
        .unwrap()
        .build(&mut cx, &fonts, &limits)
        .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(CheckedBuild::new(
            different, &w, &fonts, &limits, &expected
        ))))
        .is_err()
    );
}
#[test]
fn correct_deferred_outputs_validate_successfully() {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let w = Workload::named("japanese-short", 1).unwrap();
    let mut cx = LayoutContext::new();
    let ps = w.build(&mut cx, &fonts, &limits).unwrap();
    let run = layout(&w, &ps, &mut cx, &fonts, &limits, Operation::AllLines).unwrap();
    let expected = digest(&run, &fonts).unwrap();
    drop(CheckedRun::new(run, &fonts, &expected));
    drop(CheckedBuild::new(ps, &w, &fonts, &limits, &expected));
}
