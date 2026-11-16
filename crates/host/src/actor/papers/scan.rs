//! Each matter's background scan for tracked changes in its `.docx` papers (`revisions/marks.rs`
//! reads one file). A listing never waits for it: [`listed`] answers from what the matter's
//! scans have read ([`Kept`]), a paper not yet read showing `tracked: false`, and hands the
//! papers not yet read to the scan. The scan runs on a short-lived `clauseo-marks` thread, never
//! on the actor or the matter's worker, so a checkpoint never waits for it.
//!
//! - One scan thread per matter at a time; papers a listing adds while one runs wait for it, so
//!   no file is read by two scans at once.
//! - Each thread reads one batch: at most [`BATCH_FILES`] files, and none started after
//!   [`BATCH_TIME`] on the host clock; each file's read is bounded by `MAX_XML_BYTES` a part.
//! - When a batch ends the actor keeps its answers, then publishes one
//!   `HostEvent::PapersChanged { matter }` (the matter's id only, never logged, in no stream) if
//!   the batch found marks in a paper the matter's newest listing showed without them. The app
//!   lists the papers again on it, and that listing answers from what was read. So 2,000
//!   `.docx` cost about four events, at most one a [`BATCH_TIME`] on a slow disk, never one a
//!   file.
//! - Stale results publish nothing: a batch for a matter that is gone or whose folder moved is
//!   dropped, and answers for papers that changed since (another size or time) are not shown.
//! - A read that failed is not kept, and not tried again until the next listing.
//!
//! [`rescan`] is the entry point for anything else that knows papers changed (a folder watcher):
//! it reads the given papers in the background, then pushes `PapersChanged`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_matters::Paper;
use clauseo_protocol::{HostEvent, MatterId, MatterPath};

use super::super::Actor;
use super::super::input::Input;
use crate::clock::Clock;
use crate::log_file::Cause;
use crate::revisions::{Kept, check};

/// The most files one scan thread reads.
pub(crate) const BATCH_FILES: usize = 500;

/// No file is started after this long into a batch: on a slow disk a batch ends early, so the
/// app hears of what was read at least this often.
pub(crate) const BATCH_TIME: Duration = Duration::from_secs(2);

/// The name of the scan threads.
const THREAD_NAME: &str = "clauseo-marks";

/// Every matter's scan state, in the actor's memory only.
#[derive(Default)]
pub(crate) struct PaperScans(HashMap<MatterId, MatterScan>);

/// One matter's scan state.
#[derive(Default)]
struct MatterScan {
    /// The folder the answers are for: a matter moved elsewhere starts again.
    folder: PathBuf,
    /// What its scans have read.
    kept: Kept,
    /// The papers waiting to be read, as the newest listing (or [`rescan`]) gave them.
    wanted: Vec<Paper>,
    /// A scan thread is running.
    running: bool,
}

/// What a scan thread posts when its batch ends.
pub(crate) struct Batch {
    matter: MatterId,
    folder: PathBuf,
    /// Each paper read, with what [`check`] answered (`None`: the read failed).
    read: Vec<(Paper, Option<bool>)>,
}

/// `papers` of `matter`'s `folder` as just listed: those known to hold tracked changes or
/// comments. Forgets answers for papers no longer listed, and scans the `.docx` papers not yet
/// read in the background.
pub(crate) fn listed(
    actor: &mut Actor,
    matter: MatterId,
    folder: &Path,
    papers: &[Paper],
) -> HashSet<MatterPath> {
    let scan = state(actor, matter, folder);
    scan.kept.keep_only(papers);
    // the newest listing is what waits: papers an older one wanted and this one lacks go
    scan.wanted.clear();
    let tracked = papers
        .iter()
        .filter(|paper| scan.kept.get(paper) == Some(true))
        .map(|paper| paper.path.clone())
        .collect();
    rescan(actor, matter, folder, papers);
    tracked
}

/// Reads `papers` of `matter`'s `folder` in the background (those with an answer kept for their
/// size and time are skipped), then pushes `PapersChanged { matter }` once they are read, so the
/// app lists again. For anything that knows papers changed; a listing does this by itself.
pub(crate) fn rescan(actor: &mut Actor, matter: MatterId, folder: &Path, papers: &[Paper]) {
    let scan = state(actor, matter, folder);
    let unread: Vec<Paper> = scan.kept.unread(papers).cloned().collect();
    let mut waiting: HashSet<MatterPath> = scan
        .wanted
        .iter()
        .map(|wanted| wanted.path.clone())
        .collect();
    for paper in unread {
        if waiting.insert(paper.path.clone()) {
            scan.wanted.push(paper);
        }
    }
    start(actor, matter);
}

/// `matter`'s scan state for `folder`, started afresh if it was for another folder.
fn state<'a>(actor: &'a mut Actor, matter: MatterId, folder: &Path) -> &'a mut MatterScan {
    let scan = actor.paper_scans.0.entry(matter).or_default();
    if scan.folder != folder {
        scan.folder = folder.to_path_buf();
        scan.kept = Kept::default();
        scan.wanted.clear();
    }
    scan
}

/// Starts a scan thread on `matter`'s next batch, unless one runs or nothing waits.
fn start(actor: &mut Actor, matter: MatterId) {
    let Some(scan) = actor.paper_scans.0.get_mut(&matter) else {
        return;
    };
    if scan.running || scan.wanted.is_empty() {
        return;
    }
    let papers: Vec<Paper> = scan.wanted.iter().take(BATCH_FILES).cloned().collect();
    let folder = scan.folder.clone();
    let clock = Arc::clone(&actor.clock);
    let post = actor.post.clone();
    let spawned = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let batch = read(matter, folder, papers, clock.as_ref());
            // The actor may be gone by then; nobody is left to tell.
            let _gone = post.try_send(Input::PaperMarks(batch));
        });
    match spawned {
        Ok(_detached) => scan.running = true,
        Err(error) => tracing::warn!(error = %Cause(&error), "no thread for a papers scan"),
    }
}

/// Reads `papers` of `folder`, stopping before a file once [`BATCH_TIME`] has passed.
fn read(matter: MatterId, folder: PathBuf, papers: Vec<Paper>, clock: &dyn Clock) -> Batch {
    let started = clock.now().mono;
    let mut read = Vec::with_capacity(papers.len());
    for paper in papers {
        if clock.now().mono.saturating_sub(started) >= BATCH_TIME {
            break;
        }
        let mut path = folder.clone();
        path.extend(paper.path.as_str().split('/'));
        let has = check(&path);
        read.push((paper, has));
    }
    Batch {
        matter,
        folder,
        read,
    }
}

/// A scan thread's batch ended: keeps its answers, tells the app if the newest listing gains
/// from them, and starts the next batch.
pub(crate) fn on_batch(actor: &mut Actor, batch: Batch) {
    let Batch {
        matter,
        folder,
        read,
    } = batch;
    let current = actor
        .check_matter(matter)
        .ok()
        .map(|entry| PathBuf::from(&entry.row.folder_path));
    let Some(scan) = actor.paper_scans.0.get_mut(&matter) else {
        return;
    };
    scan.running = false;
    if current.as_deref() != Some(folder.as_path()) || scan.folder != folder {
        // The matter is gone or moved: what was read is for a folder nobody lists.
        if current.is_none() {
            actor.paper_scans.0.remove(&matter);
            return;
        }
        if scan.folder == folder {
            scan.wanted.clear();
        }
        start(actor, matter);
        return;
    }
    // by path, each at most once: a listing lists a path once and [`rescan`] adds it once
    let wanted: HashMap<&MatterPath, &Paper> = scan
        .wanted
        .iter()
        .map(|wanted| (&wanted.path, wanted))
        .collect();
    let mut shown = false;
    for (paper, has) in &read {
        let waited = wanted.get(&paper.path) == Some(&paper);
        if let Some(has) = *has {
            scan.kept.put(paper, has);
            // a paper the listing showed untracked that holds marks: its tile changes
            shown |= waited && has;
        }
    }
    // Tried, read or not: a failed read waits for the next listing, never loops here. A paper
    // listed again at another size or time since stays, to be read as it is now.
    let tried: HashMap<&MatterPath, &Paper> =
        read.iter().map(|(paper, _)| (&paper.path, paper)).collect();
    scan.wanted
        .retain(|wanted| tried.get(&wanted.path) != Some(&wanted));
    if shown {
        super::super::app::publish(actor, &HostEvent::PapersChanged { matter });
    }
    start(actor, matter);
}

#[cfg(test)]
mod tests;
