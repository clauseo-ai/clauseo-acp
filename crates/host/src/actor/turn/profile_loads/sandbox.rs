//! A Claude session's sandbox for its session call (`agent::loads::sandbox`), gathered on the
//! actor: on macOS and Linux the sandbox or the launch's refusal, never neither. Split from
//! `profile_loads.rs` for its size.

use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentId, AgentKind, ThreadId};

use super::super::super::Actor;
use super::super::launch_checks::RunFolders;
use crate::agent::loads::sandbox::{ClaudeSandbox, SandboxOs, SandboxPaths};
use crate::error::Error;
use crate::log_file::Cause;

/// A Claude session's sandbox ([`claude_sandbox`]); none for another agent, nor on Windows.
/// Where one was meant to be and can't be built, the launch is refused
/// (`ClaudeSandbox::for_launch`), with one warn line naming the matter.
pub(super) fn session_sandbox(
    actor: &Actor,
    thread: ThreadId,
    kind: Option<AgentKind>,
    agent_id: &AgentId,
    run: &RunFolders,
    matter: &Path,
) -> Result<Option<ClaudeSandbox>, Error> {
    if kind != Some(AgentKind::Claude) {
        return Ok(None);
    }
    claude_sandbox(actor, agent_id, run, matter).inspect_err(|error| {
        let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
        tracing::warn!(
            %thread,
            matter = ?matter,
            error = %Cause(error),
            "a Claude launch has no sandbox on macOS or Linux: the launch is refused"
        );
    })
}

/// Claude Code's own sandbox for a Claude session of `agent_id` in `run`, whose matter folder
/// is `matter` (`agent::loads::sandbox`): none on Windows, else it or the launch's refusal.
/// Path joins only.
fn claude_sandbox(
    actor: &Actor,
    agent_id: &AgentId,
    run: &RunFolders,
    matter: &Path,
) -> Result<Option<ClaudeSandbox>, Error> {
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
        other_matters: &run.other_matters,
    };
    ClaudeSandbox::for_launch(SandboxOs::CURRENT, &paths)
}
