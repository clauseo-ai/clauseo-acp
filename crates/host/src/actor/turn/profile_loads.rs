//! The launch profile's per-session values, gathered on the actor: the folders, the `tools.*`
//! switches, the login, the proxy port and the browser inputs, handed to `agent::loads`, which
//! does the merging. Path joins and map work only: the folders were made on the launch-prep
//! thread (`launch_checks.rs`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_acp::McpServer;
use clauseo_profile::instructions::BrowserPart;
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
        let shell = shell_browser(actor, &prepared.run);
        let folders = Folders::of(&prepared.run.work, &prepared.run.notes)?
            .with_connectors(part)
            .with_browser(browser_part(shell.as_ref()));
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
            codex_home: &codex.home,
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
    let shell = shell_browser(actor, &run);
    let folders = Folders::of(&run.work, &run.notes)?
        .with_connectors(part)
        .with_browser(browser_part(shell.as_ref()));
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
        proxy_set: actor.wiring.proxy.is_some(),
        // Only a proxy that opened for this session gives a port (`launch_checks::egress`).
        port: run.egress.as_ref().and_then(|egress| egress.port()),
        blocked_by_policy: run.browser_blocked,
        executable: run.chrome.as_deref(),
        browser_dir: &run.browser,
        claims: run.browser_session.as_deref().map(Claims::of),
    };
    if run.browser_policy_read {
        // What the app heard from a "Try again" follows this newer read.
        super::super::browser_policy::on_launch_read(actor, run.browser_blocked);
    }
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
    // The shell tool's state stands in for the parked entry's (`servers::BROWSER_MCP`).
    let browser_state = browser_state.or(shell.map(|(state, _)| state));
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
    let windows_env = case_law::windows_env(cfg!(windows), lookup, case_law::windows_directory);
    let inputs = CaseLawInputs {
        relay,
        control,
        windows_env: &windows_env,
    };
    let (rest, entry, state) = case_law::hook_servers(hook, &inputs);
    if entry.is_none()
        && let Some(relay) = run.case_law.relay()
    {
        relay.end();
    }
    (rest, entry, state)
}

/// The shell tool's header state and browser part for `run`'s session, or none when the app
/// wired no shell tool (`ProfileHook::browser_tool`).
fn shell_browser(
    actor: &Actor,
    run: &RunFolders,
) -> Option<(clauseo_protocol::ToolState, BrowserPart)> {
    actor.profile.browser_tool()?;
    let (_, switch) = switches(actor);
    let found = run.browser_shell && run.chrome.is_some();
    Some(browser::shell_state(switch, run.browser_blocked, found))
}

/// `env` (the hook's variables) with agent-browser's (`agent::loads::browser::shell_env`) for a
/// session with the browser as a shell tool: its launch prep found its Chrome and made its
/// folders. They go over the agent's own and the settings' variables, under what the host
/// forces for the agent's kind.
pub(super) fn with_shell_env(
    actor: &Actor,
    run: &RunFolders,
    mut env: std::collections::BTreeMap<OsString, OsString>,
) -> std::collections::BTreeMap<OsString, OsString> {
    let Some(executable) = shell_chrome(actor, run) else {
        return env;
    };
    let config = actor.effective.locations().data_dir();
    let config = config
        .join(crate::browser::CONFIG_DIR)
        .join(crate::browser::CONFIG_FILE);
    let session = run.key.id();
    let inputs = browser::ShellInputs {
        session: &session,
        config: &config,
        downloads: &run.downloads,
        profile: &run.browser_profile,
        executable,
    };
    env.extend(browser::shell_env(&inputs));
    env
}

/// The shell browser's tools folder for the agent's `PATH` (after stetdoc's, before the profile
/// hook's: `agents::path::launch_parts`), for a session that has it.
pub(super) fn browser_tools(actor: &Actor, run: &RunFolders) -> Option<PathBuf> {
    shell_chrome(actor, run).and_then(|_| actor.profile.browser_tool())
}

/// The Chrome of `run`'s session when it has the browser as a shell tool.
fn shell_chrome<'a>(actor: &Actor, run: &'a RunFolders) -> Option<&'a Path> {
    actor.profile.browser_tool()?;
    run.chrome.as_deref().filter(|_| run.browser_shell)
}

/// What `{browser}` becomes: the shell tool's part, or the no-browser sentence without it.
fn browser_part(shell: Option<&(clauseo_protocol::ToolState, BrowserPart)>) -> &'static str {
    let part = shell.map_or(BrowserPart::Off, |(_, part)| *part);
    clauseo_profile::instructions::browser_part(part)
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
/// plugin) and the matter's notes. codex-acp makes each turn's `workspaceWrite` roots from them,
/// beside the matter folder and tmp (codex-acp 2.2.2 `dist/index.js:35598-35612, 35878-35885`).
/// With the data-folder fence's profile selected, Codex reads no `writable_roots` from
/// `CODEX_CONFIG`, so these and the profile's own `write` entries (`agent::codex_fence`) are what
/// reopen both folders under its deny of `<data>/run` and `<data>/agent-state`.
pub(super) fn codex_directories(run: &RunFolders) -> Vec<PathBuf> {
    vec![run.root.clone(), run.notes.clone()]
}

/// The session call answered: the tool state [`session_loads`] held shows in the header.
pub(super) fn show_tools(actor: &mut Actor, thread: ThreadId) {
    super::super::header_tools::show_held(actor, thread);
}
