//! The launch profile tests' rig (`profile.rs`, `tools.rs`, `mcp.rs`): a launcher ([`Wired`])
//! that sees each launch after its prep made the session's folders and fills `@WORK@`,
//! `@BROWSER@` and `@SESSION@` into the scenario the agent is about to read, so every value is
//! checked exactly; a profile hook; and scenario builders.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_host::{McpServer, ProfileHook};
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher};
use clauseo_profile::SkillWrite;
use clauseo_protocol::{
    AgentId, ClauseoTool, MatterId, StopReason, StreamHeader, StreamId, ThreadId, ToolAvailability,
    ToolState, ToolsUnavailableReason, ViewPatchBody,
};
use serde_json::{Map, Value, json};

use crate::support::{Dir, Running, Scenario};
use crate::write;

pub(crate) const CLAUDE: &str = "claude-acp";
pub(crate) const CODEX: &str = "codex-acp";

/// One launch as [`Wired`] saw it: its session folder (`<data>/run/<session id>`, none for a
/// launch whose prep made none) and its environment.
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    pub(crate) run: Option<PathBuf>,
    pub(crate) env: BTreeMap<OsString, OsString>,
}

impl Seen {
    /// The `CODEX_CONFIG` object it got, if any.
    pub(crate) fn codex_config(&self) -> Option<Value> {
        let text = self.env.get(&OsString::from("CODEX_CONFIG"))?;
        serde_json::from_str(text.to_str()?).ok()
    }

    /// `<run>/work`.
    pub(crate) fn work(&self) -> String {
        path_text(&self.run.as_ref().expect("a run folder").join("work"))
    }
}

/// A launcher that fills each session's folders into the scenario the agent reads, then
/// launches it as the tests' own launcher does.
pub(crate) struct Wired {
    inner: PassthroughLauncher,
    data: PathBuf,
    known: Mutex<BTreeSet<PathBuf>>,
    pub(crate) seen: Arc<Mutex<Vec<Seen>>>,
}

impl Wired {
    pub(crate) fn new(dir: &Dir) -> Arc<Self> {
        Arc::new(Self {
            inner: dir.launcher(),
            data: dir.data().to_owned(),
            known: Mutex::new(BTreeSet::new()),
            seen: Arc::default(),
        })
    }

    /// Every launch so far.
    pub(crate) fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Launcher for Wired {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        let mut known = self.known.lock().unwrap();
        let run = std::fs::read_dir(self.data.join("run"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| !known.contains(path));
        if let Some(run) = &run {
            known.insert(run.clone());
        }
        let args: Vec<_> = spec
            .args()
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect();
        let scenario = args
            .iter()
            .position(|arg| arg == "--scenario")
            .and_then(|at| args.get(at + 1));
        if let (Some(run), Some(scenario)) = (&run, scenario) {
            let template = PathBuf::from(format!("{scenario}.tmpl"));
            if let Ok(text) = std::fs::read_to_string(&template) {
                let session = run.file_name().unwrap().to_str().unwrap();
                let text = text
                    .replace("@WORK@", &escaped(&path_text(&run.join("work"))))
                    .replace("@BROWSER@", &escaped(&path_text(&run.join("browser"))))
                    .replace("@SESSION@", &escaped(session));
                write::write(Path::new(scenario.as_ref()), text.as_bytes());
            }
        }
        self.seen.lock().unwrap().push(Seen {
            run,
            env: spec.env().clone(),
        });
        self.inner.launch(spec)
    }
}

/// `path` as text, `\\?\` and all, as the host fills it.
pub(crate) fn path_text(path: &Path) -> String {
    dunce::simplified(path).to_str().unwrap().to_owned()
}

/// `text` escaped for inside a JSON string.
pub(crate) fn escaped(text: &str) -> String {
    let quoted = serde_json::to_string(text).unwrap();
    quoted[1..quoted.len() - 1].to_owned()
}

/// `<data>/agent-state/<matter>/notes`.
pub(crate) fn notes(dir: &Dir, matter: MatterId) -> PathBuf {
    dir.data()
        .join("agent-state")
        .join(matter.to_string())
        .join("notes")
}

/// A profile hook: `meta` for a Claude agent, `codex` keys for a Codex agent, and `servers` for
/// every agent.
#[derive(Clone, Default)]
pub(crate) struct Hook {
    pub(crate) meta: Value,
    pub(crate) codex: Value,
    pub(crate) servers: Vec<McpServer>,
    pub(crate) skills: Option<SkillWrite>,
}

impl ProfileHook for Hook {
    fn session_meta(&self, agent: &AgentId) -> Option<Map<String, Value>> {
        (agent.as_str() == CLAUDE)
            .then(|| self.meta.as_object().cloned())
            .flatten()
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }

    fn mcp_servers(&self, _agent: &AgentId) -> Vec<McpServer> {
        self.servers.clone()
    }

    fn skill_files(&self, _agent: &AgentId) -> Option<SkillWrite> {
        self.skills.clone()
    }

    fn codex_config(&self, agent: &AgentId) -> Map<String, Value> {
        if agent.as_str() == CODEX {
            self.codex.as_object().cloned().unwrap_or_default()
        } else {
            Map::new()
        }
    }
}

/// The `browser` entry as `clauseo_profile::servers::browser` builds it.
pub(crate) fn browser_server() -> McpServer {
    McpServer::Stdio {
        name: "browser".to_owned(),
        command: std::env::temp_dir().join("clauseo-mcp"),
        args: [
            "browser-guard",
            "--agent-browser",
            "/app/agent-browser",
            "--cwd",
            "{browser_dir}",
            "--",
            "mcp",
            "--tools",
            "core",
        ]
        .map(str::to_owned)
        .to_vec(),
        env: vec![
            (
                "AGENT_BROWSER_CONFIG".to_owned(),
                "/cfg/agent-browser.json".to_owned(),
            ),
            (
                "AGENT_BROWSER_PROXY".to_owned(),
                "http://127.0.0.1:{proxy_port}".to_owned(),
            ),
            (
                "AGENT_BROWSER_SESSION".to_owned(),
                "{session_id}".to_owned(),
            ),
            (
                "AGENT_BROWSER_EXECUTABLE_PATH".to_owned(),
                "{browser_executable}".to_owned(),
            ),
        ],
    }
}

/// The `browser` entry `session/new` carries once filled, with the proxy on 8123 and Chrome at
/// `/chrome`, for a template.
pub(crate) fn filled_browser() -> Value {
    json!({"name": "browser", "command": "{{any}}",
    "args": ["browser-guard", "--agent-browser", "/app/agent-browser", "--cwd", "@BROWSER@",
        "--", "mcp", "--tools", "core"],
    "env": [
        {"name": "AGENT_BROWSER_CONFIG", "value": "/cfg/agent-browser.json"},
        {"name": "AGENT_BROWSER_PROXY", "value": "http://127.0.0.1:8123"},
        {"name": "AGENT_BROWSER_SESSION", "value": "@SESSION@"},
        {"name": "AGENT_BROWSER_EXECUTABLE_PATH", "value": "/chrome"},
    ]})
}

/// A generic agent's scenario: `initialize`, `method` (`session/new` or `session/load` of
/// `sess-1`) matching `params` with `off` off, one prompt.
pub(crate) fn agent_lines(name: &str, method: &str, params: &Value, off: &[&str]) -> String {
    let label = if method == "session/new" {
        "new"
    } else {
        "load"
    };
    let respond = if method == "session/new" {
        json!({"sessionId": "sess-1"})
    } else {
        json!({})
    };
    [
        json!({"scenario": name, "description": "a launch profile test", "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"expect": {"method": method, "as": label, "params": params, "off": off}}),
        json!({"respond": {"to": label, "result": respond}}),
        json!({"expect": {"method": "session/prompt", "as": "p", "params": {"sessionId": "sess-1"}}}),
        json!({"update": {"update": {"sessionUpdate": "agent_message_chunk", "messageId": format!("m-{label}"),
            "content": {"type": "text", "text": "Done."}}}}),
        json!({"respond": {"to": "p", "result": {"stopReason": "end_turn"}}}),
    ]
    .map(|line| line.to_string())
    .join("\n")
}

/// A Codex agent's scenario, as `codex-handshake` (codex-acp 2.2.2): `initialize`,
/// `session/new` matching `params`, the switch to its ask mode, one prompt.
pub(crate) fn codex_lines(name: &str, params: &Value) -> String {
    [
        json!({"scenario": name, "description": "a launch profile test, Codex", "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1, "agentCapabilities": {
            "loadSession": true, "promptCapabilities": {"image": true, "embeddedContext": true},
            "mcpCapabilities": {"http": true, "sse": false}, "sessionCapabilities": {"list": {},
            "delete": {}, "additionalDirectories": {}, "resume": {}, "close": {}}},
            "authMethods": [], "agentInfo": {"name": "@agentclientprotocol/codex-acp",
            "title": "Codex", "version": "2.2.2"}}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": params}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1", "modes": {
            "currentModeId": "agent", "availableModes": [{"id": "read-only", "name": "Read-only"},
            {"id": "workspace-write", "name": "Workspace access"}, {"id": "agent", "name": "Auto review"},
            {"id": "agent-full-access", "name": "Full access"}]}}}}),
        json!({"expect": {"method": "session/set_mode", "as": "set-mode",
            "params": {"sessionId": "sess-1", "modeId": "workspace-write"}}}),
        json!({"respond": {"to": "set-mode", "result": {}}}),
        json!({"expect": {"method": "session/prompt", "as": "p", "params": {"sessionId": "sess-1"}}}),
        json!({"update": {"update": {"sessionUpdate": "agent_message_chunk", "messageId": "m-codex",
            "content": {"type": "text", "text": "Done."}}}}),
        json!({"respond": {"to": "p", "result": {"stopReason": "end_turn"}}}),
    ]
    .map(|line| line.to_string())
    .join("\n")
}

/// Writes `lines` as the template of the scenario `name` (filled at each launch) and returns
/// the scenario.
pub(crate) fn template(dir: &Dir, name: &str, lines: &str) -> Scenario {
    let folder = dir.data().join("scenarios");
    write::create_dir_all(&folder);
    let path = folder.join(format!("{name}.jsonl"));
    write::write(&folder.join(format!("{name}.jsonl.tmpl")), lines.as_bytes());
    // A placeholder until the first launch fills it.
    write::write(&path, lines.as_bytes());
    Scenario::Written(path)
}

/// The settings' `agents` object: each id runs `fake_agent` with its scenario and `env`.
pub(crate) fn agents(entries: &[(&str, &Scenario, Value)]) -> Value {
    let bin = clauseo_fake_agent::bin_path().unwrap();
    let mut map = Map::new();
    for (id, scenario, env) in entries {
        map.insert(
            (*id).to_owned(),
            json!({"command": bin.display().to_string(), "args": ["--scenario", scenario.arg()],
                "env": env}),
        );
    }
    json!({"agents": map})
}

/// Writes the settings: `agents` and the sections of `extra`.
pub(crate) fn settings(dir: &Dir, agents: Value, extra: Value) {
    let mut all = agents;
    if let (Some(all), Value::Object(extra)) = (all.as_object_mut(), extra) {
        all.extend(extra);
    }
    dir.set_settings(&[], all);
}

/// Runs one prompt on `thread` to its end, with no scenario mismatch.
pub(crate) fn turn(host: &Running, thread: ThreadId) {
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Go.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
}

/// The tools the header of `thread` shows on a new subscriber's first frame.
pub(crate) fn first_frame_tools(host: &Running, thread: ThreadId) -> Vec<ToolState> {
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until_items(1);
    let first = view.patches().next().expect("a first frame");
    match &first.body {
        ViewPatchBody::Reset { view_window } => match &view_window.header {
            Some(StreamHeader::Thread(header)) => header.tools.clone(),
            other => panic!("no thread header: {other:?}"),
        },
        other => panic!("the first frame is not a reset: {other:?}"),
    }
}

pub(crate) fn tool(tool: ClauseoTool, reason: Option<ToolsUnavailableReason>) -> ToolState {
    ToolState {
        tool,
        availability: match reason {
            Some(reason) => ToolAvailability::Unavailable { reason },
            None => ToolAvailability::Available,
        },
    }
}
