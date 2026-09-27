//! Allocation-visible requested bytes, separate from timing builds and RSS.
use serde::Serialize;
use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct AllocationCounts {
    pub calls: u64,
    pub allocated_bytes: u64,
    pub deallocated_bytes: u64,
    pub start_live_bytes: u64,
    pub live_bytes: u64,
    pub peak_extra_bytes: u64,
    pub net_bytes: i128,
}
struct Counters {
    calls: AtomicU64,
    allocated: AtomicU64,
    freed: AtomicU64,
    live: AtomicU64,
    peak: AtomicU64,
    active: AtomicBool,
}
impl Counters {
    const fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
            allocated: AtomicU64::new(0),
            freed: AtomicU64::new(0),
            live: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            active: AtomicBool::new(false),
        }
    }
    fn acquire(&self, size: usize) {
        let size = size as u64;
        self.calls.fetch_add(1, Relaxed);
        self.allocated.fetch_add(size, Relaxed);
        let live = self.live.fetch_add(size, Relaxed).wrapping_add(size);
        self.peak.fetch_max(live, Relaxed);
    }
    fn release(&self, size: usize) {
        let size = size as u64;
        self.freed.fetch_add(size, Relaxed);
        self.live.fetch_sub(size, Relaxed);
    }
    fn resize(&self, old: usize, new: usize) {
        self.calls.fetch_add(1, Relaxed);
        self.allocated.fetch_add(new as u64, Relaxed);
        self.freed.fetch_add(old as u64, Relaxed);
        let live = if new >= old {
            self.live
                .fetch_add((new - old) as u64, Relaxed)
                .wrapping_add((new - old) as u64)
        } else {
            self.live
                .fetch_sub((old - new) as u64, Relaxed)
                .wrapping_sub((old - new) as u64)
        };
        self.peak.fetch_max(live, Relaxed);
    }
}
/// Delegates unchanged layouts to the underlying allocator. Counters never
/// allocate, format, lock or unwind. Scopes require a single-threaded probe.
pub struct CountingAllocator<A> {
    inner: A,
    counters: Counters,
}
impl<A> CountingAllocator<A> {
    pub const fn new(inner: A) -> Self {
        Self {
            inner,
            counters: Counters::new(),
        }
    }
    pub fn begin(&self) -> Result<Scope<'_>, ScopeActive> {
        self.counters
            .active
            .compare_exchange(false, true, Relaxed, Relaxed)
            .map_err(|_| ScopeActive)?;
        let c = &self.counters;
        let live = c.live.load(Relaxed);
        c.peak.store(live, Relaxed);
        Ok(Scope {
            counters: c,
            calls: c.calls.load(Relaxed),
            allocated: c.allocated.load(Relaxed),
            freed: c.freed.load(Relaxed),
            live,
            finished: false,
        })
    }
}
#[derive(Debug)]
pub struct ScopeActive;
impl std::fmt::Display for ScopeActive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("allocation scope already active")
    }
}
impl std::error::Error for ScopeActive {}
pub struct Scope<'a> {
    counters: &'a Counters,
    calls: u64,
    allocated: u64,
    freed: u64,
    live: u64,
    finished: bool,
}
impl Scope<'_> {
    pub fn finish(mut self) -> AllocationCounts {
        let c = self.counters;
        let live = c.live.load(Relaxed);
        let counts = AllocationCounts {
            calls: c.calls.load(Relaxed).wrapping_sub(self.calls),
            allocated_bytes: c.allocated.load(Relaxed).wrapping_sub(self.allocated),
            deallocated_bytes: c.freed.load(Relaxed).wrapping_sub(self.freed),
            start_live_bytes: self.live,
            live_bytes: live,
            peak_extra_bytes: c.peak.load(Relaxed).saturating_sub(self.live),
            net_bytes: i128::from(live) - i128::from(self.live),
        };
        self.finished = true;
        c.active.store(false, Relaxed);
        counts
    }
}
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.counters.active.store(false, Relaxed);
        }
    }
}
// SAFETY: every operation delegates the original pointer/layout to A, preserving
// allocation ownership. Statistics observe successful calls only; callbacks
// contain solely nonpanicking atomic integer operations. Null realloc leaves
// the original block and its counters unchanged, per GlobalAlloc's contract.
unsafe impl<A: GlobalAlloc> GlobalAlloc for CountingAllocator<A> {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { self.inner.alloc(l) };
        if !p.is_null() {
            self.counters.acquire(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { self.inner.alloc_zeroed(l) };
        if !p.is_null() {
            self.counters.acquire(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { self.inner.dealloc(p, l) };
        self.counters.release(l.size());
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        let next = unsafe { self.inner.realloc(p, l, size) };
        if !next.is_null() {
            self.counters.resize(l.size(), size);
        }
        next
    }
}
