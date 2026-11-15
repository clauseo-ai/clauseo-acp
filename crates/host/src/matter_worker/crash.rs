//! The matter thread's loop, and what it does when a job panics. That can happen only where a
//! panic unwinds (dev builds and tests): a release build has `panic = "abort"`, so a panic ends
//! the whole app there.
//!
//! When a job panics, the worker answers that one job with `internal` and drops the matter it
//! held, since the matter may be half way through a change in memory. Before the next job that
//! needs the matter, it opens the matter again from the disk, the same open a start-up does
//! after a crash. The history on disk is crash-safe (see `clauseo_matters`' invariants:
//! objects, then the ref and the stat cache; temp files beside the papers until their rename),
//! so this open finds what a restart would find. The job is never tried again, because it may
//! have written part of its change. Its reply goes back through `post` like any other reply,
//! so the actor's work count, the writer and the waiting command, turn or query all move on as
//! they do for an R8 error.
//!
//! After [`LIMIT`] panics within [`WINDOW`] the worker rests for [`REST`]. While it rests,
//! each job is answered with `internal` at once, without opening the matter. Once the rest is
//! over, the next job tries the matter again, so no restart is needed. Panics come only from
//! jobs, and every job comes from a lawyer's action or a paced step (a turn's end, a listing),
//! so they cannot loop hot. The rest exists so that a matter whose open panics every time is
//! not read in full (timeline, stat cache, tree check) for each of those jobs.
//!
//! The log has the matter id and the job's kind and nothing else: a panic's text may hold a
//! path, so it is dropped unread.

use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use clauseo_matters::Settings;
use clauseo_protocol::{ErrorCode, MatterId};

use super::{MatterJob, MatterReply, Slot, handle};
use crate::agent::data_fence::PathsChecked;
use crate::clock::Clock;
use crate::error::Error;
use crate::revisions::{Act, RevisionsDone};

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
        let reply = guarded((matter, clock), settings, (&mut slot, &mut crashes), job);
        post(tag, reply);
    }
}

/// Runs `job`, catching a panic: then the job is answered with `internal` and `slot` reopens
/// before the next job. While the worker rests, `job` is answered with `internal` and does not
/// run.
fn guarded(
    (matter, clock): (MatterId, &dyn Clock),
    settings: &Settings,
    (slot, crashes): (&mut Slot, &mut Crashes),
    job: MatterJob,
) -> MatterReply {
    let kind = Kind::of(&job);
    if crashes.resting(clock.now().mono) {
        tracing::debug!(%matter, job = kind.name(), "a matter job was refused: its worker rests");
        drop(job);
        return kind.failed(Error::Internal(
            "the matter's worker rests after repeated failures".to_owned(),
        ));
    }
    let ran = panic::catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        hook::fire(matter, kind.name());
        handle(settings, slot, job)
    }));
    let payload = match ran {
        Ok(reply) => {
            if failed_in_history(&reply) {
                tracing::info!(
                    %matter,
                    job = kind.name(),
                    "a matter job failed in its history; the matter opens again"
                );
                reopen_later(slot);
            }
            return reply;
        }
        Err(payload) => payload,
    };
    // Dropped unread: its text may hold a path. A payload's drop may panic too.
    let _dropped = panic::catch_unwind(AssertUnwindSafe(move || drop(payload)));
    reopen_later(slot);
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
    kind.failed(Error::Internal(
        "the matter's job stopped unexpectedly".to_owned(),
    ))
}

/// Whether `reply` is a job that writes (a checkpoint, an undo, a restore, a tracked-changes
/// decision) failing on the disk or in the history (`matter.checkpoint_failed`). R8 reads its
/// timeline again after a publish that fails, but if that read fails too, the matter in memory
/// may no longer match the disk, and every later checkpoint would fail until a restart. Opening
/// it again from the disk costs one open after such a failure, and is what a restart would do.
fn failed_in_history(reply: &MatterReply) -> bool {
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
        | MatterReply::HeldOpen(_)
        | MatterReply::Papers(_)
        | MatterReply::EditedSince(_)
        | MatterReply::StillGone(_)
        | MatterReply::Discarded(_)
        | MatterReply::SidesWritten(_) => return false,
    };
    error.code() == ErrorCode::MatterCheckpointFailed
}

/// After a panic: an open matter is dropped and opened again from its folder before the next
/// job that needs it. A matter not yet opened stays as it is; with none, nothing is held.
fn reopen_later(slot: &mut Slot) {
    let Slot::Open(matter) = slot else {
        return;
    };
    let next = Slot::Unopened {
        id: matter.id(),
        folder: matter.folder().to_path_buf(),
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

/// A job's kind: its name for the log, and its reply when it cannot run.
#[derive(Debug)]
enum Kind {
    Create,
    Import,
    Open,
    Locate,
    BeforeTurn,
    AfterTurn,
    Repair,
    Undo,
    Restore,
    Compare,
    Preview,
    HeldOpen,
    ListPapers,
    EditedSince,
    StillGone,
    Discard,
    WriteSides,
    /// The job's private folder, removed when the job does not finish, as `revisions::run`
    /// removes it after a job that does.
    Revisions {
        decide: bool,
        work: PathBuf,
    },
}

impl Kind {
    fn of(job: &MatterJob) -> Self {
        match job {
            MatterJob::Create { .. } => Self::Create,
            MatterJob::Import { .. } => Self::Import,
            MatterJob::Open { .. } => Self::Open,
            MatterJob::Locate { .. } => Self::Locate,
            MatterJob::BeforeTurn(_) => Self::BeforeTurn,
            MatterJob::AfterTurn { .. } => Self::AfterTurn,
            MatterJob::Repair { .. } => Self::Repair,
            MatterJob::Undo(_) => Self::Undo,
            MatterJob::Restore(_) => Self::Restore,
            MatterJob::Compare { .. } => Self::Compare,
            MatterJob::Preview { .. } => Self::Preview,
            MatterJob::HeldOpen { .. } => Self::HeldOpen,
            MatterJob::ListPapers { .. } => Self::ListPapers,
            MatterJob::EditedSince { .. } => Self::EditedSince,
            MatterJob::StillGone { .. } => Self::StillGone,
            MatterJob::Discard => Self::Discard,
            MatterJob::WriteSides(_) => Self::WriteSides,
            MatterJob::Revisions(job) => Self::Revisions {
                decide: matches!(job.act, Act::Decide { .. }),
                work: job.work.clone(),
            },
        }
    }

    /// The name for the log: the job's fields hold paths.
    pub(super) const fn name(&self) -> &'static str {
        match self {
            Self::Create => "Create",
            Self::Import => "Import",
            Self::Open => "Open",
            Self::Locate => "Locate",
            Self::BeforeTurn => "BeforeTurn",
            Self::AfterTurn => "AfterTurn",
            Self::Repair => "Repair",
            Self::Undo => "Undo",
            Self::Restore => "Restore",
            Self::Compare => "Compare",
            Self::Preview => "Preview",
            Self::HeldOpen => "HeldOpen",
            Self::ListPapers => "ListPapers",
            Self::EditedSince => "EditedSince",
            Self::StillGone => "StillGone",
            Self::Discard => "Discard",
            Self::WriteSides => "WriteSides",
            Self::Revisions { .. } => "Revisions",
        }
    }

    /// The job's reply when it panicked or did not run, holding `error`. `HeldOpen` has no
    /// error to hold: it reads as nothing held, so the permission request goes to the lawyer,
    /// as it does when the job cannot be sent (`turn/asks.rs`).
    fn failed(self, error: Error) -> MatterReply {
        match self {
            Self::Create => MatterReply::Created(Err(error)),
            Self::Import => MatterReply::Imported(Err(error)),
            Self::Open => MatterReply::Opened(Err(error)),
            Self::Locate => MatterReply::Located(Err(error)),
            Self::BeforeTurn => MatterReply::BeforeTurn(Err(error)),
            Self::AfterTurn | Self::Repair => MatterReply::AfterTurn(Err(error)),
            Self::Undo => MatterReply::Undone(Err(error)),
            Self::Restore => MatterReply::Restored(Err(error)),
            Self::Compare => MatterReply::Compared(Err(error)),
            Self::Preview => MatterReply::Previewed(Err(error)),
            Self::HeldOpen => MatterReply::HeldOpen(PathsChecked::Held(None)),
            Self::ListPapers => MatterReply::Papers(Err(error)),
            Self::EditedSince => MatterReply::EditedSince(Err(error)),
            Self::StillGone => MatterReply::StillGone(Err(error)),
            Self::Discard => MatterReply::Discarded(Err(error)),
            Self::WriteSides => MatterReply::SidesWritten(Err(error)),
            Self::Revisions { decide, work } => {
                if let Err(err) = crate::write::remove_tree(&work) {
                    tracing::warn!(
                        error = %crate::log_file::Cause(&err),
                        "a tracked-changes work folder was left behind"
                    );
                }
                MatterReply::Revisions(Box::new(if decide {
                    RevisionsDone::Decided(Err(error))
                } else {
                    RevisionsDone::Listed(Err(error))
                }))
            }
        }
    }
}

/// A test's panic in a chosen job of a chosen matter.
#[cfg(test)]
pub(crate) mod hook {
    use std::sync::{Mutex, PoisonError};

    use clauseo_protocol::MatterId;

    /// The panics armed: a matter, a job kind's name, and how many of its next such jobs panic.
    static ARMED: Mutex<Vec<(MatterId, &'static str, usize)>> = Mutex::new(Vec::new());

    /// The next `times` jobs of kind `job` (`Kind::name`, such as `"AfterTurn"`) on `matter`'s
    /// worker panic before they run.
    pub(crate) fn panic_next(matter: MatterId, job: &'static str, times: usize) {
        let mut armed = ARMED.lock().unwrap_or_else(PoisonError::into_inner);
        armed.push((matter, job, times));
    }

    /// Panics when a panic is armed for this job.
    pub(super) fn fire(matter: MatterId, job: &str) {
        let hit = {
            let mut armed = ARMED.lock().unwrap_or_else(PoisonError::into_inner);
            let found = armed
                .iter_mut()
                .find(|(at, name, left)| *at == matter && *name == job && *left > 0);
            found.map(|(_, _, left)| *left -= 1).is_some()
        };
        assert!(!hit, "a test's panic in a matter job");
    }
}

#[cfg(test)]
mod tests;
