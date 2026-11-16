//! Clauseo's own MCP entries beside `case-law` in one session call (`profile_loads.rs`): the
//! Windows variables both connector entries carry, the `clauseo-versions` entry with the undo
//! note's flag, and the note of what Clauseo sent, which the Codex auto-allow reads.

use std::ffi::OsString;

use clauseo_acp::McpServer;
use clauseo_protocol::ThreadId;

use super::super::Actor;
use super::launch_checks::RunFolders;
use crate::agent::loads::versions::{self, VersionsInputs};
use crate::agent::loads::{self, case_law};
use crate::versions::VersionsTool;

/// The variables a connector entry (`case-law`, `clauseo-versions`) carries on Windows
/// (`case_law::windows_env`), from the environment the host inherited; none elsewhere.
pub(super) fn windows_env(actor: &Actor) -> Vec<(String, String)> {
    let inherited: Vec<(OsString, OsString)> = match &actor.wiring.inherited_env {
        Some(env) => env.clone(),
        None if cfg!(windows) => std::env::vars_os().collect(),
        None => Vec::new(),
    };
    let lookup = |name: &str| {
        inherited
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    };
    case_law::windows_env(cfg!(windows), lookup, case_law::windows_directory)
}

/// The hook's servers without `clauseo-versions`, and the entry `run`'s session gets (none
/// without a listener or under the firm's `managed-mcp.json`). A listener whose entry does not
/// go is ended at once. The undo note of `thread` points at the tool from now on only when the
/// entry goes: this runs at every session call, so a later launch without it sets it back.
pub(super) fn versions_servers(
    actor: &mut Actor,
    thread: ThreadId,
    run: &RunFolders,
    hook: Vec<McpServer>,
    control: loads::McpControl,
) -> (Vec<McpServer>, Option<McpServer>) {
    let windows_env = windows_env(actor);
    let listener = run.versions.listener();
    let inputs = VersionsInputs {
        listener: listener.map(|listener| (listener.port(), run.versions_token.as_path())),
        control,
        windows_env: &windows_env,
    };
    let (rest, entry) = versions::hook_servers(hook, &inputs);
    if entry.is_none()
        && let Some(listener) = listener
    {
        listener.end();
    }
    if let Some(state) = super::state(actor, thread) {
        state.note.versions = tool(entry.as_ref());
    }
    (rest, entry)
}

/// Whether the session has the tool: whether its `entry` goes.
pub(super) fn tool(entry: Option<&McpServer>) -> VersionsTool {
    if entry.is_some() {
        VersionsTool::Offered
    } else {
        VersionsTool::Absent
    }
}

/// Notes on `thread`'s agent what may run unasked in its session (`agent/mcp_trust.rs`): only
/// what Clauseo sent, the lawyer's `trusted` servers and Clauseo's own entries that went
/// (`own`: `case-law`'s and `clauseo-versions`', each when it goes).
pub(super) fn note_mcp_sent(
    actor: &mut Actor,
    thread: ThreadId,
    trusted: &[String],
    own: [&Option<McpServer>; 2],
) {
    let mut sent = trusted.to_vec();
    sent.extend(
        own.into_iter()
            .flatten()
            .map(|entry| entry.name().to_owned()),
    );
    if let Some(agent) = super::state(actor, thread).and_then(|state| state.agent.as_deref_mut())
        && let Some(slot) = agent.mcp_sent.as_mut()
    {
        *slot = sent;
    }
}
