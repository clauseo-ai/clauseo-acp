//! A cancelled install ([`super::Agents::cancel_install`]): the lawyer's Cancel while an agent
//! downloads or sets up. The plan goes back to not installed, and what its install wrote goes.
//!
//! - **At once**, under the shared lock: the agent's running job is stopped as a newer job
//!   would stop it (its ticket cancelled, no `Paused`, every later emit of it dropped), and
//!   `Missing` is sent with the agent's last `Missing` size (else the `Downloading` total, else
//!   the snapshot's size with Node's part when no runtime is unpacked).
//! - **A clean-up job** takes the agent's slot ([`super::Slot::cleanup`]) on a counted job
//!   thread. It waits for the agent's turn lock, so the stopped job's thread has ended first:
//!   the downloader checks its cancel between chunks and a stalled read ends within its 60 s
//!   read timeout; npm's handle is dropped as the job sees the cancel, which kills its tree.
//!   Then, with `agents/.lock` shared and the agent's `.lock-<id>` exclusive (either held by
//!   another process skips the removal: that Clauseo may be installing it now), it removes
//!   [`remove_partial`]'s three places and ends its slot with no event.
//! - **The resume marker** (`install::restart`) goes at once and again in the clean-up, as a
//!   pause removes it, so the next start does not resume an install the lawyer cancelled.
//! - **A newer `install` or `retry`** of the agent supersedes the clean-up (it is no install
//!   running, so the install starts), and `shutdown` cancels it like any job. Its disk reserve
//!   is zero.

use std::sync::{Arc, Mutex, PoisonError};

use clauseo_protocol::AgentId;

use super::{Inner, Shared, Ticket};
use crate::cancel::Cancel;
use crate::config::AgentsConfig;
use crate::install;
use crate::receipt;
use crate::state::InstallState;
use crate::write::{self, AgentsLock, Mode};

/// Cancels `agent`'s install, if it is downloading, installing, waiting, paused or failed, and
/// starts its clean-up job, which waits on `turn`, the agent's turn lock.
pub(super) fn cancel(
    config: &AgentsConfig,
    shared: &Arc<Shared>,
    agent: &AgentId,
    turn: Arc<Mutex<()>>,
) {
    let ticket = {
        let mut inner = shared.lock();
        let cancellable = matches!(
            inner.states.get(agent),
            Some(
                InstallState::Downloading { .. }
                    | InstallState::Installing
                    | InstallState::Waiting { .. }
                    | InstallState::Paused
                    | InstallState::Failed(_)
            )
        );
        if inner.closed || !cancellable {
            return;
        }
        let download_bytes = missing_bytes(config, &inner, agent);
        inner.stop_job(&shared.events, agent, true);
        let missing = InstallState::Missing { download_bytes };
        Inner::set(&mut inner, &shared.events, agent, missing, None);
        inner.next_job += 1;
        let ticket = Ticket {
            job: inner.next_job,
            agent: agent.clone(),
            cancel: Arc::new(Cancel::new()),
        };
        inner.add_job(ticket.clone(), true);
        ticket
    };
    tracing::info!(
        target: "clauseo_agents::lifecycle",
        %agent,
        "agent install: cancelled"
    );
    let job = ticket.clone();
    let config = config.clone();
    let on_thread = Arc::clone(shared);
    let spawned = shared.spawn_job(format!("clauseo-cleanup-{agent}"), move || {
        clean_up(&config, &on_thread, &job, &turn);
    });
    if let Err(err) = spawned {
        // The files stay; the next start's clean-up or the next install's try removes them.
        tracing::warn!(%agent, %err, "agent install: the clean-up did not start");
        shared.lock().end_slot(&ticket);
    }
}

/// The size the cancelled agent's `Missing` shows: its last `Missing`'s, else the total of the
/// download it was in, else the snapshot's (Node's part counted when no runtime is unpacked).
fn missing_bytes(config: &AgentsConfig, inner: &Inner, agent: &AgentId) -> u64 {
    if let Some(bytes) = inner.missing_bytes.get(agent) {
        return *bytes;
    }
    if let Some(InstallState::Downloading { total, .. }) = inner.states.get(agent) {
        return *total;
    }
    let node_needed = !install::runtime_ready(config);
    config
        .snapshot
        .download_bytes(agent, config.platform, node_needed)
        .unwrap_or(0)
}

/// The clean-up job's thread: once the agent's turn is its own, and unless a newer job
/// superseded it meanwhile, removes the agent's partial files and ends its slot quietly.
fn clean_up(config: &AgentsConfig, shared: &Shared, ticket: &Ticket, turn: &Mutex<()>) {
    let _turn = turn.lock().unwrap_or_else(PoisonError::into_inner);
    if ticket.cancel.is_cancelled() {
        return;
    }
    // Again once the stopped job's thread has ended, in case it wrote the marker after the cancel.
    install::restart::clear_wanted(&config.data_dir, &ticket.agent);
    remove_partial(config, &ticket.agent);
    shared.lock().end_slot(ticket);
}

/// Removes what `agent`'s install wrote and no other plan uses, under `agents/.lock` (shared)
/// and the agent's `.lock-<id>` (exclusive), skipped when another process holds either or the
/// data folder fails the install's path check:
/// - `agents/downloads/<id>/`, its package tarballs and their `.part` files;
/// - `agents/npm-cache/<id>/`;
/// - every entry of `agents/<id>/install/` but the current `<hash>` folder with a valid receipt
///   (temp trees, kept set-ups and stale folders alike).
///
/// Nothing else: Node's and Python's archives in `agents/downloads/` are shared. Each place is
/// removed only when every folder down to it is a real one (`write::real_dirs_under`), so no
/// removal reaches through a link. When the clean-up is skipped, the next start empties the npm
/// cache and the stale install folders, but keeps `downloads/<id>/`: a start cannot tell a
/// cancelled try's files from a quit try's, whose `.part` files the next try resumes. Those
/// files stay until that agent's next `Ready` removes them, or a later cancel that gets its
/// locks. A skip means another Clauseo holds the locks and may be using those very files.
fn remove_partial(config: &AgentsConfig, agent: &AgentId) {
    if install::check_paths(config).is_err() {
        return;
    }
    let data = &config.data_dir;
    let take = |lock, mode| write::try_lock(data, lock, mode).ok().flatten();
    let Some(all) = take(AgentsLock::All, Mode::Shared) else {
        tracing::info!(%agent, "agent install: clean-up skipped, the agents folder is busy");
        return;
    };
    let Some(own) = take(AgentsLock::Agent(agent), Mode::Exclusive) else {
        tracing::info!(%agent, "agent install: clean-up skipped, another Clauseo installs it");
        return;
    };
    let downloads = install::agent_downloads(data, agent);
    if write::real_dirs_under(data, &downloads).unwrap_or(false) {
        drop(write::remove_dir_all(&downloads));
    }
    install::empty_npm_cache(data, agent);
    if let Some(hash) = config.snapshot.install_hash(agent, config.platform) {
        remove_install_trees(config, agent, &hash);
    }
    drop(own);
    drop(all);
}

/// Removes every entry of `agent`'s `install/` but the `hash` folder with a valid receipt.
fn remove_install_trees(config: &AgentsConfig, agent: &AgentId, hash: &str) {
    let data = &config.data_dir;
    // `scan` checks the folders down to `install/` are real, and finds the valid folder.
    let Ok(scan) = receipt::scan(data, agent, hash, config.platform) else {
        return;
    };
    let keep = scan.keep.map(|(path, _receipt)| path);
    let root = receipt::install_root(data, agent);
    if !write::real_dirs_under(data, &root).unwrap_or(false) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if keep.as_ref() != Some(&path) {
            drop(write::remove_any(&path));
        }
    }
    write::remove_empty_dir(&root);
}
