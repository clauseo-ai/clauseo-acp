//! The guard as a process: what the desktop app's `--pre-write-guard` start and `clauseo-guard`
//! run. [`main`] sets a panic hook that ends the process with [`DENY`] and
//! [`COULD_NOT_CHECK_AGENT`] on stderr, then runs [`run`] on the standard streams. The guard is
//! written not to panic; should it anyway, the step is refused in plain words rather than left
//! to the agent CLI's handling of a crash (a release build aborts on a panic, which Claude Code
//! reads as exit 1 and Codex lets through).

use std::ffi::OsString;
use std::io::Write as _;

use super::{COULD_NOT_CHECK_AGENT, DENY, run};

/// Runs the guard with `args` (the arguments after its flag) on stdin, stdout and stderr, and
/// returns its exit code; a panic on the way exits with [`DENY`] ([`refuse_on_panic`]).
pub fn main(args: &[OsString]) -> i32 {
    refuse_on_panic();
    // Unlocked handles: the panic hook writes stderr too, and a lock held here would stall it.
    run(
        args,
        std::io::stdin().lock(),
        std::io::stdout(),
        std::io::stderr(),
    )
}

/// Replaces the panic hook with one that writes [`COULD_NOT_CHECK_AGENT`] and a newline to
/// stderr and exits with [`DENY`]. Only for the guard's own process: it ends the process.
pub(super) fn refuse_on_panic() {
    std::panic::set_hook(Box::new(|_| {
        // Nothing more can be done when stderr is gone; the exit code still refuses.
        drop(writeln!(std::io::stderr(), "{COULD_NOT_CHECK_AGENT}"));
        exit_refused();
    }));
}

#[expect(
    clippy::exit,
    reason = "the guard's panic hook decides its process's exit: a release build aborts after it"
)]
fn exit_refused() -> ! {
    std::process::exit(DENY)
}
