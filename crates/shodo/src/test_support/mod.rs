//! Support for `src/` unit tests only. Not part of the published crate contract.

pub(crate) mod alloc;
pub(crate) mod fonts;

std::thread_local! {
    pub(crate) static GEOMETRY_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
