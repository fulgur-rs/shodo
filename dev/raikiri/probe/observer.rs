//! Disposable single-thread observer; no source algorithm or allocator lives here.
use std::cell::{Cell, RefCell};

pub enum Event<'a> {
    Start {
        engine: &'static str,
        node: usize,
        text: &'a str,
    },
    End,
}
type Callback = Box<dyn for<'a> FnMut(Event<'a>)>;
thread_local! {
    static HOOK: RefCell<Option<Callback>> = RefCell::new(None);
    static NODE: Cell<usize> = const { Cell::new(usize::MAX) };
}
pub struct Hook;
impl Drop for Hook {
    fn drop(&mut self) {
        HOOK.with(|h| {
            h.borrow_mut().take();
        });
    }
}
pub fn install(callback: Callback) -> Hook {
    HOOK.with(|h| {
        assert!(h.borrow().is_none(), "library observer already installed");
        *h.borrow_mut() = Some(callback);
    });
    Hook
}
pub fn start(engine: &'static str, node: usize, text: &str) {
    HOOK.with(|h| {
        if let Some(callback) = h.borrow_mut().as_mut() {
            callback(Event::Start { engine, node, text });
        }
    });
}
pub fn end() {
    HOOK.with(|h| {
        if let Some(callback) = h.borrow_mut().as_mut() {
            callback(Event::End);
        }
    });
}
pub fn current_node() -> usize {
    NODE.with(Cell::get)
}
pub fn with_node<T>(node: usize, f: impl FnOnce() -> T) -> T {
    struct Restore(usize);
    impl Drop for Restore {
        fn drop(&mut self) {
            NODE.with(|n| n.set(self.0));
        }
    }
    let restore = Restore(NODE.with(|n| n.replace(node)));
    let value = f();
    drop(restore);
    value
}
