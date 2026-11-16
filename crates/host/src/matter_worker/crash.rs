//! The matter thread's loop, and what it does when a job panics. That can happen only where a
//! panic unwinds (dev builds and tests): a release build has `panic = "abort"`, so a panic ends
//! the whole app there.
//!
//! When a job panics, the worker answers that one job with `internal` and drops the matter it
//! held, since the matter may be half way through a change in memory. Before the next job that
//! needs the matter, it opens the matter again from the disk, the same open a start-up does
//! after a crash; a job that had emptied the slot first (a locate) leaves it to be opened from
//! the folder it had before. The history on disk is crash-safe (see `clauseo_matters`'
//! invariants: objects, then the ref and the stat cache; temp files beside the papers until
//! their rename), so this open finds what a restart would find. The job is never tried again,
//! because it may have written part of its change. Its reply goes back through `post` like any
//! other reply, so the actor's work count, the writer and the waiting command, turn or query all
//! move on as they do for an R8 error. The whole step (the job, its log lines, its clean-up and
//! `post`) runs under a catch, so no panic ends the thread and strands the jobs queued behind.
//!
//! After [`LIMIT`] panics within [`WINDOW`] the worker rests for [`REST`]. While it rests,
//! a job that reads or writes the history is answered with `internal` at once, without opening
//! the matter ([`runs_while_resting`] names those that still run). Once the rest is over, the
//! next job tries the matter again, so no restart is needed. Panics come only from jobs, and
//! every job comes from a lawyer's action or a paced step (a turn's end, a listing), so they
//! cannot loop hot. The rest exists so that a matter whose open panics every time is not read in
//! full (timeline, stat cache, tree check) for each of those jobs.
//!
//! This module's own log lines carry the matter id and the job's kind only, and it never reads
//! a panic's payload. The host's panic hook (`log_file/panic.rs`) still writes each panic,
//! caught ones too, to `host.log`: its `file:line`, its thread, and its message only when that
//! is code text (a `&'static str`); a formatted message is left out.

use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use clauseo_matters::Settings;
use clauseo_protocol::MatterId;

use super::{MatterJob, MatterReply, Slot, handle};
use crate::clock::Clock;
use crate::error::Error;
use crate::revisions::RevisionsDone;

use kind::Kind;

/// How many panics within [`WINDOW`] make the worker rest.
pub(super) const LIMIT: usize = 3;
/// The window [`LIMIT`] counts panics in.
pub(super) const WINDOW: Duration = Duration::from_secs(60);
/// How long the worker rests after [`LIMIT`] panics.
pub(super) const REST: Duration = Duration::from_secs(60);

/// The thread's loop: one job at a time until the actor drops its sender. Each job is
/// answered, even one that panics or comes while the worker rests.
pub(super) fn run<T>(
    (matter, clock): (MatterId, &dyn Clock),
    settings: &Settings,
    jobs: &mpsc::Receiver<(MatterJob, T)>,
    post: &dyn Fn(T, MatterReply),
) {
    let mut slot = Slot::Empty;
    let mut crashes = Crashes::default();
    while let Ok((job, tag)) = jobs.recv() {
        let kind = Kind::of(&job);
        let before = held(&slot);
        let stepped = panic::catch_unwind(AssertUnwindSafe(|| {
            guarded(
                (matter, clock),
                settings,
                (&mut slot, &mut crashes),
                &kind,
                job,
            )
        }));
        let reply = stepped.unwrap_or_else(|payload| {
            // A panic in the step's own log lines or clean-up: the job has its reply still.
            forget(payload);
            reopen_later(&mut slot, before, &kind);
            kind.reply(stopped())
        });
        let posted = panic::catch_unwind(AssertUnwindSafe(|| post(tag, reply)));
        if let Err(payload) = posted {
            forget(payload);
        }
    }
}

/// Runs `job`, catching a panic: then the job is answered with `internal` and `slot` reopens
/// before the next job. While the worker rests, a job that needs the history is answered with
/// `internal` and does not run.
fn guarded(
    (matter, clock): (MatterId, &dyn Clock),
    settings: &Settings,
    (slot, crashes): (&mut Slot, &mut Crashes),
    kind: &Kind,
    job: MatterJob,
) -> MatterReply {
    if crashes.resting(clock.now().mono) && !runs_while_resting(kind) {
        tracing::debug!(%matter, job = kind.name(), "a matter job was refused: its worker rests");
        drop(job);
        kind.clean_up();
        return kind.reply(Error::Internal(
            "the matter's worker rests after repeated failures".to_owned(),
        ));
    }
    let before = held(slot);
    let ran = panic::catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        hook::fire(&settings.matters_root, kind.name());
        handle(settings, slot, job)
    }));
    let payload = match ran {
        Ok(reply) => {
            if failed_in_history(settings, &reply) {
                tracing::info!(
                    %matter,
                    job = kind.name(),
                    "a matter job failed in its history; the matter opens again"
                );
                reopen_later(slot, before, kind);
            }
            return reply;
        }
        Err(payload) => payload,
    };
    forget(payload);
    reopen_later(slot, before, kind);
    kind.clean_up();
    if crashes.crashed(clock.now().mono) {
        tracing::warn!(
            %matter,
            job = kind.name(),
            panics = LIMIT,
            rest_secs = REST.as_secs(),
            "a matter job panicked again; the matter's worker rests"
        );
    } else {
        tracing::warn!(
            %matter,
            job = kind.name(),
            "a matter job panicked; the matter opens again before its next job"
        );
    }
    kind.reply(stopped())
}

/// The error of a job that panicked.
fn stopped() -> Error {
    Error::Internal("the matter's job stopped unexpectedly".to_owned())
}

/// Drops a panic's payload unread, catching a panic in its drop too.
fn forget(payload: Box<dyn std::any::Any + Send>) {
    let _dropped = panic::catch_unwind(AssertUnwindSafe(move || drop(payload)));
}

/// The jobs that run while the worker rests, since they read no history or must not be lost:
/// - `HeldOpen` stats the agent's paths; refused, it would put a permission request to the
///   lawyer (`turn/asks.rs`);
/// - `Discard` undoes a create the host could not record; refused, it would leave that folder
///   behind, and of a matter not open (as after a panic) it is `Ok` with nothing to do;
/// - `AfterTurn` and `Repair` record what the turn wrote: refused, the turn's edits would land
///   in the next checkpoint as outside edits, the lawyer's own. Each comes once per turn, so
///   letting them try the matter costs at most one open per turn.
fn runs_while_resting(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::HeldOpen | Kind::Discard | Kind::AfterTurn | Kind::Repair
    )
}

/// The matter `slot` holds or will open: its id and folder.
fn held(slot: &Slot) -> Option<(MatterId, PathBuf)> {
    match slot {
        Slot::Open(matter) => Some((matter.id(), matter.folder().to_path_buf())),
        Slot::Unopened { id, folder, .. } => Some((*id, folder.clone())),
        Slot::Empty => None,
    }
}

/// Whether `reply` is a job that writes (a checkpoint, an undo, a restore, a tracked-changes
/// decision) failing in the history: an R8 `History` error, or an I/O error on a file under
/// the matters' histories (`Settings::matters_root`). R8 reads its timeline again after a
/// publish that fails, but if that read fails too, the matter in memory may no longer match the
/// disk, and every later checkpoint would fail until a restart. Opening it again from the disk
/// costs one open after such a failure, and is what a restart would do. An I/O error on a paper
/// (one the lawyer may not write) leaves the history as it was, so it reopens nothing.
fn failed_in_history(settings: &Settings, reply: &MatterReply) -> bool {
    let error = match reply {
        MatterReply::BeforeTurn(Err(error))
        | MatterReply::AfterTurn(Err(error))
        | MatterReply::Undone(Err(error))
        | MatterReply::Restored(Err(error)) => error,
        MatterReply::Revisions(done) => match &**done {
            RevisionsDone::Decided(Err(error)) => error,
            RevisionsDone::Decided(Ok(_)) | RevisionsDone::Listed(_) => return false,
        },
        MatterReply::Created(_)
        | MatterReply::Imported(_)
        | MatterReply::Opened(_)
        | MatterReply::Located(_)
        | MatterReply::BeforeTurn(Ok(_))
        | MatterReply::AfterTurn(Ok(_))
        | MatterReply::Undone(Ok(_))
        | MatterReply::Restored(Ok(_))
        | MatterReply::Compared(_)
        | MatterReply::Previewed(_)
        | MatterReply::Read
        | MatterReply::HeldOpen(_)
        | MatterReply::Papers(_)
        | MatterReply::EditedSince(_)
        | MatterReply::StillGone(_)
        | MatterReply::Discarded(_)
        | MatterReply::SidesWritten(_) => return false,
    };
    in_history(&settings.matters_root, error)
}

/// Whether `error` is R8's `History`, or its `Io` on a path under `root`.
fn in_history(root: &Path, error: &Error) -> bool {
    matches!(
        error,
        Error::Matters(clauseo_matters::Error::History { .. })
    ) || matches!(
        error,
        Error::Matters(clauseo_matters::Error::Io { path, .. }) if is_under(root, path)
    )
}

/// Whether `path` lies under `root`. On Windows R8 does its history I/O through the verbatim
/// form of the git dir (`\\?\C:\…`, `\\?\UNC\srv\…`; `clauseo_matters`' `long_path`), so an
/// error's path may carry that prefix while `root` is plain: both are compared in their plain
/// text form too. Text only; nothing is resolved.
fn is_under(root: &Path, path: &Path) -> bool {
    if path.starts_with(root) {
        return true;
    }
    let (root, path) = (plain_text(root), plain_text(path));
    let root = root.trim_end_matches(['\\', '/']);
    !root.is_empty()
        && path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with(['\\', '/']))
}

/// `path`'s text without a verbatim prefix: `\\?\UNC\srv\x` is `\\srv\x`, `\\?\C:\x` is
/// `C:\x`; any other text as it is.
fn plain_text(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        text.into_owned()
    }
}

/// After a panic or a history failure: an open matter is dropped and opened again from its
/// folder before the next job that needs it. A slot the job emptied (a locate empties it before
/// `Matter::locate`) is opened again from `before`, the folder it held, which is the one the
/// actor still records (it records a new folder only on the locate's `Ok`); a discard's empty
/// slot stays empty. A matter not yet opened stays as it is; with none, nothing is held.
fn reopen_later(slot: &mut Slot, before: Option<(MatterId, PathBuf)>, kind: &Kind) {
    let (id, folder) = match slot {
        Slot::Open(matter) => (matter.id(), matter.folder().to_path_buf()),
        Slot::Empty if !matches!(kind, Kind::Discard) => match before {
            Some(before) => before,
            None => return,
        },
        Slot::Empty | Slot::Unopened { .. } => return,
    };
    let next = Slot::Unopened {
        id,
        folder,
        relink: None,
    };
    let old = std::mem::replace(slot, next);
    // The matter's drop runs here, not on a later job, and a panic in it is caught too.
    let _dropped = panic::catch_unwind(AssertUnwindSafe(move || drop(old)));
}

/// One worker's recent panics, and the end of its rest, in the host clock's monotonic time.
#[derive(Debug, Default)]
pub(super) struct Crashes {
    /// The panics within the last [`WINDOW`], oldest first.
    recent: Vec<Duration>,
    /// The worker rests until then.
    until: Option<Duration>,
}

impl Crashes {
    /// Whether the worker rests at `now`.
    pub(super) fn resting(&mut self, now: Duration) -> bool {
        match self.until {
            Some(until) if now < until => true,
            Some(_) | None => {
                self.until = None;
                false
            }
        }
    }

    /// A job panicked at `now`; `true` when that makes the worker rest.
    pub(super) fn crashed(&mut self, now: Duration) -> bool {
        self.recent.retain(|at| now.saturating_sub(*at) < WINDOW);
        self.recent.push(now);
        if self.recent.len() < LIMIT {
            return false;
        }
        self.recent.clear();
        self.until = now.checked_add(REST);
        true
    }
}

/// A test's panic in a chosen job, or at a chosen point of one, of the matters under a chosen
/// root (`Settings::matters_root`): each test has its own data folder, so no test fires another's.
#[cfg(test)]
pub(crate) mod hook {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};

    /// The panics armed: a matters root, a job kind's name or a point's, and how many of the
    /// next ones panic.
    static ARMED: Mutex<Vec<(PathBuf, &'static str, usize)>> = Mutex::new(Vec::new());

    /// The next `times` jobs of kind `job` (`Kind::name`, such as `"AfterTurn"`), or passes
    /// through the point `job` (such as `"Locate:emptied"`), of a worker whose matters live
    /// under `root` panic.
    pub(crate) fn panic_next(root: &Path, job: &'static str, times: usize) {
        let mut armed = ARMED.lock().unwrap_or_else(PoisonError::into_inner);
        armed.push((root.to_path_buf(), job, times));
    }

    /// Panics when a panic is armed for this job or point.
    pub(crate) fn fire(root: &Path, job: &str) {
        let hit = {
            let mut armed = ARMED.lock().unwrap_or_else(PoisonError::into_inner);
            let found = armed
                .iter_mut()
                .find(|(at, name, left)| at == root && *name == job && *left > 0);
            found.map(|(_, _, left)| *left -= 1).is_some()
        };
        assert!(!hit, "a test's panic in a matter job");
    }
}

mod kind;
#[cfg(test)]
mod tests;
