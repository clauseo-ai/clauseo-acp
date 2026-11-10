//! Tracked changes in a matter's Word file, the matter thread's side: `ListRevisions` and
//! `DecideRevisions` run here as one [`RevisionsJob`] each, so the actor never waits on stetdoc
//! and a decision holds the matter (and the host's writer) for its whole length.
//!
//! stetdoc only ever works on private copies in `<data>/revisions/<matter id>/`, never in the
//! matter folder: the file is read once into `read.docx` (hashed: the [`DocVersion`]), stetdoc
//! reads that copy and writes `out.docx` there, and `Matter::replace_file` writes the result into
//! the folder between two checkpoints. The folder belongs to this matter's thread, which runs one
//! job at a time: each job empties it first (a crash's leftovers go then) and removes it after,
//! whatever happened.
//!
//! stetdoc starts through a passthrough launcher with a clean environment (on Windows only
//! `SystemRoot`, and `TEMP` and `TMP` pointing into the job's folder), not the agents'
//! launcher: that one may be the sandbox's, which would keep stetdoc from the data folder.

mod parse;
mod stetdoc;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_matters::{CheckpointOutcome, Matter, ReplaceFile, Replaced, TimelineEntry};
use clauseo_protocol::{
    CheckpointId, DocVersion, ErrorCode, MatterPath, RevisionDecision, RevisionList, Timestamp,
};
use sha2::{Digest as _, Sha256};

use crate::clock::Clock;
use crate::error::Error;
use crate::log_file::Cause;
use crate::write;

pub(crate) use parse::is_docx;

/// The folder under the data folder that holds each matter's private copies.
pub(crate) const WORK_FOLDER: &str = "revisions";

/// One tracked-changes job for a matter's thread.
pub(crate) struct RevisionsJob {
    /// The Word file.
    pub(crate) path: MatterPath,
    /// The stetdoc program.
    pub(crate) stetdoc: PathBuf,
    /// The matter's private folder, `<data>/revisions/<matter id>`.
    pub(crate) work: PathBuf,
    /// For stetdoc's deadline.
    pub(crate) clock: Arc<dyn Clock>,
    /// List, or decide.
    pub(crate) act: Act,
}

impl std::fmt::Debug for RevisionsJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RevisionsJob")
            .field("act", &self.act)
            .finish_non_exhaustive()
    }
}

/// What a [`RevisionsJob`] does.
#[derive(Debug, Clone)]
pub(crate) enum Act {
    /// Read the file's tracked changes.
    List,
    /// Accept or reject `ids` in the file whose bytes are `version`, and write it.
    Decide {
        version: DocVersion,
        ids: Vec<u32>,
        decision: RevisionDecision,
        /// The `outside-edits` checkpoint's id, used if the folder changed.
        outside_edits: CheckpointId,
        /// The `revisions` checkpoint's id.
        checkpoint: CheckpointId,
        at: Timestamp,
    },
}

/// A [`RevisionsJob`]'s answer.
#[derive(Debug)]
pub(crate) enum RevisionsDone {
    Listed(Result<RevisionList, Error>),
    Decided(Result<Box<Decided>, Error>),
}

/// A decision written, with the timeline entries of the checkpoints it names that it did not
/// create (`before` when no outside-edits checkpoint was taken, a reused `revisions` id): an
/// earlier attempt may have committed one that the matter's log never recorded, which the
/// actor then records late, as a restore's reused id.
#[derive(Debug)]
pub(crate) struct Decided {
    pub(crate) replaced: Replaced,
    /// `before`'s entry, `None` if the timeline does not hold it.
    pub(crate) before: Option<TimelineEntry>,
    /// The reused `revisions` id's entry; `None` when it was created.
    pub(crate) reused: Option<TimelineEntry>,
}

/// [`run`] as the matter thread's reply to a `MatterJob::Revisions`.
pub(crate) fn reply(
    matter: Result<&mut Matter, Error>,
    job: &RevisionsJob,
) -> crate::matter_worker::MatterReply {
    crate::matter_worker::MatterReply::Revisions(Box::new(run(matter, job)))
}

/// Runs `job` against the open `matter` (or the error opening it), then removes the job's
/// private folder.
pub(crate) fn run(matter: Result<&mut Matter, Error>, job: &RevisionsJob) -> RevisionsDone {
    let done = match job.act {
        Act::List => RevisionsDone::Listed(matter.and_then(|matter| list(matter, job))),
        Act::Decide { .. } => {
            RevisionsDone::Decided(matter.and_then(|matter| decide(matter, job).map(Box::new)))
        }
    };
    if let Err(err) = write::remove_tree(&job.work) {
        tracing::warn!(error = %Cause(&err), "a tracked-changes work folder was left behind");
    }
    done
}

/// The file's tracked changes, read with stetdoc from a private copy.
fn list(matter: &Matter, job: &RevisionsJob) -> Result<RevisionList, Error> {
    let copy = Copy::take(matter, job, ErrorCode::CommandInvalid)?;
    let run = stetdoc::Runner::new(&job.stetdoc, &job.clock, &job.work);
    let check = run.json(
        &["check".into(), "--json".into(), copy.read.clone().into()],
        false,
    )?;
    let view = run.json(
        &[
            "view".into(),
            copy.read.clone().into(),
            "--revisions".into(),
            "--full".into(),
            "--all".into(),
            "--json".into(),
        ],
        false,
    )?;
    let (revisions, truncated) = parse::revisions(&check, &view)?;
    Ok(RevisionList {
        path: job.path.clone(),
        version: copy.version,
        revisions,
        truncated,
    })
}

/// Accepts or rejects the ids on a private copy, then writes the result into the folder.
fn decide(matter: &mut Matter, job: &RevisionsJob) -> Result<Decided, Error> {
    let Act::Decide {
        version,
        ref ids,
        decision,
        outside_edits,
        checkpoint,
        at,
    } = job.act
    else {
        return Err(Error::Internal("a list job reached decide".to_owned()));
    };
    let copy = Copy::take(matter, job, ErrorCode::WordRevisionsStale)?;
    if copy.version != version {
        return Err(Error::refused(
            ErrorCode::WordRevisionsStale,
            "the file changed since its tracked changes were listed",
        ));
    }
    if clauseo_matters::held_open(matter.folder(), &job.path).is_some() {
        return Err(Error::Matters(clauseo_matters::Error::FileLocked {
            paths: vec![job.path.clone()],
        }));
    }
    let ops = job.work.join("ops.txt");
    write::replace_private_file(&ops, parse::batch(decision, ids).as_bytes())
        .map_err(|err| io_error("write the batch", &err))?;
    let out = job.work.join("out.docx");
    stetdoc::Runner::new(&job.stetdoc, &job.clock, &job.work).json(
        &[
            "edit".into(),
            copy.read.clone().into(),
            "--ops".into(),
            ops.into(),
            "-o".into(),
            out.clone().into(),
            "--author".into(),
            "Clauseo".into(),
            "--json".into(),
        ],
        true,
    )?;
    let request = ReplaceFile {
        path: &job.path,
        expected: &copy.read,
        new: &out,
        outside_edits,
        checkpoint,
        at,
    };
    let replaced = matter.replace_file(&request)?;
    let entry = |id: CheckpointId| matter.timeline().iter().rev().find(|e| e.id == id).copied();
    let before = entry(replaced.before);
    let reused = match replaced.checkpoint.as_ref().map(|taken| taken.outcome) {
        Some(CheckpointOutcome::Reused(id)) => entry(id),
        Some(CheckpointOutcome::Created(_)) | None => None,
    };
    Ok(Decided {
        replaced,
        before,
        reused,
    })
}

/// The file's bytes as read once, copied into the job's private folder, and their hash.
struct Copy {
    read: PathBuf,
    version: DocVersion,
}

impl Copy {
    /// Empties the job's folder, then copies the matter's file into it. A path missing or
    /// reached through a link out of the folder is `missing`; one that is there but not a
    /// regular file (a folder) is `word.not_docx`. The file is read with downloads off: one that
    /// is cloud-only or over the size cap is never opened and is `word.edit_failed`
    /// (`Matter::read_local`), as no write of it could be checkpointed.
    fn take(matter: &Matter, job: &RevisionsJob, missing: ErrorCode) -> Result<Self, Error> {
        let no_such = || Error::refused(missing, "the matter has no such file");
        match inside(matter.folder(), &job.path) {
            Inside::File => {}
            Inside::Missing => return Err(no_such()),
            Inside::NotAFile => {
                return Err(Error::refused(
                    ErrorCode::WordNotDocx,
                    "the path is not a file",
                ));
            }
        }
        let bytes = matter.read_local(&job.path).map_err(|err| {
            if let clauseo_matters::Error::Io { source, .. } = &err {
                if source.kind() == std::io::ErrorKind::NotFound {
                    return no_such();
                }
                return io_error("read the file", source);
            }
            Error::Matters(err)
        })?;
        write::remove_tree(&job.work).map_err(|err| io_error("empty the work folder", &err))?;
        if let Some(parent) = job.work.parent() {
            write::create_dir_all(parent).map_err(|err| io_error("make the folder", &err))?;
        }
        write::create_private_dir(&job.work).map_err(|err| io_error("make the folder", &err))?;
        let read = job.work.join("read.docx");
        write::replace_private_file(&read, &bytes)
            .map_err(|err| io_error("copy the file", &err))?;
        let version = DocVersion::from_bytes(Sha256::digest(&bytes).into());
        Ok(Self { read, version })
    }
}

/// What `folder`/`path` is.
#[derive(Debug, PartialEq, Eq)]
enum Inside {
    /// A regular file whose real path is inside the folder: no link on the way leads out.
    File,
    /// Not there, or reached through a link out of the matter.
    Missing,
    /// Inside the folder, but not a regular file.
    NotAFile,
}

fn inside(folder: &Path, path: &MatterPath) -> Inside {
    let abs = folder.join(path.as_str());
    let (Ok(real), Ok(root)) = (dunce::canonicalize(&abs), dunce::canonicalize(folder)) else {
        return Inside::Missing;
    };
    if !real.starts_with(&root) {
        return Inside::Missing;
    }
    match std::fs::metadata(&real) {
        Ok(metadata) if metadata.is_file() => Inside::File,
        Ok(_) => Inside::NotAFile,
        Err(_) => Inside::Missing,
    }
}

/// A failed read or write of the copies: `word.edit_failed`, retryable.
fn io_error(what: &str, err: &std::io::Error) -> Error {
    tracing::warn!(error = %Cause(err), what, "tracked changes: a file step failed");
    Error::refused(ErrorCode::WordEditFailed, format!("could not {what}"))
}

#[cfg(test)]
mod tests;
