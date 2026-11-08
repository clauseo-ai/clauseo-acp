//! A process whose kill was refused ([`Processes::kill`] failed) may still end by itself. A
//! console host (`conhost.exe`) of the pool user's refuses `TerminateProcess` with 5, and exits
//! once its last client is gone, which the same round may just have killed. So after a round's
//! kills, each refused process is looked at again every [`EXIT_POLL`] for up to [`EXIT_WAIT`];
//! only one still running then is a failure (the first act fails closed on it, a later sweep
//! logs it). The rule is the same for every process: no image name is special.

use std::io;
use std::time::Duration;

use super::{Point, Processes, Seen, code};

/// How long a refused process has to end by itself.
pub(super) const EXIT_WAIT: Duration = Duration::from_secs(2);
/// How often it is looked at meanwhile.
pub(super) const EXIT_POLL: Duration = Duration::from_millis(100);

/// A process whose kill was refused, still open (its pid pinned).
pub(super) struct Refused<'a, T> {
    pub(super) found: &'a Seen,
    pub(super) process: T,
    /// Its creation time, when the sweep read it.
    pub(super) started: Option<u64>,
    /// Why the kill failed.
    pub(super) error: io::Error,
}

impl<T> Refused<'_, T> {
    /// The line logged at `point` once it exited by itself.
    pub(super) fn exited_line(&self, point: Point) -> String {
        format!(
            "clauseo-sandbox-run: sweep ({point}): the sandbox user's process {} ({}) exited by \
             itself after its kill was refused ({})",
            self.found.pid,
            self.found.image,
            code(&self.error)
        )
    }
}

/// The refused processes that exited by themselves, and the ones still running.
pub(super) type Outcome<'a, T> = (Vec<Refused<'a, T>>, Vec<Refused<'a, T>>);

/// Looks at each refused process, then again every [`EXIT_POLL`] until none is left or
/// [`EXIT_WAIT`] has passed.
pub(super) fn wait_out<'a, P: Processes>(
    calls: &mut P,
    refused: Vec<Refused<'a, P::Process>>,
) -> Outcome<'a, P::Process> {
    let mut ended = Vec::new();
    let mut left = refused;
    let mut waited = Duration::ZERO;
    loop {
        let (gone, still): (Vec<_>, Vec<_>) = left
            .into_iter()
            .partition(|refused| calls.exited(&refused.process, Duration::ZERO));
        ended.extend(gone);
        left = still;
        if left.is_empty() || waited >= EXIT_WAIT {
            return (ended, left);
        }
        calls.pause(EXIT_POLL);
        waited += EXIT_POLL;
    }
}
