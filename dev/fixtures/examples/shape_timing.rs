//! Fixed-corpus build timings. Plan/scratch retention is per context;
//! font loading and registration are excluded from all measurements.
use shodo::{LayoutContext, limits::Limits};
use shodo_fixtures::{cases, load_fonts};
use std::{hint::black_box, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let limits = Limits::default();
    let fonts = load_fonts(&limits)?;
    let mut cx = LayoutContext::new();
    let build = |cx: &mut LayoutContext| -> Result<(), Box<dyn std::error::Error>> {
        for case in cases() {
            black_box(case.build(cx, &fonts, &limits)?);
        }
        Ok(())
    };
    let start = Instant::now();
    build(&mut cx)?;
    let cold = start.elapsed();
    let start = Instant::now();
    for _ in 0..100 {
        build(&mut cx)?;
    }
    let warm = start.elapsed() / 100;
    cx.shrink_to(0);
    let start = Instant::now();
    build(&mut cx)?;
    println!(
        "{} cases: first={cold:?}, retained-average={warm:?}, after-shrink={:?}",
        cases().len(),
        start.elapsed()
    );
    Ok(())
}
