//! Source-preserving CSS ruby input and formatting.
pub(crate) mod builder;
pub(crate) mod input;
pub(crate) mod pairing;
pub use input::*;

#[cfg(test)]
#[path = "tests/input.rs"]
mod input_tests;
