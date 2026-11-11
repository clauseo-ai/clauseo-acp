//! The scans behind Settings › Tools' import counts (`McpToolsView::importable`): what the
//! actor keeps of one ([`Found`]: names, sources and a "blocked" flag, never a value), when one
//! runs (the start, each app catch-up, after a removal; one at a time, asks while it runs fold
//! into one more), and the skip when nothing it would read changed. Split from `import.rs`.

use std::path::PathBuf;

use clauseo_protocol::{McpImportSource, McpImportable};
use clauseo_settings::mcp_import::ImportCandidate;

use super::super::Actor;
use super::super::input::Input;
use super::import::{skip_of, source_of};
use super::{McpInput, finder, view};
use crate::config::mcp::{McpFileStamp, McpImportFinder};

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;

/// The name of a scan's thread; the finder itself runs on a worker of [`super::finder`].
const SCAN_THREAD: &str = "clauseo-mcp-import";

/// One server a scan found, as the actor keeps it: its name in Clauseo, the apps that set it
/// up, and whether a problem blocks it. Never a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    name: String,
    sources: Vec<McpImportSource>,
    blocked: bool,
}

impl Found {
    fn of(candidate: &ImportCandidate) -> Self {
        let mut sources: Vec<McpImportSource> = candidate.sources.iter().map(source_of).collect();
        sources.sort_unstable();
        sources.dedup();
        Self {
            name: candidate.name.clone(),
            sources,
            blocked: skip_of(&candidate.problems).is_some(),
        }
    }
}

/// Each app with servers that would import now, with how many: the last scan's unblocked
/// servers whose name the list does not hold in any case. Empty before the first scan.
pub(crate) fn importable(actor: &Actor) -> Vec<McpImportable> {
    let Some(found) = &actor.mcp.found else {
        return Vec::new();
    };
    let held = actor.effective.mcp_servers();
    let free = |name: &str| !held.keys().any(|have| have.eq_ignore_ascii_case(name));
    McpImportSource::ALL
        .into_iter()
        .filter_map(|source| {
            let count = found
                .iter()
                .filter(|one| !one.blocked && one.sources.contains(&source) && free(&one.name))
                .count();
            let count = u32::try_from(count).unwrap_or(u32::MAX);
            (count > 0).then_some(McpImportable { source, count })
        })
        .collect()
}

/// Starts a scan on a thread of its own, unless one runs (then one more follows it), the
/// finder is not wired, or the firm decides the list. Its answer is [`McpInput::Scanned`].
pub(crate) fn scan(actor: &mut Actor) {
    let Some(find) = actor.wiring.mcp_find.clone() else {
        return;
    };
    if actor.mcp.scanning {
        actor.mcp.rescan = true;
        return;
    }
    if actor.effective.mcp_locked() || view::managed(actor) {
        return;
    }
    let existing: Vec<String> = actor.effective.mcp_servers().keys().cloned().collect();
    let post = actor.post.clone();
    let busy = std::sync::Arc::clone(&actor.mcp.scan_busy);
    let last = actor.mcp.last_scan.clone();
    let started = std::thread::Builder::new()
        .name(SCAN_THREAD.to_owned())
        .spawn(move || {
            let mut unanswered = ScanUnanswered(Some(post.clone()));
            let job = move || scan_job(find.as_ref(), existing, last);
            let found = finder::bounded(job, &busy, finder::TIMEOUT);
            unanswered.0 = None;
            let _gone = post.try_send(Input::Mcp(Box::new(McpInput::Scanned(found))));
        });
    match started {
        Ok(_) => actor.mcp.scanning = true,
        Err(error) => tracing::warn!(%error, "the MCP import scan did not start"),
    }
}

/// The list and the files the last fresh scan saw, each with its stamp then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LastScan {
    existing: Vec<String>,
    files: Vec<(PathBuf, McpFileStamp)>,
}

/// What a scan answered.
pub(crate) enum ScanAnswer {
    /// The list is as it was and every file the last scan saw has its stamp then: nothing read.
    Unchanged,
    /// The servers found, and what the scan saw.
    Fresh { found: Vec<Found>, last: LastScan },
}

/// One scan on the finder's worker: skipped when nothing it would read changed since `last`
/// (the finder knows its files, the list is the same, and each file's stamp matches), else a
/// fresh run of the finder.
fn scan_job(
    find: &dyn McpImportFinder,
    existing: Vec<String>,
    last: Option<LastScan>,
) -> ScanAnswer {
    if let Some(last) = last
        && !last.files.is_empty()
        && last.existing == existing
    {
        let (paths, stamps): (Vec<PathBuf>, Vec<McpFileStamp>) = last.files.into_iter().unzip();
        if find.stamps(&paths) == stamps {
            return ScanAnswer::Unchanged;
        }
    }
    let found = find.find(&existing);
    ScanAnswer::Fresh {
        found: found.candidates.iter().map(Found::of).collect(),
        last: LastScan {
            existing,
            files: found.files,
        },
    }
}

/// Posts an empty [`McpInput::Scanned`] when dropped while armed: the scan panicked.
struct ScanUnanswered(Option<async_channel::Sender<Input>>);

impl Drop for ScanUnanswered {
    fn drop(&mut self) {
        if let Some(post) = self.0.take() {
            let _gone = post.try_send(Input::Mcp(Box::new(McpInput::Scanned(None))));
        }
    }
}

/// A scan ended: its servers kept (unless it failed), the view sent when it changed, and the
/// scan asked for meanwhile started.
pub(super) fn on_scanned(actor: &mut Actor, answer: Option<ScanAnswer>) {
    actor.mcp.scanning = false;
    if let Some(ScanAnswer::Fresh { found, last }) = answer {
        actor.mcp.found = Some(found);
        actor.mcp.last_scan = Some(last);
        view::publish(actor);
    }
    if std::mem::take(&mut actor.mcp.rescan) {
        scan(actor);
    }
}
