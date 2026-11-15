//! A Claude session's own sandbox (`agent::loads::sandbox`), gathered on the actor: the
//! session's folders, the data folder's spellings and the other matters' folders the launch
//! fenced (`agent::matter_fence`). Split from `profile_loads.rs` for its length.

use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentKind, ThreadId};

use super::super::super::Actor;
use super::super::launch_checks::RunFolders;
use crate::agent::loads::sandbox::{ClaudeSandbox, Off, SandboxOs, SandboxPaths};

/// A Claude session's sandbox ([`claude_sandbox`]); none for another agent. Where one was meant
/// to be and is not, holds `auto` back for `thread`.
pub(super) fn session_sandbox(
    actor: &mut Actor,
    thread: ThreadId,
    kind: Option<AgentKind>,
    agent_id: &clauseo_protocol::AgentId,
    run: &RunFolders,
    matter: &Path,
) -> Option<ClaudeSandbox> {
    let sandbox =
        (kind == Some(AgentKind::Claude)).then(|| claude_sandbox(actor, agent_id, run, matter));
    if let Some(Err(off)) = sandbox
        && off.holds_auto_back()
    {
        // No sandbox where one was meant to be: no command may run on the classifier alone.
        super::super::session::hold_auto_back(actor, thread);
    }
    sandbox.and_then(Result::ok)
}

/// Claude Code's own sandbox for a Claude session of `agent_id` in `run`, whose matter folder
/// is `matter` (`agent::loads::sandbox`), or why there is none (on Windows, among others).
/// Path joins only.
fn claude_sandbox(
    actor: &Actor,
    agent_id: &clauseo_protocol::AgentId,
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
    // Every other matter's folder the launch knew (forgotten ones too), but those the lawyer
    // let this conversation read (`agent::matter_fence`); one registered later is the residual.
    let fence = &run.data_deny.matters;
    let denies = sandbox_denies(run);
    let (inside, other_matters) = (denies.inside, denies.other_matters);
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
        matter_shut: fence.own_shut,
        matter_inside: &inside,
    };
    ClaudeSandbox::for_os(SandboxOs::CURRENT, &paths)
}

/// The other matters' folders Claude's sandbox denies, each as `deny_form` writes it, and
/// whether one had to be widened to a folder above it (the session's commands are limited).
pub(super) struct SandboxDenies {
    /// The other matters' folders inside the matter: denied writes.
    pub(super) inside: Vec<PathBuf>,
    /// Every fenced other matter's folder: denied reads.
    pub(super) other_matters: Vec<PathBuf>,
    /// Whether a deny was widened to a folder above what it names.
    pub(super) limited: bool,
}

/// [`SandboxDenies`] of `run`'s fence: each folder as registered, and where it leads
/// (`agent::loads::sandbox::deny_form`). Paths only.
pub(super) fn sandbox_denies(run: &RunFolders) -> SandboxDenies {
    let fence = &run.data_deny.matters;
    let stored: Vec<&Path> = fence
        .others
        .iter()
        .map(|other| other.folder.as_path())
        .collect();
    let mut limited = false;
    let mut form = |path: &Path| {
        let (path, widened) = crate::agent::loads::sandbox::deny_form(path, &stored);
        limited |= widened;
        path
    };
    let inside = fence
        .inside_own
        .iter()
        .map(|(path, _)| form(path))
        .collect();
    let other_matters = fence.fenced_folders().map(&mut form).collect();
    SandboxDenies {
        inside,
        other_matters,
        limited,
    }
}
