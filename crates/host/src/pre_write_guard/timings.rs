//! Opt-in phase timings for the guard (clauseo-ai/clauseo-acp#291): with [`ENV`] set to
//! anything but empty or `0`, [`super::run`] ends by writing one more stderr line, starting
//! [`PREFIX`], with the microseconds each phase took since the guard started and what the walk
//! of held files read. Off by default; off, each mark is one thread-local read.
//!
//! The phases, in order (one that did not run is left out): `read` (stdin), `parse` (JSON),
//! `targets` (the paths the tool names; for a shell command, its words, each looked up as a file
//! from every folder it may be in), `resolve` (each target resolved canonically and checked for
//! an owner file), `walk` (a shell command's listing of the matter's held files), `decide` (the
//! rest: the name backstop, the read-only rule) and `total`. The counts: `entries` and `folders`
//! the walk read, `owner_folders` the folders with an owner file in it, `checked` the files
//! beside them, and `probed` those looked up on disk (the ones an owner file may point at, and on
//! Windows each other one a command names, opened then).

use std::cell::RefCell;
use std::time::Duration;

use crate::clock::{Clock, SystemClock};

/// The environment variable that turns the timings on.
pub(super) const ENV: &str = "CLAUSEO_GUARD_TIMINGS";

/// The start of the timings line.
pub(super) const PREFIX: &str = "clauseo-guard-timings (us):";

/// Whether the variable's value turns the timings on: set, not empty and not `0`.
pub(super) fn wanted(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

/// One guard call's phases and counts.
#[derive(Debug, Default)]
struct Timings {
    start: Duration,
    last: Duration,
    /// Each phase's time, summed when a phase is marked more than once, in first-mark order.
    phases: Vec<(&'static str, Duration)>,
    walk: Walk,
}

/// What the walk of held files read.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Walk {
    pub(super) entries: usize,
    pub(super) folders: usize,
    pub(super) owner_folders: usize,
    pub(super) checked: usize,
    pub(super) probed: usize,
}

thread_local! {
    /// The running guard call's timings, when on. The guard runs on one thread.
    static TIMINGS: RefCell<Option<Timings>> = const { RefCell::new(None) };
}

fn now() -> Duration {
    SystemClock.now().mono
}

/// Starts timing this thread's guard call when `on`; stops any earlier one either way.
pub(super) fn start(on: bool) {
    let timings = on.then(|| {
        let start = now();
        Timings {
            start,
            last: start,
            ..Timings::default()
        }
    });
    TIMINGS.with(|cell| *cell.borrow_mut() = timings);
}

/// Ends `phase`: the time since the last mark (or the start) is added to it.
pub(super) fn mark(phase: &'static str) {
    TIMINGS.with(|cell| {
        if let Some(timings) = cell.borrow_mut().as_mut() {
            let at = now();
            let took = at.saturating_sub(timings.last);
            timings.last = at;
            match timings.phases.iter_mut().find(|(name, _)| *name == phase) {
                Some((_, sum)) => *sum = sum.saturating_add(took),
                None => timings.phases.push((phase, took)),
            }
        }
    });
}

/// Notes what the walk read.
pub(super) fn walked(walk: Walk) {
    TIMINGS.with(|cell| {
        if let Some(timings) = cell.borrow_mut().as_mut() {
            timings.walk = walk;
        }
    });
}

/// Notes one more file the walk looked up (one it kept to open later, opened now).
pub(super) fn probed() {
    TIMINGS.with(|cell| {
        if let Some(timings) = cell.borrow_mut().as_mut() {
            timings.walk.probed += 1;
        }
    });
}

/// Ends the guard call's timing: its line (without a newline) when on.
pub(super) fn finish() -> Option<String> {
    let timings = TIMINGS.with(|cell| cell.borrow_mut().take())?;
    let total = now().saturating_sub(timings.start);
    let phases: Vec<String> = timings
        .phases
        .iter()
        .copied()
        .chain(std::iter::once(("total", total)))
        .map(|(phase, took)| format!("{phase}={}", took.as_micros()))
        .collect();
    let Walk {
        entries,
        folders,
        owner_folders,
        checked,
        probed,
    } = timings.walk;
    Some(format!(
        "{PREFIX} {} | entries={entries} folders={folders} owner_folders={owner_folders} \
         checked={checked} probed={probed}",
        phases.join(" ")
    ))
}
