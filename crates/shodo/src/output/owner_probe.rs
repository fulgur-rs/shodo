//! Actual root strong-reference peak at Line construction, not geometry bytes.
use std::cell::Cell;
use std::sync::Arc;

use crate::paragraph::ParagraphData;

thread_local! {
    static STATE: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

pub(crate) fn record(data: &Arc<ParagraphData>) {
    STATE.with(|state| {
        let (owner, peak) = state.get();
        if owner == Arc::as_ptr(data) as usize {
            state.set((owner, peak.max(Arc::strong_count(data))));
        }
    });
}

pub(crate) fn measure<T>(data: &Arc<ParagraphData>, f: impl FnOnce() -> T) -> (T, usize) {
    struct Reset((usize, usize));
    impl Drop for Reset {
        fn drop(&mut self) {
            STATE.with(|state| state.set(self.0));
        }
    }
    let reset = Reset(STATE.with(|state| state.replace((Arc::as_ptr(data) as usize, 0))));
    let result = f();
    let peak = STATE.with(|state| state.get().1);
    drop(reset);
    (result, peak)
}
