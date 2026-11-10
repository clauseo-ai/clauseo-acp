//! `ListPapers`: a matter's folder as it is now, listed on the matter's worker (never on the
//! actor), each paper with a [`FileRefId`] from the host's file registry, [`FileRefs`], which
//! `RegisterFile` shares, so a path keeps one id for the host's run. Whether a `.docx` holds
//! tracked changes comes from what the matter's background scan has read ([`scan`]): the
//! listing answers at once, and the scan sends `PapersChanged` when it knows more. The open
//! matter's folder is watched, except on Windows (`follow.rs`, `watch.rs`), and a listing says
//! what it found of the folder there.

pub(crate) mod scan;

use std::collections::BTreeMap;
use std::path::PathBuf;

use clauseo_matters::PaperList;
use clauseo_protocol::{
    Count, ErrorCode, FileRefId, MAX_PAPERS, MatterId, MatterPath, PaperSummary, QueryResult,
};

use super::Actor;
use super::input::{MatterTag, Reply};
use super::matters::matter_worker;
use crate::error::Error;
use crate::log_file::Cause;
use crate::matter_worker::{MatterJob, MatterReply};

pub(crate) mod follow;
#[cfg(not(windows))]
mod live;
#[cfg(windows)]
mod no_watch;
#[cfg(not(windows))]
mod watch;
#[cfg(not(windows))]
pub(crate) use live::{Follow, on_changed, on_started};
#[cfg(windows)]
use no_watch as live;
#[cfg(windows)]
pub(crate) use no_watch::Follow;
#[cfg(not(windows))]
pub(crate) use watch::{FolderChanged, WatchStarted};

/// The files the host has handed out ids for, both ways, kept in host memory for its run.
#[derive(Debug, Default)]
pub(crate) struct FileRefs {
    paths: BTreeMap<FileRefId, PathBuf>,
    ids: BTreeMap<PathBuf, FileRefId>,
}

impl FileRefs {
    /// The path `id` names.
    pub(crate) fn get(&self, id: &FileRefId) -> Option<&PathBuf> {
        self.paths.get(id)
    }
}

/// The id of the file at `path`: the one it already has (compared as written, so another
/// spelling of the same file gets another id), else a new one.
pub(super) fn file_ref(actor: &mut Actor, path: PathBuf) -> Result<FileRefId, Error> {
    if let Some(id) = actor.files.ids.get(&path) {
        return Ok(*id);
    }
    let id = actor.ids.mint::<FileRefId>(actor.clock.now().wall)?;
    actor.files.paths.insert(id, path.clone());
    actor.files.ids.insert(path, id);
    Ok(id)
}

/// Sends `matter`'s worker the listing (the worker opens the matter first if it must);
/// [`answer`] replies once it is back.
pub(super) fn list(actor: &mut Actor, matter: MatterId, reply: Reply<QueryResult>) {
    let limit = usize::try_from(MAX_PAPERS).unwrap_or(usize::MAX);
    let folder = match actor.check_matter(matter) {
        Ok(entry) => PathBuf::from(&entry.row.folder_path),
        Err(error) => {
            let _gone = reply.send(Err(error));
            return;
        }
    };
    // The room lists the papers of the matter it shows, which the app may reopen at launch
    // without an `OpenMatter`: that matter is the one followed.
    follow::opened(actor, matter);
    let worker = match matter_worker(actor, matter) {
        Ok(worker) => worker,
        Err(error) => {
            let _gone = reply.send(Err(error));
            return;
        }
    };
    match worker.send(
        MatterJob::ListPapers { limit },
        MatterTag::Papers { reply, folder },
    ) {
        Ok(()) => actor.work.matter_jobs += 1,
        // The reply went with the job, and dropping it answers the caller that the host is
        // gone.
        Err(error) => {
            tracing::warn!(%matter, error = %Cause(&error), "a matter's worker refused a listing");
        }
    }
}

/// The worker's listing of `matter`'s `folder`, answered as `QueryResult::Papers`.
pub(super) fn answer(
    actor: &mut Actor,
    matter: MatterId,
    listed: MatterReply,
    reply: Reply<QueryResult>,
    folder: &std::path::Path,
) {
    let answer = match listed {
        MatterReply::Papers(listed) => {
            match &listed {
                Ok(_) => follow::listed(actor, matter, folder, true),
                Err(error) if error.code() == ErrorCode::MatterFolderMissing => {
                    follow::listed(actor, matter, folder, false);
                }
                Err(_) => {}
            }
            listed.and_then(|papers| summarise(actor, matter, papers))
        }
        MatterReply::Created(_)
        | MatterReply::Imported(_)
        | MatterReply::Opened(_)
        | MatterReply::BeforeTurn(_)
        | MatterReply::AfterTurn(_)
        | MatterReply::Undone(_)
        | MatterReply::Restored(_)
        | MatterReply::Compared(_)
        | MatterReply::Previewed(_)
        | MatterReply::EditedSince(_)
        | MatterReply::Located(_)
        | MatterReply::HeldOpen(_)
        | MatterReply::StillGone(_)
        | MatterReply::SidesWritten(_)
        | MatterReply::Revisions(_)
        | MatterReply::Discarded(_) => Err(Error::Internal(
            "a matter's listing got another job's reply".to_owned(),
        )),
    };
    let _gone = reply.send(answer);
}

/// `papers` with each file's id, minted on first sight of its path in `matter`'s folder, and
/// whether a `.docx` holds tracked changes or comments as far as the scan has read.
fn summarise(actor: &mut Actor, matter: MatterId, papers: PaperList) -> Result<QueryResult, Error> {
    let folder = PathBuf::from(&actor.check_matter(matter)?.row.folder_path);
    let tracked = scan::listed(actor, matter, &folder, &papers.papers);
    let mut summaries = Vec::with_capacity(papers.papers.len());
    for paper in papers.papers {
        let file = file_ref(actor, absolute(&folder, &paper.path))?;
        let tracked = tracked.contains(&paper.path);
        summaries.push(PaperSummary {
            file,
            path: paper.path,
            size: Count::clamped(paper.size),
            modified: paper.modified,
            tracked,
        });
    }
    Ok(QueryResult::Papers {
        papers: summaries,
        skipped: papers.skipped,
        unread: papers.unread,
        truncated: papers.truncated,
    })
}

/// `path` under `folder`, one component at a time, so it is spelled with the platform's
/// separator as a picked file's path is.
fn absolute(folder: &std::path::Path, path: &MatterPath) -> PathBuf {
    let mut absolute = folder.to_path_buf();
    absolute.extend(path.as_str().split('/'));
    absolute
}
