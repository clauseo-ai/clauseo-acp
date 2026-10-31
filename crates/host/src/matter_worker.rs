//! One OS thread, `clauseo-matter`, per open matter. It owns the matter's R8 [`Matter`] and runs
//! its synchronous jobs (create, import, open, checkpoints, undo) off the actor, which sends each
//! job with a tag and gets each reply back through `post`, tagged the same way.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use clauseo_matters::{
    Checkpoint, ImportWarning, Imported, Limits, Matter, NewMatter, OsProbe, Retention, Settings,
    TurnMeta, UndoRequest, Undone,
};
use clauseo_protocol::{
    CheckpointId, FileDecision, ImportSummary, MatterId, MatterPath, ThreadId, Timestamp, TurnId,
};

use crate::error::Error;

/// The name every matter thread carries.
const THREAD_NAME: &str = "clauseo-matter";

/// R8's settings for the host: matters under `<data folder>/matters`, the default limits, every
/// checkpoint kept, and the OS's cloud-only probe.
pub(crate) fn matter_settings(data_dir: &Path) -> Settings {
    Settings {
        matters_root: data_dir.join("matters"),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::new(OsProbe),
    }
}

/// An undo request that owns its decisions, so it can cross to the matter thread.
#[derive(Debug, Clone)]
pub(crate) struct UndoJob {
    pub(crate) before: CheckpointId,
    pub(crate) after: CheckpointId,
    pub(crate) decisions: Vec<FileDecision>,
    pub(crate) thread: Option<ThreadId>,
    pub(crate) turn: Option<TurnId>,
    pub(crate) outside_edits: CheckpointId,
    pub(crate) checkpoint: CheckpointId,
    pub(crate) at: Timestamp,
}

/// One job for a matter thread.
#[derive(Debug)]
pub(crate) enum MatterJob {
    /// `Matter::create`; the worker keeps the matter.
    Create(NewMatter),
    /// `Matter::import` with a no-op progress callback; `cancel` is set by shutdown.
    Import {
        new: NewMatter,
        refused: Vec<PathBuf>,
        cancel: Arc<AtomicBool>,
    },
    /// `Matter::open`. When it fails, the worker keeps the id and folder and tries the open
    /// again before the next job that needs the matter.
    Open { id: MatterId, folder: PathBuf },
    /// The before-turn checkpoint.
    BeforeTurn(TurnMeta),
    /// The after-turn checkpoint with the paths the turn's tool calls named.
    AfterTurn {
        meta: TurnMeta,
        tool_paths: BTreeSet<MatterPath>,
    },
    /// The paths of the step `before`..`after` edited since.
    EditedSince {
        before: CheckpointId,
        after: CheckpointId,
    },
    /// Undo one step.
    Undo(UndoJob),
}

/// How an import ended.
#[derive(Debug)]
pub(crate) enum ImportEnd {
    /// The matter exists; the worker keeps it.
    Done {
        summary: ImportSummary,
        warnings: Vec<ImportWarning>,
    },
    /// The cancel flag was set; nothing was kept.
    Cancelled,
}

/// One job's result, one variant per [`MatterJob`] kind.
#[derive(Debug)]
pub(crate) enum MatterReply {
    Created(Result<(), Error>),
    Imported(Result<ImportEnd, Error>),
    /// The open matter's last checkpoint.
    Opened(Result<Option<CheckpointId>, Error>),
    BeforeTurn(Result<Checkpoint, Error>),
    AfterTurn(Result<Checkpoint, Error>),
    EditedSince(Result<Vec<MatterPath>, Error>),
    Undone(Result<Undone, Error>),
}

/// What the worker holds.
enum Slot {
    /// No matter yet.
    Empty,
    /// The last open failed; tried again before the next job that needs the matter.
    Unopened { id: MatterId, folder: PathBuf },
    /// The matter.
    Open(Box<Matter>),
}

/// The matter thread, as the actor holds it.
pub(crate) struct MatterWorker<T: Send + 'static> {
    jobs: Option<mpsc::Sender<(MatterJob, T)>>,
    thread: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> MatterWorker<T> {
    /// Starts the thread. Each reply goes to `post` with its job's tag, in job order.
    pub(crate) fn start(
        settings: Settings,
        post: Box<dyn Fn(T, MatterReply) + Send>,
    ) -> Result<Self, Error> {
        let (jobs, receiver) = mpsc::channel::<(MatterJob, T)>();
        let thread = thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || run(&settings, &receiver, &*post))
            .map_err(|err| Error::Internal(format!("could not start {THREAD_NAME}: {err}")))?;
        Ok(Self {
            jobs: Some(jobs),
            thread: Some(thread),
        })
    }

    /// Queues a job. `Err(Closed)` when the thread is gone.
    pub(crate) fn send(&self, job: MatterJob, tag: T) -> Result<(), Error> {
        let jobs = self.jobs.as_ref().ok_or(Error::Closed)?;
        jobs.send((job, tag)).map_err(|_closed| Error::Closed)
    }

    /// Lets the thread finish the jobs already queued, then joins it. Set an import's cancel flag
    /// first so a long import ends early.
    pub(crate) fn stop(mut self) -> Result<(), Error> {
        self.join()
    }

    fn join(&mut self) -> Result<(), Error> {
        drop(self.jobs.take());
        match self.thread.take() {
            Some(thread) => thread.join().map_err(|_panic| Error::Closed),
            None => Ok(()),
        }
    }
}

impl<T: Send + 'static> Drop for MatterWorker<T> {
    fn drop(&mut self) {
        if let Err(err) = self.join() {
            tracing::warn!(error = %err, "the matter thread panicked");
        }
    }
}

/// The thread's loop: one job at a time until the actor drops its sender.
fn run<T>(
    settings: &Settings,
    jobs: &mpsc::Receiver<(MatterJob, T)>,
    post: &dyn Fn(T, MatterReply),
) {
    let mut slot = Slot::Empty;
    while let Ok((job, tag)) = jobs.recv() {
        let reply = handle(settings, &mut slot, job);
        post(tag, reply);
    }
}

/// Runs one job against the slot.
fn handle(settings: &Settings, slot: &mut Slot, job: MatterJob) -> MatterReply {
    match job {
        MatterJob::Create(new) => MatterReply::Created(
            Matter::create(settings.clone(), new)
                .map(|matter| *slot = Slot::Open(Box::new(matter)))
                .map_err(Error::from),
        ),
        MatterJob::Import {
            new,
            refused,
            cancel,
        } => MatterReply::Imported(import(settings, slot, new, &refused, &cancel)),
        MatterJob::Open { id, folder } => {
            *slot = Slot::Unopened { id, folder };
            MatterReply::Opened(open(settings, slot).map(|matter| matter.last_checkpoint()))
        }
        MatterJob::BeforeTurn(meta) => MatterReply::BeforeTurn(
            open(settings, slot).and_then(|matter| Ok(matter.before_turn(&meta)?)),
        ),
        MatterJob::AfterTurn { meta, tool_paths } => MatterReply::AfterTurn(
            open(settings, slot).and_then(|matter| Ok(matter.after_turn(&meta, &tool_paths)?)),
        ),
        MatterJob::EditedSince { before, after } => MatterReply::EditedSince(
            open(settings, slot).and_then(|matter| Ok(matter.edited_since(before, after)?)),
        ),
        MatterJob::Undo(job) => MatterReply::Undone(open(settings, slot).and_then(|matter| {
            let request = UndoRequest {
                before: job.before,
                after: job.after,
                decisions: &job.decisions,
                thread: job.thread,
                turn: job.turn,
                outside_edits: job.outside_edits,
                checkpoint: job.checkpoint,
                at: job.at,
            };
            Ok(matter.undo(&request)?)
        })),
    }
}

fn import(
    settings: &Settings,
    slot: &mut Slot,
    new: NewMatter,
    refused: &[PathBuf],
    cancel: &AtomicBool,
) -> Result<ImportEnd, Error> {
    match Matter::import(settings.clone(), new, refused, &mut |_progress| {}, cancel)? {
        Imported::Done {
            matter,
            summary,
            warnings,
        } => {
            *slot = Slot::Open(matter);
            Ok(ImportEnd::Done { summary, warnings })
        }
        Imported::Cancelled => Ok(ImportEnd::Cancelled),
    }
}

/// The open matter, opening it first when the last open failed.
fn open<'s>(settings: &Settings, slot: &'s mut Slot) -> Result<&'s mut Matter, Error> {
    if let Slot::Unopened { id, folder } = slot {
        let matter = Matter::open(settings.clone(), *id, folder)?;
        *slot = Slot::Open(Box::new(matter));
    }
    match slot {
        Slot::Open(matter) => Ok(matter),
        Slot::Empty | Slot::Unopened { .. } => Err(Error::Internal(
            "a matter job came before the matter was created, imported or opened".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests;
