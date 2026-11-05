//! One OS thread, `clauseo-matter`, per open matter. It owns the matter's R8 [`Matter`] and runs
//! its synchronous jobs (create, import, open, checkpoints, undo, restore, compare, preview, the
//! open-in-Word check, listing papers) off the actor, which sends each job with a tag and gets
//! each reply back through `post`, tagged the same way.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use clauseo_matters::{
    Checkpoint, CheckpointOutcome, CommitId, CompareSide, HeldOpen, ImportWarning, Imported,
    Limits, Matter, MissingReason, NewMatter, OsProbe, PaperList, Restore, RestorePlan, Restored,
    Retention, Settings, TimelineEntry, TurnMeta, UndoRequest, Undone, held_open_at,
    matter_path_in,
};
use clauseo_protocol::{
    CheckpointId, FileChange, FileDecision, FolderSupportKind, ImportSummary, MatterId, MatterPath,
    RestoreRequest, ThreadId, Timestamp, TurnId,
};

use crate::agent::folder_support::CheckSource;
use crate::error::Error;
use crate::log_file::Cause;
use crate::sent::TurnPaths;

/// The matter paths a turn's tool calls named, in `folder`: those `acp` placed inside the
/// session folder by text, and each `outside` path that [`matter_path_in`] resolves into
/// `folder` on the disk (a spelling through a symlink, or in another case where the file system
/// ignores case). The rest lie outside the matter.
pub(crate) fn tool_paths(folder: &Path, paths: TurnPaths) -> BTreeSet<MatterPath> {
    let TurnPaths {
        mut inside,
        outside,
    } = paths;
    for path in outside {
        if let Some(found) = matter_path_in(folder, Path::new(&path)) {
            inside.insert(found);
        }
    }
    inside
}

/// The name every matter thread carries.
const THREAD_NAME: &str = "clauseo-matter";

/// R8's settings for the host: matters under `<data folder>/matters`, the data folder refused
/// with everything inside it, the default limits, every checkpoint kept, and the OS's cloud-only
/// probe.
pub(crate) fn matter_settings(data_dir: &Path) -> Settings {
    Settings {
        matters_root: data_dir.join("matters"),
        refused_trees: vec![data_dir.to_path_buf()],
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

/// A restore request that owns its scope, so it can cross to the matter thread.
#[derive(Debug, Clone)]
pub(crate) struct RestoreJob {
    pub(crate) from: CheckpointId,
    pub(crate) scope: RestoreRequest,
    pub(crate) outside_edits: CheckpointId,
    pub(crate) checkpoint: CheckpointId,
    pub(crate) at: Timestamp,
}

/// One job for a matter thread.
#[derive(Debug)]
pub(crate) enum MatterJob {
    /// `Matter::create`; the worker keeps the matter. Its folder check uses `check`, the
    /// actor's source.
    Create { new: NewMatter, check: CheckSource },
    /// `Matter::import` with a no-op progress callback; `cancel` is set by shutdown.
    /// Before it, the place check (`check_place`) and then the registry: a folder that is,
    /// holds or sits inside one of `registry`'s folders is `matter.folder_is_matter`
    /// (`check_registry`). Its folder check uses `check`, the actor's source.
    Import {
        new: NewMatter,
        refused: Vec<PathBuf>,
        registry: Vec<Claimed>,
        cancel: Arc<AtomicBool>,
        check: CheckSource,
    },
    /// `Matter::open`. When it fails, the worker keeps the id and folder and tries the open
    /// again before the next job that needs the matter. With `relink`, the first open, if it
    /// finds `matter.json` deleted or unreadable, calls `Matter::relink` instead (see [`open`]).
    Open {
        id: MatterId,
        folder: PathBuf,
        relink: Option<Box<Relink>>,
    },
    /// "Locate folder": the registry check, then `Matter::locate` from `folder`. Answered as
    /// [`MatterReply::Located`]. On a refusal the matter is opened again from `current`, its
    /// recorded folder, before the next job that needs it.
    Locate {
        id: MatterId,
        folder: PathBuf,
        current: PathBuf,
        registry: Registry,
    },
    /// The before-turn checkpoint.
    BeforeTurn(TurnMeta),
    /// The after-turn checkpoint with the places the turn's tool calls named ([`tool_paths`]
    /// resolves them), and the turn's before-turn checkpoint when the host knows it (for a
    /// reused id's changes).
    AfterTurn {
        meta: TurnMeta,
        tool_paths: TurnPaths,
        before: Option<CheckpointId>,
    },
    /// The start-up repair of a turn the log left open, answered as [`MatterReply::AfterTurn`]:
    /// the turn's own after-turn checkpoint when R8's timeline has one, `before` when another
    /// turn started after it or another writer checkpointed since it started, a new after-turn
    /// checkpoint otherwise (see [`repair`]).
    Repair {
        meta: TurnMeta,
        tool_paths: TurnPaths,
        before: Option<CheckpointId>,
        /// No other open turn of the matter started after this one.
        last: bool,
    },
    /// Undo one step.
    Undo(UndoJob),
    /// Restore a version.
    Restore(RestoreJob),
    /// Compare a version with another version, or with the folder now (`to` is
    /// [`CompareSide::Folder`]; nothing is stored).
    Compare { from: CheckpointId, to: CompareSide },
    /// What restoring a version would do now; writes nothing.
    Preview {
        from: CheckpointId,
        scope: RestoreRequest,
    },
    /// The first of `paths` (as the agent named them: absolute, or relative to `folder`) that
    /// lies in `folder` and that Word or another program holds open, by
    /// `clauseo_matters::held_open_at`. Needs no open matter.
    HeldOpen {
        folder: PathBuf,
        paths: Vec<PathBuf>,
    },
    /// `Matter::papers`: the folder as it is now, its first `limit` files.
    ListPapers { limit: usize },
    /// `Matter::edited_since`: the files of the step from `before` to `after` whose content
    /// now differs from `after`; writes nothing.
    EditedSince {
        before: CheckpointId,
        after: CheckpointId,
    },
    /// `Matter::discard`: undoes the create or import this worker ran, for a matter the host
    /// could not record; the worker holds no matter afterwards. With none held, nothing to do.
    Discard,
}

/// How an import ended.
#[derive(Debug)]
pub(crate) enum ImportEnd {
    /// The matter exists; the worker keeps it.
    Done {
        /// The root checkpoint's commit.
        commit: CommitId,
        summary: ImportSummary,
        warnings: Vec<ImportWarning>,
        /// The folder check's answer (`agent/folder_support.rs`), made on this thread.
        folder_support: Option<FolderSupportKind>,
    },
    /// The cancel flag was set; nothing was kept.
    Cancelled,
}

/// A made matter's root commit and import summary, and its folder check's answer.
pub(crate) type Made = ((CommitId, Option<ImportSummary>), Option<FolderSupportKind>);

/// The root checkpoint's commit and the import summary of a create or import that succeeded
/// (`None` for a new folder), and the folder check's answer.
pub(crate) fn made(reply: MatterReply) -> Result<Made, Error> {
    match reply {
        MatterReply::Created(result) => result.map(|(commit, support)| ((commit, None), support)),
        MatterReply::Imported(result) => match result? {
            ImportEnd::Done {
                commit,
                summary,
                warnings,
                folder_support,
            } => {
                for warning in &warnings {
                    tracing::info!(?warning, "import warning");
                }
                Ok(((commit, Some(summary)), folder_support))
            }
            ImportEnd::Cancelled => Err(Error::Closed),
        },
        MatterReply::Opened(_)
        | MatterReply::Located(_)
        | MatterReply::BeforeTurn(_)
        | MatterReply::AfterTurn(_)
        | MatterReply::Undone(_)
        | MatterReply::Restored(_)
        | MatterReply::Compared(_)
        | MatterReply::Previewed(_)
        | MatterReply::EditedSince(_)
        | MatterReply::HeldOpen(_)
        | MatterReply::Papers(_)
        | MatterReply::Discarded(_) => Err(Error::Internal(
            "a matter's create got another job's reply".to_owned(),
        )),
    }
}

/// One job's result, one variant per [`MatterJob`] kind.
#[derive(Debug)]
pub(crate) enum MatterReply {
    /// The root checkpoint's commit, and the folder check's answer (`agent/folder_support.rs`),
    /// made on this thread.
    Created(Result<(CommitId, Option<FolderSupportKind>), Error>),
    Imported(Result<ImportEnd, Error>),
    /// The open matter's last checkpoint.
    Opened(Result<Option<CheckpointId>, Error>),
    /// The located matter's last checkpoint.
    Located(Result<Option<CheckpointId>, Error>),
    BeforeTurn(Result<TurnCheckpoint, Error>),
    AfterTurn(Result<TurnCheckpoint, Error>),
    Undone(Result<Box<Undone>, Error>),
    Restored(Result<Box<RestoreDone>, Error>),
    Compared(Result<Vec<FileChange>, Error>),
    Previewed(Result<Box<RestorePlan>, Error>),
    /// The first path held open, with what shows it.
    HeldOpen(Option<(MatterPath, HeldOpen)>),
    Papers(Result<PaperList, Error>),
    EditedSince(Result<Vec<MatterPath>, Error>),
    Discarded(Result<(), Error>),
}

impl MatterReply {
    /// The variant's name, for the log: its fields hold paths and file names.
    pub(crate) const fn name(&self) -> &'static str {
        match self {
            Self::Created(_) => "Created",
            Self::Imported(_) => "Imported",
            Self::Opened(_) => "Opened",
            Self::Located(_) => "Located",
            Self::BeforeTurn(_) => "BeforeTurn",
            Self::AfterTurn(_) => "AfterTurn",
            Self::Undone(_) => "Undone",
            Self::Restored(_) => "Restored",
            Self::Compared(_) => "Compared",
            Self::Previewed(_) => "Previewed",
            Self::HeldOpen(_) => "HeldOpen",
            Self::Papers(_) => "Papers",
            Self::EditedSince(_) => "EditedSince",
            Self::Discarded(_) => "Discarded",
        }
    }
}

/// A turn's checkpoint, and for an id R8 reused what the host needs to record it when its log
/// lost that checkpoint (a crash, or a commit that failed, between R8 and the log).
#[derive(Debug)]
pub(crate) struct TurnCheckpoint {
    pub(crate) checkpoint: Checkpoint,
    /// `Some` when the outcome is `Reused`.
    pub(crate) reused: Option<Reused>,
}

/// A restore R8 has done, with the timeline entry of the id it reused when it wrote nothing,
/// so the host can record that id late if the log lacks it.
#[derive(Debug)]
pub(crate) struct RestoreDone {
    pub(crate) restored: Restored,
    /// For a `Reused` restore checkpoint: its kind, thread and turn on R8's timeline.
    pub(crate) reused: Option<TimelineEntry>,
}

/// A reused checkpoint as R8's timeline has it.
#[derive(Debug, Clone)]
pub(crate) struct Reused {
    /// Its kind, thread and turn; `None` if the timeline does not hold the id.
    pub(crate) entry: Option<TimelineEntry>,
    /// For an after-turn checkpoint: the changes from the turn's before-turn checkpoint to it,
    /// empty when they are the same checkpoint or the before id is not known.
    pub(crate) changes: Vec<FileChange>,
    /// The id is the turn's own after-turn checkpoint, which the start-up repair found on the
    /// timeline: `changes` are the turn's even when the matter's log already records the id
    /// (another turn's start reused it), since the commit that would have logged them failed.
    pub(crate) own: bool,
}

/// What the worker holds.
enum Slot {
    /// No matter yet.
    Empty,
    /// The last open failed; tried again before the next job that needs the matter.
    Unopened {
        id: MatterId,
        folder: PathBuf,
        /// Taken by the first open, whatever it finds.
        relink: Option<Box<Relink>>,
    },
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
            tracing::warn!(error = %Cause(&err), "the matter thread panicked");
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
        MatterJob::Create { new, check } => {
            let (root, id, folder) = (new.checkpoint, new.id, new.folder.clone());
            MatterReply::Created(
                Matter::create(settings.clone(), new)
                    .map_err(Error::from)
                    .and_then(|matter| {
                        let commit = root_commit(&matter, root)?;
                        let folder_check = crate::agent::folder_support::FolderCheck {
                            matter: Some(id),
                            source: check,
                        };
                        let support = folder_check.run(&folder).row_kind();
                        *slot = Slot::Open(Box::new(matter));
                        Ok((commit, support))
                    }),
            )
        }
        MatterJob::Import {
            new,
            refused,
            registry,
            cancel,
            check,
        } => MatterReply::Imported(
            clauseo_matters::check_place(&new.folder, &refused)
                .map_err(Error::Matters)
                .and_then(|()| check_registry(&new.folder, &registry, true))
                .and_then(|()| import(settings, slot, new, &refused, &cancel, check)),
        ),
        MatterJob::Open { id, folder, relink } => {
            *slot = Slot::Unopened { id, folder, relink };
            MatterReply::Opened(open(settings, slot).map(|matter| matter.last_checkpoint()))
        }
        MatterJob::Locate {
            id,
            folder,
            current,
            registry,
        } => MatterReply::Located(locate(settings, slot, id, &folder, current, &registry)),
        MatterJob::BeforeTurn(meta) => {
            MatterReply::BeforeTurn(open(settings, slot).and_then(|matter| {
                let checkpoint = matter.before_turn(&meta)?;
                turn_checkpoint(matter, checkpoint, None)
            }))
        }
        MatterJob::AfterTurn {
            meta,
            tool_paths,
            before,
        } => MatterReply::AfterTurn(open(settings, slot).and_then(|matter| {
            let tool_paths = self::tool_paths(matter.folder(), tool_paths);
            let checkpoint = matter.after_turn(&meta, &tool_paths)?;
            turn_checkpoint(matter, checkpoint, before)
        })),
        MatterJob::Repair {
            meta,
            tool_paths,
            before,
            last,
        } => MatterReply::AfterTurn(open(settings, slot).and_then(|matter| {
            let tool_paths = self::tool_paths(matter.folder(), tool_paths);
            repair(matter, &meta, &tool_paths, before, last)
        })),
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
            Ok(Box::new(matter.undo(&request)?))
        })),
        MatterJob::Restore(job) => MatterReply::Restored(open(settings, slot).and_then(|matter| {
            let request = Restore {
                from: job.from,
                files: (&job.scope).into(),
                outside_edits: job.outside_edits,
                checkpoint: job.checkpoint,
                at: job.at,
            };
            let restored = matter.restore(&request)?;
            let reused = match restored.checkpoint.outcome {
                CheckpointOutcome::Reused(id) => timeline_entry(matter, id),
                CheckpointOutcome::Created(_) => None,
            };
            Ok(Box::new(RestoreDone { restored, reused }))
        })),
        MatterJob::Compare { from, to } => MatterReply::Compared(
            open(settings, slot)
                .and_then(|matter| Ok(matter.compare(CompareSide::Checkpoint(from), to)?)),
        ),
        MatterJob::Preview { from, scope } => MatterReply::Previewed(
            open(settings, slot)
                .and_then(|matter| Ok(Box::new(matter.preview_restore(from, (&scope).into())?))),
        ),
        MatterJob::HeldOpen { folder, paths } => MatterReply::HeldOpen(
            paths
                .into_iter()
                .find_map(|path| held_open_at(&folder, &path)),
        ),
        MatterJob::ListPapers { limit } => {
            MatterReply::Papers(open(settings, slot).and_then(|matter| Ok(matter.papers(limit)?)))
        }
        MatterJob::EditedSince { before, after } => MatterReply::EditedSince(
            open(settings, slot).and_then(|matter| Ok(matter.edited_since(before, after)?)),
        ),
        MatterJob::Discard => MatterReply::Discarded(match std::mem::replace(slot, Slot::Empty) {
            Slot::Open(matter) => matter.discard().map_err(Error::from),
            Slot::Empty | Slot::Unopened { .. } => Ok(()),
        }),
    }
}

/// The commit of a new matter's root checkpoint `root`.
fn root_commit(matter: &Matter, root: CheckpointId) -> Result<CommitId, Error> {
    matter
        .commit_of(root)
        .ok_or_else(|| Error::Internal(format!("the new matter has no checkpoint {root}")))
}

fn import(
    settings: &Settings,
    slot: &mut Slot,
    new: NewMatter,
    refused: &[PathBuf],
    cancel: &AtomicBool,
    check: CheckSource,
) -> Result<ImportEnd, Error> {
    let (root, id, folder) = (new.checkpoint, new.id, new.folder.clone());
    match Matter::import(settings.clone(), new, refused, &mut |_progress| {}, cancel)? {
        Imported::Done {
            matter,
            summary,
            warnings,
        } => {
            let commit = root_commit(&matter, root)?;
            let folder_check = crate::agent::folder_support::FolderCheck {
                matter: Some(id),
                source: check,
            };
            let folder_support = folder_check.run(&folder).row_kind();
            *slot = Slot::Open(matter);
            Ok(ImportEnd::Done {
                commit,
                summary,
                warnings,
                folder_support,
            })
        }
        Imported::Cancelled => Ok(ImportEnd::Cancelled),
    }
}

/// The open matter, opening it first when the last open failed.
///
/// The first open after the job that gave a [`Relink`], when it finds `matter.json` deleted or
/// unreadable, calls `Matter::relink` (which writes it again only in the folder the history
/// records), after checking that no other registered matter claims the folder. The first open
/// spends the relink whatever it finds; every later open is answered as it is.
fn open<'s>(settings: &Settings, slot: &'s mut Slot) -> Result<&'s mut Matter, Error> {
    if let Slot::Unopened { id, folder, relink } = slot {
        // Spent by this open whatever it finds: a later open may meet a folder another matter
        // has claimed since the registry was taken.
        let relink = relink.take();
        let matter = match Matter::open(settings.clone(), *id, folder) {
            Ok(matter) => matter,
            Err(
                error @ clauseo_matters::Error::FolderMissing {
                    reason: MissingReason::NoMatterJson | MissingReason::Unreadable,
                    ..
                },
            ) => match relink {
                Some(relink) => relink_matter(settings, *id, folder, &relink, error)?,
                None => return Err(error.into()),
            },
            Err(error) => return Err(error.into()),
        };
        *slot = Slot::Open(Box::new(matter));
    }
    match slot {
        Slot::Open(matter) => Ok(matter),
        Slot::Empty | Slot::Unopened { .. } => Err(Error::Internal(
            "a matter job came before the matter was created, imported or opened".to_owned(),
        )),
    }
}

mod checkpoints;
mod claims;
mod relocate;

use checkpoints::{repair, timeline_entry, turn_checkpoint};
pub(crate) use claims::{Claim, Claimed, check_registry};
pub(crate) use relocate::{Registry, Relink};
use relocate::{locate, relink_matter};

#[cfg(test)]
mod tests;
