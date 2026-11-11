//! Tracked changes in a matter's Word file (`ListRevisions`, `DecideRevisions`): the checks
//! the actor makes before the matter's thread does the work (`crate::revisions`). A path that
//! does not end in `.docx` is `word.not_docx`, and a host with no stetdoc program
//! (`HostConfig::with_stetdoc`) answers `word.stetdoc_missing`, both before anything
//! is sent. A list needs no writer and logs nothing; a decision is a restore's kind of write
//! (`turn/restore/revisions.rs`).

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{DecideRevisions, ErrorCode, MatterId, MatterPath, QueryResult};

use super::Actor;
use super::commands::CommandCtx;
use super::input::{MatterTag, Reply};
use super::matters::matter_worker;
use super::turn;
use crate::error::Error;
use crate::matter_worker::MatterJob;
use crate::revisions::{Act, RevisionsJob, WORK_FOLDER, is_docx};

/// The stetdoc program, once `path` is a `.docx` and `matter` is on the list.
fn checked(actor: &Actor, matter: MatterId, path: &MatterPath) -> Result<PathBuf, Error> {
    if !is_docx(path) {
        return Err(Error::refused(
            ErrorCode::WordNotDocx,
            "tracked changes are read only in a .docx file",
        ));
    }
    actor.check_matter(matter)?;
    let stetdoc = actor.wiring.stetdoc.as_ref();
    stetdoc
        .map(|stetdoc| stetdoc.program.clone())
        .ok_or_else(|| {
            Error::refused(
                ErrorCode::WordStetdocMissing,
                "Clauseo has no stetdoc program to read Word files with",
            )
        })
}

/// The matter's private folder for stetdoc's copies, `<data>/revisions/<matter id>`.
pub(super) fn work_folder(actor: &Actor, matter: MatterId) -> PathBuf {
    actor
        .effective
        .data_dir()
        .join(WORK_FOLDER)
        .join(matter.to_string())
}

/// Removes `<data>/revisions` with everything in it: the private copies of client documents a
/// job left when the host stopped mid-way (a crash; a job that ends removes its own). Runs at
/// start-up, before any job can make a folder there, so it races none. Each job leaves at most
/// its three files, so it is quick; a failure is logged and start-up goes on.
pub(super) fn sweep_leftovers(actor: &Actor) {
    let root = actor.effective.data_dir().join(WORK_FOLDER);
    if let Err(err) = crate::write::remove_tree(&root) {
        tracing::warn!(
            error = %crate::log_file::Cause(&err),
            "left-over tracked-changes copies were not removed"
        );
    }
}

/// `ListRevisions`: read on the matter's thread, answered as a query R8 answers.
pub(super) fn list(
    actor: &mut Actor,
    matter: MatterId,
    path: MatterPath,
    reply: Reply<QueryResult>,
) {
    let job = checked(actor, matter, &path).map(|stetdoc| RevisionsJob {
        path,
        stetdoc,
        work: work_folder(actor, matter),
        clock: Arc::clone(&actor.clock),
        act: Act::List,
    });
    let worker = job.and_then(|job| Ok((job, matter_worker(actor, matter)?)));
    match worker {
        Ok((job, worker)) => {
            match worker.send(MatterJob::Revisions(Box::new(job)), MatterTag::Query(reply)) {
                Ok(()) => actor.work.matter_jobs += 1,
                // The reply went with the job; dropping it answers that the host is gone.
                Err(error) => tracing::warn!(%matter, %error, "a revisions list was not sent"),
            }
        }
        Err(error) => {
            let _gone = reply.send(Err(error));
        }
    }
}

/// `DecideRevisions`: checked here, then written as a restore is.
pub(super) fn decide(actor: &mut Actor, ctx: CommandCtx, decide: DecideRevisions) {
    match checked(actor, decide.matter, &decide.path) {
        Ok(stetdoc) => turn::decide_revisions(actor, ctx, decide, stetdoc),
        Err(error) => actor.finish_command(ctx.id, Err(error)),
    }
}
