//! The launch profile's per-session values, gathered on the actor: the folders, the `tools.*`
//! switches, the login, the proxy port and the browser inputs, handed to `agent::loads`, which
//! does the merging. Path joins and map work only: the folders were made on the launch-prep
//! thread (`launch_checks.rs`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_acp::McpServer;
use clauseo_profile::instructions::BrowserPart;
use clauseo_protocol::{AgentId, AgentKind, ErrorCode, ThreadId, ToolAvailability};
use serde_json::{Map, Value};

use super::super::Actor;
use super::launch_checks::relay::CaseLaw;
use super::launch_checks::{Prepared, RunFolders};
use crate::agent::loads;
use crate::agent::loads::browser::{self, BrowserInputs, Claims, Switch};
use crate::agent::loads::case_law::{self, CaseLawInputs, Relay};
use crate::agent::loads::lawyer;
use crate::agent::loads::profile::{self, ClaudeKeys, Folders, Login};
use crate::agent::loads::sandbox::{ClaudeSandbox, Off, SandboxOs, SandboxPaths};
use crate::browser::seed_sites::SEED_SITES;
use crate::error::Error;
use crate::log_file::LIFECYCLE;

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
        // The part `session_loads` would fill, from the same inputs: the browser entry's state
        // as it stands now (a slot taken in between leaves wrong words, but no browser).
        let control = loads::mcp_control(&actor.managed_mcp);
        let (_, browser_switch) = switches(actor);
        let matter = super::session::folder(actor, thread)?;
        let downloads = matter.join(browser::DOWNLOADS_FOLDER);
        let inputs = browser_inputs(actor, &prepared.run, (control, browser_switch), &downloads);
        let (_, entry) = browser::hook_servers(actor.profile.mcp_servers(&agent_id), &inputs);
        let shell = shell_browser(actor, &prepared.run, &agent_id);
        let kind = AgentKind::of(&agent_id);
        let none_here = browser::no_browser(kind, cfg!(windows)).then_some(BrowserPart::Off);
        let browser = session_part(shell.as_ref(), entry.as_ref(), &prepared.run, none_here);
        let folders = Folders::of(&prepared.run.work, &prepared.run.notes)?
            .with_connectors(part)
            .with_browser(browser_part(browser, shell.is_none()));
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
            listed: &prepared.run.codex_listed,
            socket_dir: socket_dir(&prepared.run),
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

/// Notes on `thread`'s agent what may run unasked in its session (`agent/mcp_trust.rs`): only
/// what Clauseo sent, the lawyer's `trusted` servers and `case-law` when its entry went.
fn note_mcp_sent(actor: &mut Actor, thread: ThreadId, trusted: &[String], case_law: bool) {
    let mut sent = trusted.to_vec();
    if case_law {
        sent.push(clauseo_profile::servers::CASE_LAW.to_owned());
    }
    if let Some(agent) = super::state(actor, thread).and_then(|state| state.agent.as_deref_mut())
        && let Some(slot) = agent.mcp_sent.as_mut()
    {
        *slot = sent;
    }
}

/// The `_meta`, MCP servers and extra roots for `thread`'s `session/new`, `session/load` or
/// `session/resume`, whose matter folder is `matter`: the profile hook's `_meta` with the
/// host's switches and keys over it and its tokens filled (the hook gets this launch's own skill
/// packs token, so Claude's second plugin is this launch's); the servers the lawyer turned on, then
/// the hook's (none when the firm's `managed-mcp.json` is present; none of the lawyer's in the
/// Safe workspace, nor one whose keychain value is missing); and for Codex, its run folder
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
    let none_here = browser::no_browser(kind, cfg!(windows));
    let offered = browser::offered(actor.profile.mcp_servers(&agent_id), none_here);
    let (hook, case_law_entry, case_law_state) = case_law_servers(actor, &run, offered, control);
    let part = connectors_part(case_law_entry.is_some());
    let (web_search, browser_switch) = switches(actor);
    let downloads = matter.join(browser::DOWNLOADS_FOLDER);
    let inputs = browser_inputs(actor, &run, (control, browser_switch), &downloads);
    // Codex's `browser` entry (`clauseo_profile::servers::browser_mcp`), filled or left out.
    let (hook_servers, browser_state) = browser::hook_servers(hook, &inputs);
    // The shell tool (no agent's for launch); none for an agent offered the entry.
    let shell = shell_browser(actor, &run, &agent_id);
    let absent = none_here.then_some(BrowserPart::Off);
    let session_browser = session_part(shell.as_ref(), browser_state.as_ref(), &run, absent);
    if shell.is_some() || browser_state.is_some() || none_here {
        // For support: whether this session has the browser, and which Chrome by file name.
        let (browser, chrome) = browser::part_log(session_browser, run.chrome.as_deref());
        tracing::info!(target: LIFECYCLE, %thread, browser, chrome, "browser part filled");
    }
    let folders = Folders::of(&run.work, &run.notes)?
        .with_connectors(part)
        .with_browser(browser_part(session_browser, shell.is_none()));
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
    let browser_on = shell
        .as_ref()
        .is_some_and(|(_, part)| *part == BrowserPart::On);
    let sandbox = session_sandbox(actor, thread, kind, &agent_id, &run, matter);
    // The lawyer's servers this session gets, and those left out (a keychain value or the
    // program missing, the Safe workspace).
    let (lawyer_sent, left_out) = lawyer_servers(actor, &run, control);
    // Of those, the ones that run unasked (`agent/mcp_trust.rs`): only a server this session is
    // sent, so the trust never names one left out.
    let mut trusted =
        crate::agent::mcp_trust::trusted_servers(&actor.effective.settings().mcp.servers, control);
    trusted.retain(|name| lawyer_sent.iter().any(|server| server.name() == name));
    let claude = (kind == Some(AgentKind::Claude)).then_some(ClaudeKeys {
        web_search,
        login,
        folders,
        seed_sites: SEED_SITES,
        forced_ask,
        data: &run.data_deny,
        case_law: case_law_entry.is_some(),
        browser: browser_on,
        sandbox: sandbox.as_ref(),
        matter: Some(matter),
        trusted_servers: &trusted,
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
    if run.browser_policy_read {
        // What the app heard from a "Try again" follows this newer read.
        super::super::browser_policy::on_launch_read(actor, run.browser_blocked);
    }
    let mut servers = lawyer_sent;
    super::super::mcp::left_out(actor, left_out);
    note_mcp_sent(actor, thread, &trusted, case_law_entry.is_some());
    servers.extend(hook_servers);
    servers.extend(case_law_entry);
    if browser_state
        .as_ref()
        .is_some_and(|state| state.availability == ToolAvailability::Available)
    {
        // The session holds its slot until it ends (`launch_checks::discard`).
        actor.wiring.browser_slots.take(run.key);
    }
    // Claude's shell tool has no entry: its own state shows instead.
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

/// A Claude session's sandbox ([`claude_sandbox`]); none for another agent. Where one was meant
/// to be and is not, holds `auto` back for `thread`.
fn session_sandbox(
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
        super::session::hold_auto_back(actor, thread);
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
    ClaudeSandbox::for_os(SandboxOs::CURRENT, &paths)
}

/// The lawyer's servers `run`'s session gets (`agent::loads::lawyer`), and those left out for a
/// keychain value they lack. None in the Safe workspace.
fn lawyer_servers(
    actor: &Actor,
    run: &RunFolders,
    control: loads::McpControl,
) -> (Vec<McpServer>, Vec<lawyer::LeftOut>) {
    let host: Vec<(OsString, OsString)> = match &actor.wiring.inherited_env {
        Some(env) => env.clone(),
        None => std::env::vars_os().collect(),
    };
    let lookup = |name: &str| {
        host.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    };
    let windows_env = case_law::windows_env(cfg!(windows), lookup, case_law::windows_directory);
    let path = host
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .and_then(|(key, value)| Some((key.to_str()?, value)));
    let lawyer = lawyer::Lawyer {
        control,
        safe_workspace: actor.protection.safe_workspace(),
    };
    let env = lawyer::HostEnv {
        path,
        windows_env: &windows_env,
    };
    lawyer::servers(&actor.effective, lawyer, &run.mcp_secrets, env)
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

/// What the `browser` entry of `run`'s session needs (`agent::loads::browser`), whose
/// matter's downloads folder is `matter_downloads`.
fn browser_inputs<'a>(
    actor: &Actor,
    run: &'a RunFolders,
    (control, switch): (loads::McpControl, Switch),
    matter_downloads: &'a Path,
) -> BrowserInputs<'a> {
    BrowserInputs {
        switch,
        control,
        slot_free: actor.wiring.browser_slots.free_for(run.key),
        proxy_set: actor.wiring.proxy.is_some(),
        // Only a proxy that opened for this session gives a port (`launch_checks::egress`).
        port: run.egress.as_ref().and_then(|egress| egress.port()),
        blocked_by_policy: run.browser_blocked,
        executable: run.chrome.as_deref(),
        browser_dir: &run.browser,
        matter_downloads,
        claims: run.browser_session.as_deref().map(Claims::of),
    }
}

/// Whether the profile hook offers `agent` the `browser` MCP entry (Codex:
/// `clauseo_profile::servers::browser_mcp`), which takes the shell tool's place.
/// The session's browser socket folder, for Codex's fence (`agent::codex_fence::Session`): the
/// folder as made, when the session holds a browser claim (macOS and Linux).
fn socket_dir(run: &RunFolders) -> Option<&Path> {
    let session = run.browser_session.as_deref()?;
    #[cfg(unix)]
    {
        Some(session.socket_dir.path())
    }
    #[cfg(not(unix))]
    {
        let _ = session;
        None
    }
}

fn offers_entry(actor: &Actor, agent: &AgentId) -> bool {
    actor
        .profile
        .mcp_servers(agent)
        .iter()
        .any(|server| server.name() == clauseo_profile::servers::BROWSER)
}

/// The shell tool's header state and browser part for `run`'s session, or none when the app
/// wired no shell tool (`ProfileHook::browser_tool`) or `agent` is offered the `browser` entry
/// instead ([`offers_entry`]).
fn shell_browser(
    actor: &Actor,
    run: &RunFolders,
    agent: &AgentId,
) -> Option<(clauseo_protocol::ToolState, BrowserPart)> {
    actor.profile.browser_tool()?;
    if offers_entry(actor, agent) || browser::no_browser(AgentKind::of(agent), cfg!(windows)) {
        return None;
    }
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
    let session = run.key.id();
    let inputs = browser::ShellInputs {
        session: &session,
        config: &run.browser_config,
        downloads: &run.downloads,
        profile: &run.browser_profile,
        executable,
    };
    env.extend(browser::shell_env(&inputs));
    env
}

/// The Chrome of `run`'s session when it has the browser as a shell tool.
fn shell_chrome<'a>(actor: &Actor, run: &'a RunFolders) -> Option<&'a Path> {
    actor.profile.browser_tool()?;
    run.chrome.as_deref().filter(|_| run.browser_shell)
}

/// Which browser part `run`'s session gets: `none_here`'s for an agent with no browser on this
/// OS (`browser::no_browser`: the no-browser sentence for Claude anywhere and Codex on
/// Windows); else the shell tool's (no agent's for launch), else the one the `browser` entry's
/// state gives (Codex, `browser::entry_part`), else none.
fn session_part(
    shell: Option<&(clauseo_protocol::ToolState, BrowserPart)>,
    entry: Option<&clauseo_protocol::ToolState>,
    run: &RunFolders,
    none_here: Option<BrowserPart>,
) -> BrowserPart {
    if let Some(part) = none_here {
        return part;
    }
    match (shell, entry) {
        (Some((_, part)), _) => *part,
        (None, Some(state)) => browser::entry_part(state, run.chrome.is_some()),
        (None, None) => BrowserPart::Off,
    }
}

/// What `{browser}` becomes for `part`: with the browser, the line naming the `browser` entry's
/// tools for a session without the shell tool (`mcp`: Codex's), the shell tool's rules for one
/// with it (Claude's); without, the no-browser sentence.
fn browser_part(part: BrowserPart, mcp: bool) -> &'static str {
    clauseo_profile::instructions::browser_part_for(part, mcp)
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
