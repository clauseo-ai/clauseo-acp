//! The launch profile's per-session values, gathered on the actor: the folders, the `tools.*`
//! switches, the login, the proxy port and the browser inputs, handed to `agent::loads`, which
//! does the merging. Path joins and map work only: the folders were made on the launch-prep
//! thread (`launch_checks.rs`).

use std::path::Path;

use clauseo_acp::McpServer;
use clauseo_protocol::{AgentKind, ErrorCode, ThreadId, ToolAvailability};
use serde_json::{Map, Value};

use super::super::Actor;
use super::launch_checks::{Prepared, RunFolders};
use crate::agent::loads;
use crate::agent::loads::browser::{self, BrowserInputs, Claims, Switch};
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

/// A Codex agent's `CODEX_CONFIG` keys from the profile, filled into `prepared`. The hook gets
/// this launch's own skills token, taken out of `prepared` so nothing keeps it. An unfillable
/// instructions token, or a folder that is no UTF-8, fails the launch with
/// `agent.launch_failed`.
pub(super) fn launch_keys(
    actor: &mut Actor,
    thread: ThreadId,
    prepared: &mut Prepared,
) -> Result<(), Error> {
    let agent_id = actor.check_thread(thread)?.row.agent.clone();
    let written = prepared.skills_written.take();
    if let Some(codex) = prepared.codex.as_mut() {
        let folders = Folders::of(&prepared.run.work, &prepared.run.notes)?;
        let (web_search, _) = switches(actor);
        let keys = actor.profile.codex_config(&agent_id, written.as_ref());
        codex.keys = profile::codex_keys(keys, web_search, folders)?;
    }
    Ok(())
}

/// The `_meta` and MCP servers for `thread`'s `session/new`, `session/load` or
/// `session/resume`, whose matter folder is `matter`: the profile hook's `_meta` with the
/// host's switches and keys over it and its tokens filled; the servers the lawyer turned on, then
/// the hook's (none when the firm's `managed-mcp.json` is present). Holds the session's tool
/// state for the thread's header until the session call answers ([`show_tools`]).
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
    // This launch's own folder answer decided it (`agent/forced_ask.rs`).
    let forced_ask = super::state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .is_some_and(|agent| agent.forced_ask);
    let claude = (kind == Some(AgentKind::Claude)).then_some(ClaudeKeys {
        web_search,
        login,
        folders,
        seed_sites: SEED_SITES,
        forced_ask,
    });
    let guard = actor
        .pre_write_guard
        .as_deref()
        .map(|program| loads::GuardHook {
            program,
            matter,
            ask_all: forced_ask,
        });
    // Without the guard's hook, a forced-ask session would rest on the ask rules alone, and a
    // hook that is missing fails open: refuse it.
    if let (true, Some(_), Some(reason)) = (forced_ask, &claude, loads::guard_missing(guard)) {
        let matter_id = actor.check_thread(thread)?.row.matter;
        tracing::warn!(
            matter = %matter_id,
            reason,
            "a forced-ask Claude launch has no guard hook to give: the launch is refused"
        );
        return Err(Error::refused(
            ErrorCode::AgentLaunchFailed,
            "this matter's folder is unprotected, and the guard that makes Claude ask before \
             each change could not be set up",
        ));
    }
    let mut meta = loads::session_meta(
        actor.profile.session_meta(&agent_id),
        control,
        guard,
        claude.as_ref(),
    );
    profile::fill_meta(&mut meta, folders)?;
    let inputs = BrowserInputs {
        switch: browser_switch,
        control,
        slot_free: actor.wiring.browser_slots.free_for(run.key),
        port: actor
            .wiring
            .proxy
            .as_ref()
            .and_then(|proxy| proxy.port_for(run.key)),
        executable: run.chrome.as_deref(),
        browser_dir: &run.browser,
        claims: run.browser_session.as_deref().map(Claims::of),
    };
    let mut servers = loads::mcp_servers(&actor.effective, control);
    let (hook_servers, browser_state) =
        browser::hook_servers(actor.profile.mcp_servers(&agent_id), &inputs);
    servers.extend(hook_servers);
    if browser_state
        .as_ref()
        .is_some_and(|state| state.availability == ToolAvailability::Available)
    {
        // The session holds its slot until it ends (`launch_checks::discard`).
        actor.wiring.browser_slots.take(run.key);
    }
    let tools = profile::web_search_state(kind, web_search, login)
        .into_iter()
        .chain(browser_state)
        .collect();
    super::super::header_tools::hold(actor, thread, tools);
    Ok((meta, servers))
}

/// The session call answered: the tool state [`session_loads`] held shows in the header.
pub(super) fn show_tools(actor: &mut Actor, thread: ThreadId) {
    super::super::header_tools::show_held(actor, thread);
}
