//! Unix: a sign-in fake ends once the process that started it is gone. The engine starts each
//! run in a process group of its own, so a test process killed on a timeout (or by a signal to
//! its group) would otherwise leave a fake that waits forever (`never`) running with no parent.

#[cfg(unix)]
use std::time::Duration;

/// How often the fake looks at its parent.
#[cfg(unix)]
const PARENT_POLL: Duration = Duration::from_millis(250);
/// The exit code of a fake whose parent went away.
#[cfg(unix)]
const ORPHANED: i32 = 1;

/// Starts a thread that ends the process with [`ORPHANED`] once its parent changes (it was
/// re-parented, to init or a subreaper, because its parent died).
#[cfg(unix)]
pub(super) fn end_with_parent() {
    let parent = std::os::unix::process::parent_id();
    std::thread::spawn(move || {
        loop {
            crate::run::effects::pause(PARENT_POLL);
            if std::os::unix::process::parent_id() != parent {
                orphaned();
            }
        }
    });
}

/// Windows: nothing; the leak this guards against was seen on Unix only.
#[cfg(not(unix))]
pub(super) fn end_with_parent() {}

#[cfg(unix)]
#[expect(
    clippy::exit,
    reason = "a sign-in fake whose parent died ends from its watch thread"
)]
fn orphaned() -> ! {
    std::process::exit(ORPHANED)
}
