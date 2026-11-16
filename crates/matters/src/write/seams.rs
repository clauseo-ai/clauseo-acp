//! Tests: the steps of a write made to fail, one at a time on each thread.

use std::io;

thread_local! {
    /// Tests: the step of a write that fails, once armed by [`fail_at`] on this thread.
    static FAIL_AT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// The steps of a write in place that [`fail_at`] can make fail: each refuses the rename first.
const IN_PLACE: [&str; 6] = [
    "rename refused",
    "in place",
    "in place torn",
    "in place cut",
    "in place sync",
    "put back",
];

/// Tests: makes the step `step` of this thread's writes fail: "keep", "sync", "rename" (before
/// it), "rename refused" (the rename refused as macOS refuses a target that denies `delete`),
/// "in place" (a write in place before its first byte), "in place torn" (after each 64 KiB
/// written), "in place cut" (the cut to length), "in place sync" (its sync), "put back"
/// (writing the confirmed version back after one of those), "after rename" (the step after it,
/// [`super::finish`]) or "sweep" (each removal of a crash's temp file, [`super::sweep_temps`]).
/// Several steps joined by `+` fail each ("in place torn+put back").
pub(crate) fn fail_at(step: Option<&'static str>) {
    FAIL_AT.with(|at| at.set(step));
}

/// The steps armed on this thread.
fn armed() -> impl Iterator<Item = &'static str> {
    FAIL_AT
        .with(std::cell::Cell::get)
        .into_iter()
        .flat_map(|steps| steps.split('+'))
}

/// Tests: whether the armed steps refuse the rename of a lawyer's file (a staged write that
/// keeps its target's metadata; never the history's own files): "rename refused", or a step of
/// the write in place that follows it.
pub(super) fn refusing() -> bool {
    armed().any(|step| IN_PLACE.contains(&step))
}

/// Tests: the failure [`fail_at`] armed for `step`, if any.
pub(super) fn injected(step: &'static str) -> io::Result<()> {
    if armed().any(|armed| armed == step) {
        return Err(io::Error::other(step));
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
thread_local! {
    /// Tests: what runs once a write in place has opened its target, armed by [`after_open`].
    static AFTER_OPEN: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Tests: runs `action` on this thread once the next write in place has opened its target,
/// before it checks that the name still has that file (another file put there meanwhile).
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn after_open(action: impl FnOnce() + 'static) {
    AFTER_OPEN.set(Some(Box::new(action)));
}

/// Tests: the action [`after_open`] armed, if any, run once.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn opened() {
    if let Some(action) = AFTER_OPEN.take() {
        action();
    }
}
