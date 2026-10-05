//! Source-preserving CSS ruby input and formatting.
//!
//! The [ruby integration guide] covers pairing input, coordinated wrapping,
//! annotation transforms, painting and dedicated annotation hit testing.
//!
//! [ruby integration guide]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/ruby.md
pub(crate) mod align;
pub(crate) mod base_budget;
pub(crate) mod budget;
pub(crate) mod builder;
pub(crate) mod cuts;
pub(crate) mod geometry;
pub(crate) mod hit;
pub use hit::RubyHit;
mod index;
pub(crate) mod input;
pub(crate) mod measure;
pub(crate) mod memo;
pub(crate) mod overhang;
pub(crate) mod pairing;
pub(crate) mod place;
pub(crate) mod prepare;
pub use input::*;

#[cfg(test)]
#[path = "tests/input.rs"]
mod input_tests;

#[cfg(test)]
#[path = "tests/preflight.rs"]
mod preflight_tests;

#[cfg(test)]
#[path = "tests/cuts.rs"]
mod cut_tests;

#[cfg(test)]
#[path = "tests/limits.rs"]
mod limit_tests;

#[cfg(test)]
#[path = "tests/prepared_cuts.rs"]
mod prepared_cut_tests;

#[cfg(test)]
#[path = "tests/measure.rs"]
mod measure_tests;

#[cfg(test)]
#[path = "tests/metrics.rs"]
mod metric_tests;

#[cfg(test)]
#[path = "tests/memo.rs"]
mod memo_tests;

#[cfg(test)]
#[path = "tests/accumulate.rs"]
mod accumulate_tests;
