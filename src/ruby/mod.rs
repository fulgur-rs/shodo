//! Source-preserving CSS ruby input and formatting.
pub(crate) mod budget;
pub(crate) mod builder;
pub(crate) mod cuts;
mod index;
pub(crate) mod input;
pub(crate) mod pairing;
pub(crate) mod prepare;
pub use input::*;

#[cfg(test)]
#[path = "tests/input.rs"]
mod input_tests;

#[cfg(test)]
#[path = "tests/cuts.rs"]
mod cut_tests;

#[cfg(test)]
#[path = "tests/limits.rs"]
mod limit_tests;

#[cfg(test)]
#[path = "tests/prepared_cuts.rs"]
mod prepared_cut_tests;
