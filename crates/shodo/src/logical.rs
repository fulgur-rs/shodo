//! Target-independent byte model for fail-closed `Limits` charges.
//!
//! Exceeding a limit changes output, errors and warnings, so a charge must not
//! depend on the target's pointer width. Charges use each type's layout on
//! 64-bit targets: native outcomes keep their tuned values and 32-bit targets
//! (wasm32) are charged the same, more conservative, amounts.
//!
//! Types whose size differs by target are declared with [`logical_sizes!`],
//! which pins the 64-bit value and adds a 64-bit test comparing it with
//! `size_of`. A layout change then fails CI and becomes a deliberate change to
//! the limit model instead of silently shifting admission thresholds. Types
//! made only of fixed-width fields may keep using `size_of` directly.

/// A pointer-width slot (`usize`, a pointer) at its 64-bit size.
pub(crate) const WORD: u64 = 8;
/// A `Vec` or `String` header: pointer, capacity and length.
pub(crate) const VEC_HEADER: u64 = 3 * WORD;

/// Declares `const NAME: u64 = BYTES;` for each `NAME: Type = BYTES;` entry,
/// plus a 64-bit test pinning each value to `size_of::<Type>()`. Use at most
/// once per module.
macro_rules! logical_sizes {
    ($($vis:vis $name:ident: $t:ty = $bytes:literal;)+) => {
        $($vis const $name: u64 = $bytes;)+
        #[cfg(all(test, target_pointer_width = "64"))]
        #[test]
        fn logical_sizes_match_64_bit_layouts() {
            $(assert_eq!(
                std::mem::size_of::<$t>() as u64,
                $name,
                concat!("64-bit layout of ", stringify!($t), " changed; update the limit model"),
            );)+
        }
    };
}
pub(crate) use logical_sizes;
