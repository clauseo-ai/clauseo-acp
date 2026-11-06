//! The launch profile's per-session values, gathered on the actor: the folders, the `tools.*`
//! switches, the login, the proxy port and the browser inputs, handed to `agent::loads`, which
//! does the merging. Path joins and map work only: the folders were made on the launch-prep
//! thread (`launch_checks.rs`).

use std::path::{Path, PathBuf};

use clauseo_acp::McpServer;
use clauseo_protocol::{AgentKind, ErrorCode, ThreadId, ToolAvailability};
use serde_json::{Map, Value};

use super::super::Actor;
use super::launch_checks::relay::CaseLaw;
use super::launch_checks::{Prepared, RunFolders};
use crate::agent::loads;
use crate::agent::loads::browser::{self, BrowserInputs, Claims, Switch};
use crate::agent::loads::case_law::{self, CaseLawInputs, Relay};
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
/// this launch's own skills token, taken out of `prepared` so nothing keeps it, and its own
/// skill packs token (decision record 0040). An unfillable
/// instructions token, or a folder that is no UTF-8, fails the launch with
/// `agent.launch_failed`.
pub(super) fn launch_keys(
    actor: &mut Actor,
    thread: ThreadId,
    prepared: &mut Prepared,
) -> Result<(), Error> {
    let agent_id = actor.check_thread(thread)?.row.agent.clone();
    let written = prepared.skills_written.take();
    recheck_managed(actor, &mut prepared.run);
    if let Some(codex) = prepared.codex.as_mut() {
        // Codex's instructions are fixed here, right before the launch: the paragraph goes in
        // when the relay is open now. `session_loads` reads `managed-mcp.json` once more after
        // the agent answers `initialize`; a file that appears in that window drops the entry
        // (`ManagedByFirm`, the relay ended) while this paragraph stays: wrong words, but no
        // case law reaches the agent.
        let part = connectors_part(prepared.run.case_law.relay().is_some());
        let folders = Folders::of(&prepared.run.work, &prepared.run.notes)?.with_connectors(part);
        let (web_search, _) = switches(actor);
        let packs = prepared.run.packs_written.as_deref();
        let keys = actor
            .profile
            .codex_config(&agent_id, written.as_ref(), packs);
        codex.keys = profile::codex_keys(keys, web_search, folders)?;
        // The data-folder fence (`agent::codex_fence`), in every Codex session.
        let session = crate::agent::codex_fence::Session {
            run_root: &prepared.run.root,
            notes: &prepared.run.notes,
            state: &codex.state,
        };
        let fence =
            crate::agent::codex_fence::keys(actor.effective.locations(), session, cfg!(windows));
        codex.keys.extend(fence);
    }
    Ok(())
}

/// What [`session_loads`] gives a session call: `_meta`, the MCP servers and ACP's
/// `additionalDirectories`.
pub(super) type SessionLoads = (Map<String, Value>, Vec<McpServer>, Vec<PathBuf>);

/// The `_meta`, MCP servers and extra roots for `thread`'s `session/new`, `session/load` or
/// `session/resume`, whose matter folder is `matter`: the profile hook's `_meta` with the
/// host's switches and keys over it and its tokens filled (the hook gets this launch's own skill
/// packs token, so Claude's second plugin is this launch's); the servers the lawyer turned on, then
/// the hook's (none when the firm's `managed-mcp.json` is present); and for Codex, its run folder
/// and the matter's notes ([`codex_directories`]). Holds the session's tool state for the
/// thread's header until the session call answers ([`show_tools`]).
pub(super) fn session_loads(
    actor: &mut Actor,
    thread: ThreadId,
    matter: &Path,
) -> Result<SessionLoads, Error> {
    let agent_id = actor.check_thread(thread)?.row.agent.clone();
    let kind = AgentKind::of(&agent_id);
    let run: RunFolders = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.run.clone())
        .ok_or_else(|| {
            Error::refused(ErrorCode::AgentLaunchFailed, "the session has no folders")
        })?;
    let control = loads::mcp_control(&actor.managed_mcp);
    let (hook, case_law_entry, case_law_state) =
        case_law_servers(actor, &run, actor.profile.mcp_servers(&agent_id), control);
    let part = connectors_part(case_law_entry.is_some());
    let folders = Folders::of(&run.work, &run.notes)?.with_connectors(part);
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
        data: &run.data_deny,
        case_law: case_law_entry.is_some(),
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
            ErrorCode::AgentGuardMissing,
            "this matter's folder is unprotected, and the guard that makes Claude ask before \
             each change could not be set up",
        ));
    }
    let mut meta = loads::session_meta(
        actor
            .profile
            .session_meta(&agent_id, run.packs_written.as_deref()),
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
    let (hook_servers, browser_state) = browser::hook_servers(hook, &inputs);
    servers.extend(hook_servers);
    servers.extend(case_law_entry);
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
        .chain(case_law_state)
        .collect();
    super::super::header_tools::hold(actor, thread, tools);
    let directories = if kind == Some(AgentKind::Codex) {
        codex_directories(&run)
    } else {
        Vec::new()
    };
    Ok((meta, servers, directories))
}

/// The hook's servers without `case-law`, the `case-law` entry `run`'s session gets (after
/// the others), and its header state (`agent::loads::case_law`). A relay whose entry does not
/// go (the firm's `managed-mcp.json` appeared since the launch prep) is ended at once.
fn case_law_servers(
    actor: &Actor,
    run: &RunFolders,
    hook: Vec<McpServer>,
    control: loads::McpControl,
) -> (
    Vec<McpServer>,
    Option<McpServer>,
    Option<clauseo_protocol::ToolState>,
) {
    let relay = match &run.case_law {
        CaseLaw::Absent => Relay::Absent,
        CaseLaw::Unavailable(reason) | CaseLaw::Ended(_, reason) => Relay::Unavailable(*reason),
        CaseLaw::Open(relay) => Relay::Open {
            port: relay.port(),
            token_file: &run.relay_token,
        },
    };
    let system_root = case_law::system_root(
        cfg!(windows),
        match &actor.wiring.inherited_env {
            Some(env) => env.clone(),
            None if cfg!(windows) => std::env::vars_os().collect(),
            None => Vec::new(),
        },
    );
    let inputs = CaseLawInputs {
        relay,
        control,
        system_root: system_root.as_deref(),
    };
    let (rest, entry, state) = case_law::hook_servers(hook, &inputs);
    if entry.is_none()
        && let Some(relay) = run.case_law.relay()
    {
        relay.end();
    }
    (rest, entry, state)
}

/// What `{connectors}` becomes: the paragraph when the session gets the `case-law` entry.
fn connectors_part(with_case_law: bool) -> &'static str {
    clauseo_profile::instructions::connectors_part(with_case_law)
}

/// The firm's `managed-mcp.json`, read again right before the launch: one that appeared since
/// the launch prep opened the relay ends it, so the session gets no entry and no paragraph
/// (`ManagedByFirm`). What is left is the spawn and `initialize` (see `launch_keys`).
fn recheck_managed(actor: &Actor, run: &mut RunFolders) {
    if run.case_law.relay().is_some()
        && loads::mcp_control(&actor.managed_mcp) == loads::McpControl::Managed
    {
        let reason = clauseo_protocol::ToolsUnavailableReason::ManagedByFirm;
        run.case_law.end_before_launch(reason);
    }
}

/// Codex's ACP `additionalDirectories`: the session's run folder (its work folder and its pack
/// plugin), the matter's notes and the `tmp` of the matter's Codex state folder (its `CODEX_HOME`,
/// R17 PR-C; `agent::codex_fence::Session::state`). codex-acp makes each turn's `workspaceWrite`
/// roots from them, beside the matter folder and tmp (codex-acp 2.2.2 `dist/index.js:35598-35612,
/// 35878-35885`). With the data-folder fence's profile selected, Codex reads no `writable_roots`
/// from `CODEX_CONFIG`, so these and the profile's own `write` entries (`agent::codex_fence`) are
/// what reopen these folders under its deny of `<data>/run` and `<data>/agent-state`.
pub(super) fn codex_directories(run: &RunFolders) -> Vec<PathBuf> {
    [run.root.clone(), run.notes.clone()]
        .into_iter()
        .chain(
            run.state
                .as_deref()
                .map(crate::agent::codex_fence::shell_folder),
        )
        .collect()
}

/// The session call answered: the tool state [`session_loads`] held shows in the header.
pub(super) fn show_tools(actor: &mut Actor, thread: ThreadId) {
    super::super::header_tools::show_held(actor, thread);
}
