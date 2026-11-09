//! `WordBlackline`: one Word document's text between a version and another version or the
//! folder now, as stetdoc's redline shows it.
//!
//! It answers, in the order the query runs:
//! 1. what is refused at once, on the actor: a path that does not end in `.docx` (any case) is
//!    `blackline.not_word`, and a host with no stetdoc (`HostConfig::with_stetdoc`)
//!    `blackline.no_word_tool`; then the matter and its versions are checked as
//!    `CompareVersions` checks them (`query.rs`'s `own_versions`);
//! 2. where the sides go: the matter's thread copies both into a fresh
//!    `<data>/tmp/blackline-<id>/` (`matter_worker/sides.rs`), and copies only;
//! 3. where stetdoc runs: on a `clauseo-blackline` thread of its own, never the actor
//!    (`run.rs`), whose answer is read by `parse.rs`;
//! 4. what the caller gets: the blackline, or `blackline.failed` for any failure of the runs
//!    or the read, whose detail goes to the log only (never document text);
//! 5. how the folder goes: its guard (`BlacklineDir`) removes it on every path, before the
//!    answer is sent; a folder a crash or quit left is removed at the next start
//!    ([`sweep_leftovers`]). Nothing is written in the matter.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_matters::CompareSide;
use clauseo_protocol::{ErrorCode, HostQuery, MatterPath, QueryResult, StreamId, Timestamp};

use super::own_versions;
use crate::actor::Actor;
use crate::actor::input::{MatterTag, Reply};
use crate::clock::ProcessTimer;
use crate::config::Stetdoc;
use crate::error::Error;
use crate::log_file::Cause;
use crate::matter_worker::{BlacklineDir, MatterJob, MatterReply, Sides};

mod parse;
mod run;

/// The name of the thread stetdoc runs on.
const THREAD_NAME: &str = "clauseo-blackline";

/// The name of the start-up sweep's thread.
const SWEEP_THREAD: &str = "clauseo-blackline-sweep";

/// What a failed blackline's `HostError.message` says: always this, so no detail of the run
/// (a path, a code of stetdoc's) reaches the client; the detail is logged.
const FAILED_MESSAGE: &str = "the Word blackline failed; the detail is in the host's log";

/// The prefix of every blackline folder's name in `<data>/tmp/`.
const DIR_PREFIX: &str = "blackline-";

/// What the blackline goes on with once the matter's thread has written the sides.
pub(crate) struct Ask {
    reply: Reply<QueryResult>,
    stetdoc: Stetdoc,
    launcher: Arc<dyn clauseo_process::Launcher>,
    timer: ProcessTimer,
}

impl std::fmt::Debug for Ask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ask")
            .field("stetdoc", &self.stetdoc)
            .finish_non_exhaustive()
    }
}

/// Answers `WordBlackline` (`query` is one): the checks, then the sides on the matter's thread.
pub(super) fn word_blackline(actor: &mut Actor, query: &HostQuery, reply: Reply<QueryResult>) {
    let HostQuery::WordBlackline {
        matter,
        ref path,
        from,
        to,
    } = *query
    else {
        let _gone = reply.send(Err(Error::Internal("not a WordBlackline".to_owned())));
        return;
    };
    let path = path.clone();
    if !is_docx(&path) {
        let refused = Error::refused(ErrorCode::BlacklineNotWord, "the path is not a .docx");
        let _gone = reply.send(Err(refused));
        return;
    }
    let Some(stetdoc) = actor.wiring.stetdoc.clone() else {
        let refused = Error::refused(ErrorCode::BlacklineNoWordTool, "stetdoc is not configured");
        let _gone = reply.send(Err(refused));
        return;
    };
    if let Err(error) = actor.check_matter(matter) {
        let _gone = reply.send(Err(error));
        return;
    }
    let ask = Box::new(Ask {
        reply,
        stetdoc,
        launcher: Arc::clone(&actor.launcher),
        timer: ProcessTimer(Arc::clone(&actor.clock)),
    });
    let stream = StreamId::Matter(matter);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let (versions, side) = match to {
                Some(to) => (vec![from, to], CompareSide::Checkpoint(to)),
                None => (vec![from], CompareSide::Folder),
            };
            let checked = loaded
                .and_then(|()| own_versions(actor, matter, &versions))
                .and_then(|()| temp_dir(actor))
                .and_then(|dir| Ok((dir, actor.matter_worker(matter)?)));
            match checked {
                Ok((dir, worker)) => {
                    let job = MatterJob::WriteSides(Sides {
                        from,
                        to: side,
                        path,
                        dir,
                    });
                    // A refused send drops the tag and with it the reply: the caller sees the
                    // host closed.
                    match worker.send(job, MatterTag::Blackline(ask)) {
                        Ok(()) => actor.work.matter_jobs += 1,
                        Err(error) => {
                            tracing::warn!(%matter, error = %Cause(&error), "a blackline was not sent");
                        }
                    }
                }
                Err(error) => {
                    let _gone = ask.reply.send(Err(error));
                }
            }
            actor.release(stream);
        }),
    );
}

/// Whether `path` names a Word document: its name ends in `.docx`, in any case.
fn is_docx(path: &MatterPath) -> bool {
    let name = path.as_str().rsplit('/').next().unwrap_or_default();
    name.len() > ".docx".len()
        && name
            .get(name.len() - ".docx".len()..)
            .is_some_and(|end| end.eq_ignore_ascii_case(".docx"))
}

/// A fresh folder for one blackline, not made yet: `<data>/tmp/blackline-<id>`, the id a new
/// v7 uuid from the host's ids.
fn temp_dir(actor: &mut Actor) -> Result<PathBuf, Error> {
    let wall = actor.now().wall;
    let id: clauseo_protocol::FileRefId = actor.ids.mint(wall)?;
    let name = format!("{DIR_PREFIX}{}", id.as_uuid().simple());
    Ok(actor.effective.data_dir().join("tmp").join(name))
}

/// The matter's thread answered a blackline's sides: with the folder, stetdoc runs on a thread
/// of its own, which answers; with an error, that is the answer.
pub(in crate::actor) fn sides_written(ask: Box<Ask>, reply: MatterReply) {
    let written = if let MatterReply::SidesWritten(written) = reply {
        written
    } else {
        Err(Error::Internal(format!(
            "a blackline answered with another job's reply: {}",
            reply.name()
        )))
    };
    let dir = match written {
        Ok(dir) => dir,
        Err(error) => {
            let _gone = ask.reply.send(Err(error));
            return;
        }
    };
    let started = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || run_stetdoc(*ask, dir));
    if let Err(error) = started {
        // The closure went with the failed start, and the reply with it: the caller sees the
        // host closed.
        tracing::error!(%error, "could not start {THREAD_NAME}");
    }
}

/// On the blackline's thread: both runs and the read, then the folder removed, then the answer.
fn run_stetdoc(ask: Ask, dir: BlacklineDir) {
    let Ask {
        reply,
        stetdoc,
        launcher,
        timer,
    } = ask;
    let ran = futures::executor::block_on(run::blackline(&*launcher, &timer, &stetdoc, dir.path()));
    drop(dir);
    // The detail goes to the log alone; the caller gets a fixed message.
    let answer = ran.map(QueryResult::WordBlackline).map_err(|failed| {
        tracing::warn!(%failed, "the Word blackline failed");
        Error::refused(ErrorCode::BlacklineFailed, FAILED_MESSAGE)
    });
    let _gone = reply.send(answer);
}

/// At host start, on a thread of its own: removes the blackline folders an earlier run left in
/// `<data>/tmp/` (a crash or a quit while stetdoc ran), and nothing else there. A folder is an
/// earlier run's when its name is `blackline-<id>` and the id's time is before `started`, or
/// the rest of its name is no id at all; a folder this run makes has an id from its start or
/// later, so the sweep never races a blackline of this run. Links and files are left.
pub(in crate::actor) fn sweep_leftovers(data_dir: &Path, started: Timestamp) {
    let tmp = data_dir.join("tmp");
    let started = std::thread::Builder::new()
        .name(SWEEP_THREAD.to_owned())
        .spawn(move || {
            let removed = sweep(&tmp, started);
            if removed > 0 {
                tracing::info!(removed, "removed leftover Word blackline folders");
            }
        });
    if let Err(error) = started {
        tracing::warn!(%error, "could not start {SWEEP_THREAD}");
    }
}

/// The sweep itself: how many folders it removed.
fn sweep(tmp: &Path, started: Timestamp) -> usize {
    let Ok(entries) = std::fs::read_dir(tmp) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name.to_str().and_then(|name| name.strip_prefix(DIR_PREFIX)) else {
            continue;
        };
        let earlier = uuid::Uuid::try_parse(rest)
            .ok()
            .and_then(|id| id.get_timestamp())
            .map(|ts| ts.to_unix())
            .is_none_or(|(secs, nanos)| {
                let millis = i128::from(secs) * 1000 + i128::from(nanos / 1_000_000);
                millis < i128::from(started.as_millis())
            });
        let path = entry.path();
        if earlier && crate::write::is_real_dir(&path) {
            match crate::write::remove_tree(&path) {
                Ok(()) => removed += 1,
                Err(error) => tracing::warn!(%error, "a leftover blackline folder stayed"),
            }
        }
    }
    removed
}
