//! A Claude session's sandbox (`agent::loads::sandbox`) and what it decides for the browser.
//!
//! Claude's browser is the guarded `browser` MCP entry (`clauseo_profile::servers::browser_mcp`).
//! Claude Code starts the entry's guard outside its sandbox, so what keeps Claude's shell off the
//! guard's Chrome and daemon is the sandbox: loopback denied, `<data>` and the daemon's socket
//! folders unreadable. A session whose sandbox falls back off on macOS or Linux (a `*` or `?` in
//! a path, a path to deny that is not UTF-8, no short temp folder: `Off::drops_browser`) has a
//! shell that could reach both, so it gets no entry, no browser in its header and the no-browser
//! sentence (decision record 0030, decision 4 of the browser plan). The same decision holds
//! `auto` back (`Off::holds_auto_back`): the session runs in `acceptEdits` and Claude asks before
//! running commands. One plain notice says both, once per thread for the host's run
//! (`NoticeKind::SandboxOff`). On Windows, where no sandbox was meant to be, Claude keeps the
//! entry (`Off::drops_browser`) and gets no notice.

use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentId, AgentKind, Notice, NoticeKind, NoticeLevel, ThreadId};

use super::super::super::Actor;
use super::super::launch_checks::RunFolders;
use super::super::state;
use crate::agent::loads::sandbox::{ClaudeSandbox, Off, SandboxOs, SandboxPaths};

/// The host English of the notice (`NoticeKind::SandboxOff`), for a view that does not know the
/// kind.
pub(crate) const SANDBOX_OFF: &str = "Clauseo couldn't start Claude's safeguards for this \
     matter, so Claude asks before running commands and has no browser in this conversation.";

/// A thread's notice that Claude's sandbox did not start: pending from the session call that
/// found it off until its session attaches, and told once for the host's run. It lives with the
/// thread's turn state, so a relaunch keeps it.
#[derive(Debug, Default)]
pub(crate) struct SandboxOffNotice {
    pending: bool,
    told: bool,
}

/// A Claude session's sandbox (none for another agent, or when it fell back off), and whether the
/// session gets no browser: none on this OS for the agent
/// ([`crate::agent::loads::browser::no_browser`]), or for want of the sandbox
/// ([`drops_browser`]). Where one was meant to be and is not, holds `auto` back for `thread` and,
/// from that same decision, holds the notice for its attach ([`notice`]); any other launch clears
/// one left pending.
pub(super) fn session_sandbox(
    actor: &mut Actor,
    thread: ThreadId,
    kind: Option<AgentKind>,
    agent_id: &AgentId,
    (run, matter): (&RunFolders, &Path),
    windows: bool,
) -> (Option<ClaudeSandbox>, bool) {
    let sandbox =
        (kind == Some(AgentKind::Claude)).then(|| claude_sandbox(actor, agent_id, run, matter));
    let off = matches!(sandbox, Some(Err(off)) if off.holds_auto_back());
    if off {
        // No sandbox where one was meant to be: no command may run on the classifier alone.
        super::super::session::hold_auto_back(actor, thread);
        tracing::info!(%thread, "Claude's sandbox is off: acceptEdits and no browser");
    }
    if let Some(state) = state(actor, thread) {
        state.sandbox_off.pending = off;
    }
    let dropped = drops_browser(sandbox.as_ref());
    let none_here = crate::agent::loads::browser::no_browser(kind, windows) || dropped;
    (sandbox.and_then(Result::ok), none_here)
}

/// Whether `sandbox` ([`session_sandbox`]) keeps the browser out of the session: a Claude
/// sandbox that fell back off where one was meant to be (the same reasons as `holds_auto_back`).
fn drops_browser(sandbox: Option<&Result<ClaudeSandbox, Off>>) -> bool {
    matches!(sandbox, Some(Err(off)) if off.drops_browser())
}

/// The notice for `thread`'s session that just attached, when its session call found Claude's
/// sandbox off and the thread was not told yet in the host's run.
pub(crate) fn notice(actor: &mut Actor, thread: ThreadId) -> Option<Notice> {
    let state = state(actor, thread)?;
    let off = &mut state.sandbox_off;
    if !std::mem::take(&mut off.pending) || std::mem::replace(&mut off.told, true) {
        return None;
    }
    Some(Notice {
        level: NoticeLevel::Info,
        text: SANDBOX_OFF.to_owned(),
        kind: Some(NoticeKind::SandboxOff),
    })
}

/// Claude Code's own sandbox for a Claude session of `agent_id` in `run`, whose matter folder
/// is `matter` (`agent::loads::sandbox`), or why there is none: on every OS for now, the
/// not-wanted `Off::Windows` ([`SandboxOs::LAUNCH`]). Path joins only.
fn claude_sandbox(
    actor: &Actor,
    agent_id: &AgentId,
    run: &RunFolders,
    matter: &Path,
) -> Result<ClaudeSandbox, Off> {
    let tmp = crate::agent::temp::temp_dir(&run.root);
    let skills: Vec<PathBuf> = run
        .data_deny
        .spellings
        .iter()
        .map(|data| {
            crate::agent::data_fence::BUILT_IN_SKILLS
                .iter()
                .fold(data.clone(), |path, name| path.join(name))
        })
        .collect();
    let claude_home = actor.effective.locations().agent_home(agent_id.as_str());
    // Every other matter's folder the host knows now; one registered later is the residual.
    let other_matters: Vec<PathBuf> = actor
        .matters
        .iter()
        .filter(|(id, _)| **id != run.key.matter)
        .map(|(_, entry)| PathBuf::from(&entry.row.folder_path))
        .collect();
    let paths = SandboxPaths {
        matter,
        work: &run.work,
        tmp: &tmp,
        claude_tmp: run.claude_tmp.as_deref(),
        downloads: &run.downloads,
        skill_packs: &run.skill_packs,
        notes: &run.notes,
        skills: &skills,
        path: &run.path.dirs,
        env_file: run.path.env_file.as_deref(),
        ca_file: run.path.ca_file.as_deref(),
        claude_home: &claude_home,
        data: &run.data_deny.spellings,
        other_matters: &other_matters,
    };
    ClaudeSandbox::for_os(SandboxOs::LAUNCH, &paths)
}
