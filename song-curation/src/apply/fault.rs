//! Deterministic fault-injection points for the §14 adversarial matrix.
//!
//! Pure test harness: hooks are thread-local and one-shot; a production run
//! registers none, so every point is an inert no-op. A hook may perform side
//! effects (corrupt a staged file, run a second Apply inline — same thread,
//! no scheduler timing) and/or return an `Err` that the surrounding
//! operation treats as its own I/O failure. One-shot removal makes nested
//! Apply calls inside a hook safe (the point cannot re-fire recursively).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;

type Hook = Box<dyn FnMut() -> io::Result<()>>;

thread_local! {
    static HOOKS: RefCell<BTreeMap<&'static str, Hook>> = RefCell::new(BTreeMap::new());
}

/// Register a one-shot hook at a named point (test harness only).
pub fn set<F>(point: &'static str, hook: F)
where
    F: FnMut() -> io::Result<()> + 'static,
{
    HOOKS.with(|h| h.borrow_mut().insert(point, Box::new(hook)));
}

/// Remove every registered hook (call between test scenarios).
pub fn clear() {
    HOOKS.with(|h| h.borrow_mut().clear());
}

/// Fire the hook at `point`, if any. The hook is consumed before it runs, so
/// a nested Apply inside it never re-enters the same point.
pub(crate) fn hit(point: &'static str) -> io::Result<()> {
    let hook = HOOKS.with(|h| h.borrow_mut().remove(point));
    match hook {
        None => Ok(()),
        Some(mut hook) => hook(),
    }
}
