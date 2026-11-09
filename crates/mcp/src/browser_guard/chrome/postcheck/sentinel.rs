//! The check for Chrome's whole life (`postcheck.rs`): after the check at the start, a thread
//! looks at Chrome's children every [`EVERY`] until Chrome drops or exits, and judges each child
//! that has appeared since (a later renderer, the GPU process) as the start's check does. A child
//! that stays unsandboxed for [`GRACE`] refuses the browser: the caller's `on_refused` (in
//! `session.rs`) gives the start's refusal and kills Chrome, which is never started again.
//!
//! Polling, on every OS. Windows' job notifications (`JOB_OBJECT_MSG_NEW_PROCESS` through
//! `JOBOBJECT_ASSOCIATE_COMPLETION_PORT`) would tell each new process at once, but Windows does
//! not guarantee their delivery (the structure's documentation), so a check that relied on them
//! would still have to poll; one path on all three systems, tested over fakes, is the simpler.
//!
//! What a look costs: one listing of Chrome's children (Linux: one listing of `/proc`, the
//! `stat` of each process not seen before, and one `stat` per child of Chrome; Windows: the
//! job's process list and one start-time query each; macOS: `proc_listchildpids` down the tree
//! and one `proc_pidinfo` each); a child already judged, with the same start time, is not read
//! again. A new child costs its command line and its sandbox query, once.

use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::super::super::session::{Refused, Shared};
use super::super::{Chrome, Tracked, lock};
use super::{Kind, Look, REQUIRED, Source, Watch, source};

/// How often the sentinel looks.
pub(crate) const EVERY: Duration = Duration::from_millis(250);
/// How long a child may fail before it refuses the browser: a renderer the zygote has just
/// forked installs its seccomp filter only after the fork, and a Windows child may be listed
/// before its token can be read.
pub(crate) const GRACE: Duration = Duration::from_secs(2);

/// Starts the sentinel over `chrome`, whose children `watch` judged at its start, reading this
/// OS's children through `shared`'s launcher (its profile, and on Windows its job). `on_refused`
/// runs on the sentinel's thread with the first refusal, unless Chrome has dropped or exited by
/// then (that is the relay's to report: `Event::ChromeExited`).
pub(crate) fn start(
    chrome: &mut Chrome,
    watch: Watch,
    shared: Arc<Shared>,
    on_refused: impl FnOnce(Refused) + Send + 'static,
) {
    let (stop, stopped) = mpsc::channel::<()>();
    chrome.stops.push(stop);
    let tracked = Arc::clone(&chrome.child);
    let pid = lock(&tracked).child.id();
    let grace = u32::try_from(GRACE.as_millis() / EVERY.as_millis()).unwrap_or(u32::MAX);
    drop(thread::spawn(move || {
        let launcher = &shared.launcher;
        let refused = match source(
            &launcher.profile,
            #[cfg(windows)]
            &launcher.job,
        ) {
            Ok(mut source) => keep_looking(&mut *source, watch, (pid, REQUIRED, grace), || {
                runs(&stopped, &tracked, EVERY)
            }),
            Err(refused) => Some(refused),
        };
        // Each test in its own statement, so no lock is held into `on_refused`, which takes the
        // state's lock (a stop under that lock takes Chrome's).
        let dropped = stopped.try_recv() != Err(TryRecvError::Empty);
        let exited = lock(&tracked).exited();
        if let Some(refused) = refused
            && !dropped
            && !exited
        {
            on_refused(refused);
        }
    }));
}

/// Waits `every`, then whether Chrome still runs and has not dropped.
fn runs(stopped: &mpsc::Receiver<()>, tracked: &Mutex<Tracked>, every: Duration) -> bool {
    match stopped.recv_timeout(every) {
        Err(RecvTimeoutError::Timeout) => !lock(tracked).exited(),
        Ok(()) | Err(RecvTimeoutError::Disconnected) => false,
    }
}

/// Looks at the children of `chrome` (with the kinds `required` and a grace of `grace` looks)
/// each time `wait` says Chrome still runs: the first refusal, or `None` once `wait` says no.
pub(crate) fn keep_looking(
    source: &mut dyn Source,
    mut watch: Watch,
    (chrome, required, grace): (u32, &[Kind], u32),
    mut wait: impl FnMut() -> bool,
) -> Option<Refused> {
    while wait() {
        if let Look::Unsandboxed { kind, why } = watch.look(source, chrome, required, grace) {
            return Some(Refused::Unsandboxed { kind, why });
        }
    }
    None
}
