//! The guard's own fail-closed exits, armed first by its real entry ([`super::run`], what the
//! app's `--pre-write-guard` reaches; never by a unit test, since both end the process):
//!
//! - A watchdog thread that, when the guard has not ended within [`DEADLINE`], writes
//!   [`LATE`] to stderr and exits with [`DENY`](super::DENY).
//! - A panic hook that writes [`BROKEN`] to stderr and exits with the same code, from any thread.
//!   Release builds abort on a panic (`panic = "abort"`), and the hook runs before the abort.
//!
//! Why: Claude Code 2.1.293 and Codex 0.162.1 block a tool call on its `PreToolUse` hook's exit
//! 2, Codex only with something on stderr, so every way out of the guard but its own answer is
//! exit 2 with one line the agent can act on.
//!
//! [`DEADLINE`] is 30 s: the agents' hook timeout (`agent::loads::GUARD_TIMEOUT_SECONDS`, and
//! Codex's) is 600 s, so the watchdog always answers first, by a wide margin for a busy machine;
//! and 30 s is far above what the guard takes (milliseconds; a shell command's walk of a large
//! matter on a network share, its slowest part, is bounded at 20 000 entries), so it fires only
//! on a hang (a stalled share, a stuck read), where 30 s is the most a tool call waits.
//!
//! The watchdog takes no clock reading of its own (the core bans `Instant::now`): it waits on a
//! channel no one sends on, with `recv_timeout`. The guard ending first ends the thread with
//! the process. When the thread cannot start, the guard refuses at once: nothing would enforce
//! the deadline. Not covered: a stderr pipe the agent never drains (the line's write would
//! block), and a guard killed from outside.
//!
//! Merging with the file-write guard (branch `file-write-guard`, `file_fence::guard::arm`, which
//! sets the same kind of panic hook and its fence's own 2 s watch): keep one panic hook, this
//! one's or its, and both deadlines.

use std::io::Write as _;
use std::time::Duration;

/// How long the guard may run, from its start, before it refuses on its own. Not final: chosen
/// from the pending Windows timing figures (the guard's own run under security software on a
/// slow laptop); 30 s for now (the module doc says why that is safe on both sides).
pub(crate) const DEADLINE: Duration = Duration::from_secs(30);

/// The stderr line when the watchdog fires.
pub(crate) const LATE: &str = "Clauseo's pre-write guard did not finish checking this tool \
     within 30 seconds, so the tool was refused. Try it again; if it is refused again, tell the \
     lawyer that Clauseo's guard is not answering.";

/// The stderr line when the guard panics.
pub(crate) const BROKEN: &str = "Clauseo's pre-write guard failed while checking this tool, so \
     the tool was refused. Try it again; if it is refused again, tell the lawyer that Clauseo's \
     guard is failing.";

/// The stderr line when the watchdog's thread cannot start.
pub(crate) const NO_WATCHDOG: &str = "Clauseo's pre-write guard could not start its time \
     limit, so the tool was refused. Try it again.";

/// Arms the panic hook and the watchdog (the module doc). Called once, first, by the real entry.
pub(super) fn arm() {
    std::panic::set_hook(Box::new(|_| refuse_now(BROKEN)));
    let deadline = deadline();
    let started = std::thread::Builder::new()
        .name("clauseo-guard-watchdog".to_owned())
        .spawn(move || {
            // No one sends: the sender lives as long as this thread waits.
            let (_sender, never) = std::sync::mpsc::channel::<()>();
            if never.recv_timeout(deadline).is_err() {
                refuse_now(LATE);
            }
        });
    if started.is_err() {
        refuse_now(NO_WATCHDOG);
    }
}

/// [`DEADLINE`]; in a test build, [`TEST_DEADLINE_MS`] milliseconds when that variable is set,
/// so the process test need not wait 30 s.
fn deadline() -> Duration {
    #[cfg(test)]
    if let Some(ms) = std::env::var_os(TEST_DEADLINE_MS)
        .and_then(|value| value.to_str().and_then(|value| value.parse::<u64>().ok()))
    {
        return Duration::from_millis(ms);
    }
    DEADLINE
}

/// A test build's variable: the watchdog's deadline in milliseconds.
#[cfg(test)]
pub(crate) const TEST_DEADLINE_MS: &str = "CLAUSEO_GUARD_TEST_DEADLINE_MS";

/// A test build's variable: `panic` makes the guard panic as it starts to decide, `hang` makes
/// it wait forever there (`tests_process.rs`).
#[cfg(test)]
pub(crate) const PLANTED: &str = "CLAUSEO_GUARD_PLANTED";

/// In a test build with [`PLANTED`] set: the planted panic or hang. Nothing otherwise.
pub(super) fn planted() {
    #[cfg(test)]
    match std::env::var_os(PLANTED)
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
    {
        Some("panic") => std::panic::panic_any("a panic planted by a test"),
        Some("hang") => hang(),
        _ => {}
    }
}

/// The planted hang: parked for good, only the watchdog ends it.
#[cfg(test)]
fn hang() -> ! {
    loop {
        std::thread::park();
    }
}

/// Writes `line` to stderr and ends the process with the refusing code.
fn refuse_now(line: &str) {
    let mut errors = std::io::stderr().lock();
    // Nothing more can be done when the agent stopped reading; the exit code still decides.
    drop(
        errors
            .write_all(line.as_bytes())
            .and_then(|()| errors.write_all(b"\n"))
            .and_then(|()| errors.flush()),
    );
    drop(errors);
    exit_now(super::DENY);
}

/// Ends the process with `code`: the refusals above, and the process test's child, which stands
/// in for the app's guard path.
#[expect(
    clippy::exit,
    reason = "the guard's deadline and panic refuse from another thread or a panic hook, where no \
              caller is left to return the code to"
)]
pub(super) fn exit_now(code: i32) {
    std::process::exit(code);
}
