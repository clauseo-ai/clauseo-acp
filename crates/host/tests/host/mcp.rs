//! What a session loads: the profile hook's `_meta`, and only the MCP servers the lawyer turned
//! on, with the switches that keep every other server out. Each scenario checks the request;
//! a mismatch makes `fake_agent` exit 3 before the turn can end.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex};

use clauseo_host::ProfileHook;
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher};
use clauseo_protocol::{AgentId, StopReason, StreamId};
use serde_json::{Map, Value, json};

use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

/// A profile hook returning a fixed `_meta` and fixed variables.
struct Hook(Value, BTreeMap<OsString, OsString>);

impl ProfileHook for Hook {
    fn session_meta(
        &self,
        _agent: &AgentId,
        _packs: Option<&clauseo_profile::PacksWritten>,
    ) -> Option<Map<String, Value>> {
        self.0.as_object().cloned()
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        self.1.clone()
    }
}

/// A launcher that keeps each spec's environment, then launches it as the tests' own does.
struct Recording {
    inner: PassthroughLauncher,
    envs: Arc<Mutex<Vec<BTreeMap<OsString, OsString>>>>,
}

impl Launcher for Recording {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        self.envs.lock().unwrap().push(spec.env().clone());
        self.inner.launch(spec)
    }
}

/// A hook that asks for what the host overrides: every setting source, also through an extra
/// flag in both forms, no strict MCP config, claude.ai connectors on.
fn greedy_hook() -> Value {
    json!({"clauseo_test": true, "claudeCode": {"options": {
        "strictMcpConfig": false,
        "settingSources": ["user", "project", "local"],
        "extraArgs": {
            "setting-sources": "user,project,local",
            "setting-sources=project": null,
            "model-x": "kept",
        },
        "env": {"ENABLE_CLAUDEAI_MCP_SERVERS": "true"},
    }}})
}

/// A server's absolute command under `bin`; nothing launches it.
fn command(bin: &Path, name: &str) -> String {
    bin.join(name).display().to_string()
}

/// Starts run `run` with `hook`, if any.
fn start(dir: &Dir, run: u16, hook: Option<Value>) -> Running {
    dir.start_with(run, |config| match hook {
        Some(meta) => config.with_profile_hook(Box::new(Hook(meta, BTreeMap::new()))),
        None => config,
    })
}

/// Opens a thread and runs one prompt to its end: the scenario accepted `session/new`.
fn one_turn(dir: &Dir, host: &Running) -> clauseo_protocol::ThreadId {
    let (_, thread, _) = matter_with_thread(dir, host, "m", &[("brief.txt", "Clause 4\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Summarise clause 4.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    thread
}

/// A folder whose one agent plays `scenario`, with no MCP server in the settings.
fn dir_playing(test: &str, scenario: &'static str) -> Dir {
    Dir::new(test, &[(FAKE, Scenario::Local(scenario))])
}

#[test]
fn profile_hook_meta_reaches_session_new() {
    let dir = dir_playing("profile_meta", "profile-meta");
    let host = start(&dir, 0, Some(json!({"clauseo_test": true})));
    one_turn(&dir, &host);
    host.shutdown();
}

#[test]
fn with_nothing_turned_on_no_server_goes_and_the_switches_do() {
    let dir = dir_playing("mcp_defaults", "mcp-defaults");
    let host = start(&dir, 0, None);
    one_turn(&dir, &host);
    host.shutdown();
}

/// The done-when of clauseo-acp-private#185: `settingSources` is `["user"]` on new and on load,
/// even when the hook asks for project and local settings.
#[test]
fn only_enabled_servers_reach_new_then_load_with_the_switches_over_the_hook() {
    let dir = Dir::new("mcp_enabled", &[]);
    let bin = dir.folder("bin", &[]);
    let mcp = json!({"mcp": {"servers": {
        "alpha": {"command": command(&bin, "alpha"), "args": ["--stdio"],
            "env": {"REGION": "uk"}, "enabled": true},
        "beta": {"command": command(&bin, "beta")},
        "gamma": {"command": command(&bin, "gamma"), "enabled": true},
    }}});
    dir.set_settings(&[(FAKE, Scenario::Local("mcp-enabled-new"))], mcp.clone());
    let host = start(&dir, 0, Some(greedy_hook()));
    let thread = one_turn(&dir, &host);
    host.shutdown();

    dir.set_settings(&[(FAKE, Scenario::Local("mcp-enabled-load"))], mcp);
    let host = start(&dir, 1, Some(greedy_hook()));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, "And clause 5?");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(events.turn_text(turn), "Second answer.");
    host.shutdown();
}

#[test]
fn a_policy_list_replaces_the_users() {
    let dir = Dir::new("mcp_policy", &[]);
    let bin = dir.folder("bin", &[]);
    let user = json!({"mcp": {"servers": {
        "alpha": {"command": command(&bin, "alpha"), "enabled": true},
    }}});
    dir.set_settings(&[(FAKE, Scenario::Local("mcp-policy"))], user);
    let policy = json!({"mcp": {"servers": {
        "firm": {"command": command(&bin, "firm"), "enabled": true},
        "alpha": {"command": command(&bin, "alpha"), "enabled": false},
    }}});
    dir.set_policy(&policy.to_string());
    let host = start(&dir, 0, None);
    one_turn(&dir, &host);
    host.shutdown();
}

/// R24 (decision D7): a firm's policy server named `case-law` (the name of Clauseo's case-law
/// connector, in any case) is dropped when settings load, so the launch carries the firm's other
/// server alone: the same `mcp-policy` scenario, which accepts only `firm`, passes.
#[test]
fn a_policy_server_named_case_law_never_reaches_the_launch() {
    let dir = Dir::new("mcp_policy_case_law", &[]);
    let bin = dir.folder("bin", &[]);
    let user = json!({"mcp": {"servers": {
        "case-law": {"command": command(&bin, "mine"), "enabled": true},
    }}});
    dir.set_settings(&[(FAKE, Scenario::Local("mcp-policy"))], user);
    let policy = json!({"mcp": {"servers": {
        "firm": {"command": command(&bin, "firm"), "enabled": true},
        "case-law": {"command": command(&bin, "firm-case-law"), "enabled": true},
        "CASE-LAW": {"command": command(&bin, "firm-case-law"), "enabled": true},
    }}});
    dir.set_policy(&policy.to_string());
    let host = start(&dir, 0, None);
    one_turn(&dir, &host);
    host.shutdown();
}

/// A policy list that is not an object, and a `policy.json` that does not parse, both lock the
/// list empty: the user's enabled server does not go.
#[test]
fn a_broken_policy_turns_every_server_off() {
    for (test, policy) in [
        ("mcp_policy_invalid", r#"{"mcp": {"servers": "all"}}"#),
        ("mcp_policy_unreadable", "{ not json"),
    ] {
        let dir = Dir::new(test, &[]);
        let bin = dir.folder("bin", &[]);
        let user = json!({"mcp": {"servers": {
            "alpha": {"command": command(&bin, "alpha"), "enabled": true},
        }}});
        dir.set_settings(&[(FAKE, Scenario::Local("mcp-defaults"))], user);
        dir.set_policy(policy);
        let host = start(&dir, 0, None);
        one_turn(&dir, &host);
        host.shutdown();
    }
}

/// Claude Code's enterprise `managed-mcp.json`: no servers and no strict MCP config, even when
/// the hook asks for strict, sets servers or passes the MCP flags.
#[test]
fn under_managed_mcp_no_server_and_no_strict_go() {
    let dir = Dir::new("mcp_managed", &[]);
    let bin = dir.folder("bin", &[]);
    let user = json!({"mcp": {"servers": {
        "alpha": {"command": command(&bin, "alpha"), "enabled": true},
    }}});
    dir.set_settings(&[(FAKE, Scenario::Local("mcp-managed"))], user);
    dir.set_managed_mcp();
    let hook = json!({"claudeCode": {"options": {
        "strictMcpConfig": true,
        "mcpServers": {"clauseo": {"command": command(&bin, "clauseo")}},
        "extraArgs": {"mcp-config": "{}", "strict-mcp-config": null, "model-x": "kept"},
    }}});
    let host = start(&dir, 0, Some(hook));
    one_turn(&dir, &host);
    host.shutdown();
}

/// #123's launch half: the hook's variables reach the agent's environment, and the host's
/// `ENABLE_CLAUDEAI_MCP_SERVERS=false` replaces the hook's own, in any case.
#[test]
fn the_hooks_launch_variables_and_the_connector_switch_reach_the_agent() {
    let dir = dir_playing("mcp_launch_env", "mcp-defaults");
    let envs = Arc::new(Mutex::new(Vec::new()));
    let launcher = Recording {
        inner: dir.launcher(),
        envs: Arc::clone(&envs),
    };
    let variables = BTreeMap::from([
        (OsString::from("CLAUSEO_FIRM"), OsString::from("acme")),
        (
            OsString::from("enable_claudeai_mcp_servers"),
            OsString::from("true"),
        ),
    ]);
    let host = dir.start_launching(0, Arc::new(launcher), |config| {
        config.with_profile_hook(Box::new(Hook(json!({}), variables)))
    });
    one_turn(&dir, &host);
    host.shutdown();
    let envs = envs.lock().unwrap();
    let [env] = envs.as_slice() else {
        panic!("one launch, not {}", envs.len());
    };
    assert_eq!(
        env.get(&OsString::from("CLAUSEO_FIRM")),
        Some(&OsString::from("acme"))
    );
    let switches: Vec<_> = env
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("ENABLE_CLAUDEAI_MCP_SERVERS"))
        .collect();
    assert_eq!(
        switches,
        [(
            &OsString::from("ENABLE_CLAUDEAI_MCP_SERVERS"),
            &OsString::from("false")
        )]
    );
}

/// R21's done-when "Hook servers": the lawyer's enabled servers first, in name order, then the
/// hook's `browser` with its per-session values, on `session/new` and then `session/load`, for a
/// `claude-acp` and a `codex-acp` agent (both `fake_agent`). The new session plays the
/// checked-in `tools-mcp-servers`; the load and the Codex session check the filled folder and
/// session id exactly.
#[test]
fn hook_servers_follow_the_lawyers_with_their_per_session_values() {
    use crate::profile_rig::{
        CLAUDE, CODEX, Hook, Wired, agent_lines, agents, browser_server, codex_lines,
        filled_browser, settings, template, turn,
    };
    let dir = Dir::new("mcp_hook_servers", &[]);
    let bin = dir.folder("bin", &[]);
    let mcp = json!({"tools": {"browser": true}, "mcp": {"servers": {
        "alpha": {"command": command(&bin, "alpha"), "args": ["--stdio"],
            "env": {"REGION": "uk"}, "enabled": true},
        "beta": {"command": command(&bin, "beta")},
        "gamma": {"command": command(&bin, "gamma"), "enabled": true},
    }}});
    let lawyers = json!([
        {"name": "alpha", "command": "{{any}}", "args": ["--stdio"],
            "env": [{"name": "REGION", "value": "uk"}]},
        {"name": "gamma", "command": "{{any}}", "args": [], "env": []},
    ]);
    let mut servers = lawyers.as_array().cloned().unwrap();
    servers.push(filled_browser());
    let codex = template(
        &dir,
        "hs-codex",
        &codex_lines("hs-codex", &json!({"mcpServers": servers})),
    );
    let claude = crate::support::Scenario::Local("tools-mcp-servers");
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({})), (CODEX, &codex, json!({}))]),
        mcp.clone(),
    );
    let hook = Hook {
        servers: vec![browser_server()],
        ..Hook::default()
    };
    let wire = |config: clauseo_host::HostConfig, hook: Hook| {
        config
            .with_profile_hook(Box::new(hook))
            .with_egress_proxy(Arc::new(clauseo_host::TestProxy(8123)))
            .with_browser_executable(Some(std::path::PathBuf::from("/chrome")))
    };
    let launcher = Wired::new(&dir);
    let first = hook.clone();
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        wire(config, first)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    turn(&host, host.create_thread(matter, CODEX));
    host.shutdown();

    let load = template(
        &dir,
        "hs-load",
        &agent_lines(
            "hs-load",
            "session/load",
            &json!({"sessionId": "sess-1", "mcpServers": servers}),
            &[],
        ),
    );
    settings(&dir, agents(&[(CLAUDE, &load, json!({}))]), mcp);
    let host = dir.start_launching(1, launcher, |config| wire(config, hook));
    turn(&host, thread);
    host.shutdown();
}
