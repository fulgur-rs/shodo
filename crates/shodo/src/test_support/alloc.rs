//! Per-thread heap accounting for tests that check `Limits` byte
//! models and cache retention against real allocations.
//!
//! `shodo-bench`'s `CountingAllocator` keeps process-global counters and allows
//! one scope at a time, which parallel unit tests would corrupt. These counters
//! are thread-local, so each test observes only its own thread.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

std::thread_local! {
    // Signed: a thread may free memory another thread allocated (the test
    // harness does), which takes its balance below zero. Only differences
    // between readings are meaningful.
    static LIVE: Cell<i64> = const { Cell::new(0) };
    static PEAK: Cell<i64> = const { Cell::new(0) };
}

fn grow(bytes: usize) {
    // `try_with` fails only during thread teardown; such allocations are ignored.
    let _ = LIVE.try_with(|live| {
        let next = live.get().wrapping_add(bytes as i64);
        live.set(next);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(next)));
    });
}

fn shrink(bytes: usize) {
    let _ = LIVE.try_with(|live| live.set(live.get().wrapping_sub(bytes as i64)));
}

/// Delegates to `System`, counting requested sizes of successful calls.
struct ThreadCounting;

// SAFETY: every call forwards the caller's pointer and layout to `System`
// unchanged. Counting touches only const-initialized thread-local `Cell<i64>`s,
// which never allocate, panic or run destructors.
unsafe impl GlobalAlloc for ThreadCounting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grow(layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            grow(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) };
        shrink(layout.size());
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(p, layout, size) };
        if !next.is_null() {
            // A null result leaves the original block, and the counters, unchanged.
            if size >= layout.size() {
                grow(size - layout.size());
            } else {
                shrink(layout.size() - size);
            }
        }
        next
    }
}

#[global_allocator]
static ALLOCATOR: ThreadCounting = ThreadCounting;

/// This thread's allocated-minus-freed balance. Compare two readings; the
/// absolute value includes memory moved between threads.
pub(crate) fn live_bytes() -> i64 {
    LIVE.with(Cell::get)
}

/// Runs `f` and returns its result with the peak of this thread's live heap
/// above the level at entry. Memory `f` frees that was allocated before entry
/// can make the live level drop below it; the peak never goes below zero.
pub(crate) fn measure_peak<R>(f: impl FnOnce() -> R) -> (R, u64) {
    let start = live_bytes();
    PEAK.with(|peak| peak.set(start));
    let result = f();
    let peak = PEAK.with(Cell::get);
    (result, peak.saturating_sub(start).max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_counts_this_threads_transient_and_retained_allocations() {
        let (kept, peak) = measure_peak(|| {
            drop(std::hint::black_box(vec![0u8; 4096]));
            std::hint::black_box(vec![0u8; 1024])
        });
        assert!(peak >= 4096, "transient block not seen: {peak}");
        assert!(peak < 4096 + 1024 + 512, "unexpected allocations: {peak}");
        let before = live_bytes();
        drop(kept);
        assert_eq!(before - live_bytes(), 1024);
    }

    #[test]
    fn peak_is_relative_when_the_balance_is_negative() {
        // Freeing memory allocated on another thread drives the balance negative.
        let foreign = std::thread::spawn(|| vec![0u8; 1 << 20]).join().unwrap();
        drop(foreign);
        assert!(live_bytes() < 0);
        let (_, peak) = measure_peak(|| drop(std::hint::black_box(vec![0u8; 4096])));
        assert!(peak >= 4096, "transient block not seen below zero: {peak}");
    }
}
