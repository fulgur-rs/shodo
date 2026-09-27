use shodo_bench::allocator::CountingAllocator;
use std::alloc::{GlobalAlloc, Layout, System};

#[test]
fn raw_realloc_growth_shrink_and_drop_have_exact_scope_accounting() {
    let a = CountingAllocator::new(System);
    let scope = a.begin().unwrap();
    // SAFETY: every nonnull block is used with its current matching layout.
    unsafe {
        let p = a.alloc(Layout::from_size_align(64, 8).unwrap());
        assert!(!p.is_null());
        let p = a.realloc(p, Layout::from_size_align(64, 8).unwrap(), 128);
        assert!(!p.is_null());
        let p = a.realloc(p, Layout::from_size_align(128, 8).unwrap(), 32);
        assert!(!p.is_null());
        a.dealloc(p, Layout::from_size_align(32, 8).unwrap());
    }
    let s = scope.finish();
    assert_eq!(
        (
            s.calls,
            s.allocated_bytes,
            s.deallocated_bytes,
            s.net_bytes,
            s.peak_extra_bytes,
            s.live_bytes
        ),
        (3, 224, 224, 0, 128, 0)
    );
}

struct RefuseRealloc;
// SAFETY: normal allocation/deallocation delegates unchanged to System;
// a failed realloc preserves the caller's original allocation and layout.
unsafe impl GlobalAlloc for RefuseRealloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, _p: *mut u8, _l: Layout, _size: usize) -> *mut u8 {
        std::ptr::null_mut()
    }
}
#[test]
fn failed_realloc_preserves_the_live_old_block() {
    let a = CountingAllocator::new(RefuseRealloc);
    let scope = a.begin().unwrap();
    let l = Layout::from_size_align(64, 8).unwrap();
    // SAFETY: successful allocation is initialized/read within its size, and
    // null realloc leaves that block valid for deallocation with old layout.
    unsafe {
        let p = a.alloc(l);
        assert!(!p.is_null());
        p.write(7);
        assert!(a.realloc(p, l, 128).is_null());
        assert_eq!(p.read(), 7);
        let s = scope.finish();
        assert_eq!(
            (
                s.calls,
                s.allocated_bytes,
                s.live_bytes,
                s.net_bytes,
                s.peak_extra_bytes
            ),
            (1, 64, 64, 64, 64)
        );
        a.dealloc(p, l);
    }
}
#[test]
fn freeing_preexisting_ownership_reports_signed_negative_net() {
    let a = CountingAllocator::new(System);
    let l = Layout::from_size_align(64, 8).unwrap();
    // SAFETY: nonnull block always uses the exact matching layout.
    unsafe {
        let p = a.alloc(l);
        assert!(!p.is_null());
        let scope = a.begin().unwrap();
        a.dealloc(p, l);
        let s = scope.finish();
        assert_eq!(
            (
                s.calls,
                s.allocated_bytes,
                s.deallocated_bytes,
                s.net_bytes,
                s.peak_extra_bytes,
                s.live_bytes
            ),
            (0, 0, 64, -64, 0, 0)
        );
    }
}
#[test]
fn zeroed_allocation_and_exclusive_scopes_preserve_real_accounting() {
    let a = CountingAllocator::new(System);
    let scope = a.begin().unwrap();
    assert!(a.begin().is_err());
    let l = Layout::from_size_align(32, 8).unwrap();
    // SAFETY: nonnull zeroed block accessed only within size, then deallocated.
    unsafe {
        let p = a.alloc_zeroed(l);
        assert!(!p.is_null());
        assert_eq!(std::slice::from_raw_parts(p, 32), &[0; 32]);
        let s = scope.finish();
        assert_eq!((s.calls, s.allocated_bytes, s.net_bytes), (1, 32, 32));
        let scope = a.begin().unwrap();
        a.dealloc(p, l);
        assert_eq!(scope.finish().net_bytes, -32);
    }
}
#[test]
fn dropping_a_scope_releases_exclusivity_without_measuring_later_work() {
    let a = CountingAllocator::new(System);
    drop(a.begin().unwrap());
    let s = a.begin().unwrap().finish();
    assert_eq!(s.calls, 0);
    let l = Layout::from_size_align(16, 8).unwrap();
    // SAFETY: use exact matching layout for explicit observed allocation.
    unsafe {
        let p = a.alloc(l);
        assert!(!p.is_null());
        a.dealloc(p, l);
    }
    assert_eq!(s.calls, 0);
}
