//! The launch profile's per-session values, gathered on the actor: the folders, the `tools.*`
//! switches, the login, the proxy port and the browser inputs, handed to `agent::loads`, which
//! does the merging. Path joins and map work only: the folders were made on the launch-prep
//! thread (`launch_checks.rs`).

use std::path::Path;

use clauseo_acp::McpServer;
use clauseo_protocol::{AgentKind, ErrorCode, ThreadId};
use serde_json::{Map, Value};

use super::super::Actor;
use super::launch_checks::{Prepared, RunFolders};
use super::state;
use crate::agent::loads;
use crate::agent::loads::browser::{self, BrowserInputs, Switch};
use crate::agent::loads::profile::{self, ClaudeKeys, Folders, Login};
use crate::browser::seed_sites::SEED_SITES;
use crate::error::Error;

/// `tools.web_search` and `tools.browser`, with whether each is locked.
fn switches(actor: &Actor) -> (Switch, Switch) {
    let tools = &actor.effective.settings().tools;
    (
        Switch {
            on: tools.web_search,
            locked: actor.effective.is_locked("tools.web_search"),
        },
        Switch {
            on: tools.browser,
            locked: actor.effective.is_locked("tools.browser"),
        },
    )
}

/// The launch's folders kept on the thread for its session start, and a Codex agent's
/// `CODEX_CONFIG` keys from the profile, filled. An unfillable instructions token, or a folder
/// that is no UTF-8, fails the launch with `agent.launch_failed`.
pub(super) fn launch_keys(
    actor: &mut Actor,
    thread: ThreadId,
    mut prepared: Prepared,
) -> Result<Prepared, Error> {
    let agent_id = actor.check_thread(thread)?.row.agent.clone();
    if let Some(codex) = prepared.codex.as_mut() {
        let folders = Folders::of(&prepared.run.work, &prepared.run.notes)?;
        let (web_search, _) = switches(actor);
        codex.keys =
            profile::codex_keys(actor.profile.codex_config(&agent_id), web_search, folders)?;
    }
    if let Some(state) = state(actor, thread) {
        state.run = Some(prepared.run.clone());
    }
    Ok(prepared)
}

/// The `_meta` and MCP servers for `thread`'s `session/new`, `session/load` or
/// `session/resume`, whose matter folder is `matter`: the profile hook's `_meta` with the
/// host's switches and keys over it and its tokens filled; the servers the lawyer turned on, then
/// the hook's (none when the firm's `managed-mcp.json` is present). Shows the session's tool
/// state in the thread's header.
pub(super) fn session_loads(
    actor: &mut Actor,
    thread: ThreadId,
    matter: &Path,
) -> Result<(Map<String, Value>, Vec<McpServer>), Error> {
    let agent_id = actor.check_thread(thread)?.row.agent.clone();
    let kind = AgentKind::of(&agent_id);
    let run: RunFolders = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.run.clone())
        .ok_or_else(|| {
            Error::refused(ErrorCode::AgentLaunchFailed, "the session has no folders")
        })?;
    let folders = Folders::of(&run.work, &run.notes)?;
    let control = loads::mcp_control(&actor.managed_mcp);
    let (web_search, browser_switch) = switches(actor);
    let settings_env = actor
        .effective
        .agent(agent_id.as_str())
        .map(|launch| launch.env.clone())
        .unwrap_or_default();
    let login = Login::of(
        &actor.profile.launch_env(&agent_id),
        &settings_env,
        |name| std::env::var_os(name),
    );
    let claude = (kind == Some(AgentKind::Claude)).then_some(ClaudeKeys {
        web_search,
        login,
        folders,
        seed_sites: &SEED_SITES,
    });
    let guard = actor
        .pre_write_guard
        .as_deref()
        .map(|program| loads::GuardHook { program, matter });
    let mut meta = loads::session_meta(
        actor.profile.session_meta(&agent_id),
        control,
        guard,
        claude.as_ref(),
    );
    profile::fill_meta(&mut meta, folders)?;
    let session_id = run.key.id();
    let inputs = BrowserInputs {
        switch: browser_switch,
        control,
        slots: actor.wiring.browser_slots,
        port: actor
            .wiring
            .proxy
            .as_ref()
            .and_then(|proxy| proxy.port_for(run.key)),
        executable: actor.wiring.browser_executable.as_deref(),
        browser_dir: &run.browser,
        session_id: &session_id,
    };
    let mut servers = loads::mcp_servers(&actor.effective, control);
    let (hook_servers, browser_state) =
        browser::hook_servers(actor.profile.mcp_servers(&agent_id), &inputs);
    servers.extend(hook_servers);
    let tools = profile::web_search_state(kind, web_search, login)
        .into_iter()
        .chain(browser_state)
        .collect();
    super::super::header_tools::show(actor, thread, tools);
    Ok((meta, servers))
}
