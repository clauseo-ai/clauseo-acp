//! R24's launch wiring at the session, against S28's fake connector: the `case-law` entry in
//! `session/new` and `session/load` for a `claude-acp` and a `codex-acp` agent (`fake_agent`),
//! after the lawyer's servers and `browser`; its token file per session, gone at the end;
//! Claude's pre-allow; the connectors paragraph in both agents' instructions; the header; and
//! every way to no entry and no relay.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use clauseo_connectors::fake::{FakeConnector, FakeHandle, FakeOptions, ScratchDir};
use clauseo_connectors::{ConnectorHandle, Connectors, MemoryStore, SecretStore};
use clauseo_host::connectors::{FixedOffset, RelayConfig, RelayEvent, RelayEvents, Relays};
use clauseo_host::{HostConfig, McpServer, SystemClock, TestProxy};
use clauseo_protocol::{ClauseoTool, ToolState, ToolsUnavailableReason};
use serde_json::{Value, json};

use crate::profile_rig::{
    CLAUDE, CODEX, Hook, Seen, Wired, agent_lines, agents, browser_server, codex_lines,
    filled_browser, first_frame_tools, settings, sidecar, template, tool, turn,
};
use crate::support::Dir;

struct NoEvents;

impl RelayEvents for NoEvents {
    fn event(&self, _event: RelayEvent) {}
}

/// The fake connector, its handle and the app's relays.
struct Connector {
    fake: FakeHandle,
    _scratch: ScratchDir,
    _connectors: Connectors,
    handle: ConnectorHandle,
    relays: Relays,
}

impl Connector {
    /// The connector, `signed_in` with an API key the fake accepts or not added.
    fn new(signed_in: bool) -> Self {
        let fake = FakeConnector::start(FakeOptions::default()).unwrap();
        let scratch = ScratchDir::new("host-case-law").unwrap();
        let config = fake.config(scratch.state_file());
        let origin = config.origin.clone();
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
        let connectors = Connectors::new(config, store).unwrap();
        let handle = connectors.get("case-law").unwrap();
        if signed_in {
            handle.add_api_key(&fake.new_api_key()).unwrap();
        }
        let relays = Relays::new(RelayConfig {
            connector: handle.clone(),
            origin,
            day_file: scratch.path().join("connectors").join("day.json"),
            clock: Arc::new(SystemClock),
            local_day: Arc::new(FixedOffset(19_800)),
            events: Arc::new(NoEvents),
        });
        Self {
            fake,
            _scratch: scratch,
            _connectors: connectors,
            handle,
            relays,
        }
    }
}

/// The instructions the hook sends both agents: R21's part stands in, then `{connectors}`.
const INSTRUCTIONS: &str = "Tools.{connectors}";

/// What they become in a session with the entry, and in one without.
fn instructions(with_case_law: bool) -> String {
    format!(
        "Tools.{}",
        clauseo_profile::instructions::connectors_part(with_case_law)
    )
}

/// The `case-law` entry as `clauseo_profile::servers::case_law` builds it.
fn case_law_server() -> McpServer {
    McpServer::Stdio {
        name: "case-law".to_owned(),
        command: sidecar(),
        args: vec!["connector".to_owned(), "case-law".to_owned()],
        env: vec![
            (
                "CLAUSEO_RELAY".to_owned(),
                "127.0.0.1:{relay_port}".to_owned(),
            ),
            (
                "CLAUSEO_RELAY_TOKEN_FILE".to_owned(),
                "{relay_token_file}".to_owned(),
            ),
        ],
    }
}

/// The entry `session/new` carries once filled: the port differs per run, so it matches any
/// value; the token file is the session's own. On Windows it also carries `SystemRoot`.
fn filled_case_law() -> Value {
    let mut env = vec![
        json!({"name": "CLAUSEO_RELAY", "value": "{{any}}"}),
        json!({"name": "CLAUSEO_RELAY_TOKEN_FILE",
            "value": format!("@RUN@{MAIN_SEPARATOR}relay-token")}),
    ];
    if cfg!(windows) {
        env.push(json!({"name": "SystemRoot", "value": "{{any}}"}));
    }
    json!({"name": "case-law", "command": "{{any}}", "args": ["connector", "case-law"],
        "env": env})
}

fn hook() -> Hook {
    Hook {
        meta: json!({"systemPrompt": {"append": INSTRUCTIONS}}),
        codex: json!({"developer_instructions": INSTRUCTIONS}),
        servers: vec![browser_server(), case_law_server()],
        ..Hook::default()
    }
}

/// The hook, the proxy and Chrome the browser entry needs, and the relays when given.
fn wire(config: HostConfig, relays: Option<&Relays>) -> HostConfig {
    let config = config
        .with_profile_hook(Box::new(hook()))
        .with_egress_proxy(Arc::new(TestProxy(8123)))
        .with_browser_executable(Some(PathBuf::from("/chrome")));
    match relays {
        Some(relays) => config.with_case_law_relays(relays.clone()),
        None => config,
    }
}

/// The lawyer's one server, and the settings that turn it and the browser on, with web search
/// off (so Claude's `allowedTools` holds only what case law adds) and `extra` over them.
fn lawyer(dir: &Dir, extra: &Value) -> (Value, Value) {
    let bin = dir.folder("bin", &[]);
    let mut tools = json!({"browser": true, "web_search": false});
    if let Some(Value::Object(more)) = extra.get("tools") {
        tools.as_object_mut().unwrap().extend(more.clone());
    }
    let settings = json!({"tools": tools, "mcp": {"servers": {
        "alpha": {"command": bin.join("alpha").display().to_string(), "enabled": true}}}});
    let entry = json!({"name": "alpha", "command": "{{any}}", "args": [], "env": []});
    (settings, entry)
}

/// Claude's `session/new` or `session/load` params with `servers` and, when it got case law,
/// the pre-allow and the paragraph.
fn claude_params(servers: &[Value], with_case_law: bool) -> Value {
    let allowed: Vec<&str> = if with_case_law {
        vec!["mcp__case-law"]
    } else {
        Vec::new()
    };
    json!({"mcpServers": servers, "_meta": {
        "systemPrompt": {"append": instructions(with_case_law)},
        "claudeCode": {"options": {"allowedTools": allowed}}}})
}

/// The relay token file of `seen`'s session, if it holds one.
fn token_of(seen: &Seen) -> Option<String> {
    let file = seen.run.as_ref()?.join("relay-token");
    std::fs::read_to_string(file)
        .ok()
        .map(|text| text.trim_end().to_owned())
}

fn case_law(reason: Option<ToolsUnavailableReason>) -> ToolState {
    tool(ClauseoTool::CaseLaw, reason)
}

/// The done-when's profile entry: both agents get `case-law` after the lawyer's server and
/// `browser`, on `session/new` and then `session/load`; the two sessions have their own token
/// files and tokens, which their stderr redactors hold and which are gone once the host stops;
/// Claude pre-allows `mcp__case-law`; both agents' instructions hold the paragraph.
#[test]
fn both_agents_get_case_law_after_the_other_servers() {
    let connector = Connector::new(true);
    let dir = Dir::new("case_law_both", &[]);
    let (mcp, alpha) = lawyer(&dir, &json!({}));
    let servers = [alpha, filled_browser(), filled_case_law()];
    let claude = template(
        &dir,
        "cl-claude",
        &agent_lines(
            "cl-claude",
            "session/new",
            &claude_params(&servers, true),
            &[],
        ),
    );
    let codex = template(
        &dir,
        "cl-codex",
        &codex_lines("cl-codex", &json!({"mcpServers": servers})),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({})), (CODEX, &codex, json!({}))]),
        mcp.clone(),
    );
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        wire(config, Some(&connector.relays))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    turn(&host, host.create_thread(matter, CODEX));
    assert_eq!(
        first_frame_tools(&host, thread),
        [
            tool(ClauseoTool::WebSearch, Some(ToolsUnavailableReason::Off)),
            tool(ClauseoTool::Browser, None),
            case_law(None),
        ]
    );
    let seen = launcher.seen();
    let tokens: Vec<String> = seen.iter().filter_map(token_of).collect();
    assert_eq!(tokens.len(), 2, "one token file per session");
    assert_ne!(tokens[0], tokens[1]);
    for (seen, token) in seen.iter().zip(&tokens) {
        assert_eq!(
            seen.redacted,
            std::slice::from_ref(token),
            "the stderr tail hides the token"
        );
        assert!(
            !seen
                .env
                .values()
                .any(|value| value.to_string_lossy().contains(token.as_str())),
            "the token is in the agent's environment"
        );
    }
    let config = seen
        .iter()
        .find_map(Seen::codex_config)
        .expect("a Codex launch");
    assert_eq!(config["developer_instructions"], json!(instructions(true)));
    host.shutdown();
    for seen in &seen {
        let file = seen.run.as_ref().unwrap().join("relay-token");
        assert!(!file.exists(), "{} stays", file.display());
    }

    let load = template(
        &dir,
        "cl-load",
        &agent_lines(
            "cl-load",
            "session/load",
            &json!({"sessionId": "sess-1", "mcpServers": servers,
                "_meta": claude_params(&servers, true)["_meta"]}),
            &[],
        ),
    );
    settings(&dir, agents(&[(CLAUDE, &load, json!({}))]), mcp);
    let host = dir.start_launching(1, launcher, |config| wire(config, Some(&connector.relays)));
    turn(&host, thread);
    host.shutdown();
}

/// One Claude session under `extra` settings (and `policy`, `managed`), with or without the
/// relays: what it got matches `servers` with no case law, and the header shows `expected`
/// for case law (none: nothing shows). No token file is written.
fn no_case_law(
    test: &str,
    connector: &Connector,
    with_relays: bool,
    (extra, policy, managed): (Value, Option<&str>, bool),
    expected: Option<&ToolState>,
) {
    let dir = Dir::new(test, &[]);
    if let Some(policy) = policy {
        dir.set_policy(policy);
    }
    let (mcp, alpha) = lawyer(&dir, &extra);
    let servers = if managed {
        Vec::new()
    } else {
        vec![alpha, filled_browser()]
    };
    let claude = template(
        &dir,
        "nc",
        &agent_lines("nc", "session/new", &claude_params(&servers, false), &[]),
    );
    settings(&dir, agents(&[(CLAUDE, &claude, json!({}))]), mcp);
    if managed {
        dir.set_managed_mcp();
    }
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        wire(config, with_relays.then_some(&connector.relays))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    let tools = first_frame_tools(&host, thread);
    let shown: Vec<&ToolState> = tools
        .iter()
        .filter(|state| state.tool == ClauseoTool::CaseLaw)
        .collect();
    assert_eq!(shown, expected.into_iter().collect::<Vec<_>>(), "{test}");
    let seen = launcher.seen();
    assert!(seen.iter().all(|seen| token_of(seen).is_none()), "{test}");
    assert!(seen.iter().all(|seen| seen.redacted.is_empty()), "{test}");
    host.shutdown();
}

/// Every way to no entry and no relay: not added, signed out, the switch off (the lawyer's or
/// the firm's), no relays, and the firm's `managed-mcp.json`.
#[test]
fn no_entry_and_no_relay_unless_every_condition_holds() {
    let not_added = Connector::new(false);
    let plain = || (json!({}), None, false);
    no_case_law("case_law_not_added", &not_added, true, plain(), None);
    let signed_out = Connector::new(true);
    signed_out.handle.sign_out().unwrap();
    no_case_law("case_law_signed_out", &signed_out, true, plain(), None);
    let signed_in = Connector::new(true);
    no_case_law("case_law_no_relays", &signed_in, false, plain(), None);
    no_case_law(
        "case_law_switch_off",
        &signed_in,
        true,
        (json!({"tools": {"connectors": false}}), None, false),
        Some(&case_law(Some(ToolsUnavailableReason::Off))),
    );
    // Policy's `false` locks it off. (An unreadable policy source locks it off too, and the
    // browser with it: `clauseo-settings` tests that refusal.)
    no_case_law(
        "case_law_policy_off",
        &signed_in,
        true,
        (
            json!({}),
            Some(r#"{"tools": {"connectors": false}}"#),
            false,
        ),
        Some(&case_law(Some(ToolsUnavailableReason::LockedOff))),
    );
    no_case_law(
        "case_law_managed",
        &signed_in,
        true,
        (json!({}), None, true),
        Some(&case_law(Some(ToolsUnavailableReason::ManagedByFirm))),
    );
    // Nothing reached the connector: no session sent it a call.
    assert_eq!(signed_in.fake.requests_to("/mcp/case-law").len(), 0);
}

const OK: &str = r#"{"clauseo_relay":1,"ok":true}"#;
const CLOSED: &str = r#"{"clauseo_relay":1,"ok":false,"error":"closed"}"#;

/// A connection to the relay at `addr` that sent the hello with `token`, and the answer line.
fn hello(addr: SocketAddr, token: &str) -> (BufReader<TcpStream>, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    // A bound on the answer, so a relay that never answers fails the test instead of hanging it
    // (and leaving a stopped agent's ladder, held on the manual clock, behind).
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let line = format!(r#"{{"clauseo_relay":1,"token":"{token}","connector":"case-law"}}"#);
    stream.write_all(format!("{line}\n").as_bytes()).unwrap();
    let mut reader = BufReader::new(stream);
    let mut answer = String::new();
    reader.read_line(&mut answer).unwrap();
    (reader, answer.trim_end().to_owned())
}

/// What the run-removal hook saw: whether the token file was still there, and the answer a
/// hello with its token got from the session's relay.
type Removal = (bool, String);

/// A hook that, just before a run folder goes, reads its token file and says hello with it.
fn removal_hook(relays: &Relays, seen: mpsc::Sender<Removal>) -> Arc<dyn Fn(&Path) + Send + Sync> {
    let relays = relays.clone();
    let seen = Mutex::new(seen);
    Arc::new(move |root: &Path| {
        let file = root.join("relay-token");
        let Ok(text) = std::fs::read_to_string(&file) else {
            seen.lock().unwrap().send((false, String::new())).unwrap();
            return;
        };
        let addr = relays.opened_for_test()[0].1;
        let (_conn, answer) = hello(addr, text.trim_end());
        seen.lock().unwrap().send((true, answer)).unwrap();
    })
}

/// One Claude session with case law, whose agent's scenario is `lines` (from
/// [`claude_params`]'s session), with the run-removal hook reporting to the returned receiver.
fn one_session(
    dir: &Dir,
    connector: &Connector,
    adjust: impl FnOnce(String) -> String,
) -> (crate::support::Running, Arc<Wired>, mpsc::Receiver<Removal>) {
    let (mcp, alpha) = lawyer(dir, &json!({}));
    let servers = [alpha, filled_browser(), filled_case_law()];
    let lines = agent_lines("cl-end", "session/new", &claude_params(&servers, true), &[]);
    let claude = template(dir, "cl-end", &adjust(lines));
    settings(dir, agents(&[(CLAUDE, &claude, json!({}))]), mcp);
    let (sender, removals) = mpsc::channel();
    let hook = removal_hook(&connector.relays, sender);
    let launcher = Wired::new(dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        wire(config, Some(&connector.relays)).with_run_removal_hook(hook)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    (host, launcher, removals)
}

/// Decision D3 through the actor: once the agent is being stopped (here by the shutdown, its
/// stop ladder held on the manual clock), the relay refuses a new hello with `closed` and has
/// shut the connection that was open; the token is revoked before its file goes, and the file is
/// gone once the run folder is removed.
#[test]
fn the_relay_refuses_once_the_agent_is_stopped_and_the_file_goes_after() {
    let connector = Connector::new(true);
    let dir = Dir::new("case_law_stop", &[]);
    let (host, launcher, removals) = one_session(&dir, &connector, |lines| {
        // Held by the ladder on the manual clock; the agent ends itself within 15 s even when
        // the test is killed from outside.
        lines.replace(
            r#""on_stdin_close":"exit""#,
            r#""on_stdin_close":"hang_briefly""#,
        )
    });
    let file = launcher.seen()[0].run.clone().unwrap().join("relay-token");
    let token = std::fs::read_to_string(&file)
        .unwrap()
        .trim_end()
        .to_owned();
    let addr = connector.relays.opened_for_test()[0].1;
    let (mut open, answer) = hello(addr, &token);
    assert_eq!(answer, OK);
    let probe_file = file.clone();
    let (again, shut, kept) = crate::support::shutdown_stopping(host, move || {
        let (_new, again) = hello(addr, &token);
        let mut rest = String::new();
        let shut = match open.read_line(&mut rest) {
            Ok(read) => read == 0,
            Err(error) => !matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
        };
        (again, shut, probe_file.exists())
    });
    assert_eq!(again, CLOSED, "a new hello once the agent is being stopped");
    assert!(shut, "the open connection was not shut");
    assert!(kept, "the token file went before the agent ended");
    let removal = removals.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(removal, (true, CLOSED.to_owned()));
    assert!(
        !file.exists(),
        "the token file stays after the run folder's removal"
    );
}

/// An agent that exits on its own is never stopped: the run folder's removal itself revokes
/// the token before the file goes.
#[test]
fn an_agent_that_exits_has_its_token_revoked_before_its_file_goes() {
    let connector = Connector::new(true);
    let dir = Dir::new("case_law_exit", &[]);
    let (host, launcher, removals) = one_session(&dir, &connector, |lines| {
        format!("{lines}\n{}", json!({"exit": {"code": 0}}))
    });
    let removal = removals.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(removal, (true, CLOSED.to_owned()));
    host.shutdown();
    let file = launcher.seen()[0].run.clone().unwrap().join("relay-token");
    assert!(!file.exists());
}

/// Launches through [`Wired`], writing the firm's `managed-mcp.json` as it spawns the agent:
/// after `launch_keys` read it, before `session_loads` reads it again.
struct ManagedOnLaunch {
    inner: Arc<Wired>,
    file: PathBuf,
}

impl clauseo_process::Launcher for ManagedOnLaunch {
    fn launch(
        &self,
        spec: &clauseo_process::LaunchSpec,
    ) -> Result<clauseo_process::Launched, clauseo_process::Error> {
        crate::write::write(&self.file, b"{\"mcpServers\": {}}");
        self.inner.launch(spec)
    }
}

/// The window `launch_keys`' recheck leaves: a `managed-mcp.json` that appears while the agent
/// starts. It fails closed: no server goes, the header says `ManagedByFirm` and the relay
/// refuses its token. Codex's instructions, fixed before the spawn, keep the paragraph (the
/// documented wrong words).
#[test]
fn managed_mcp_that_appears_during_the_launch_fails_closed() {
    let connector = Connector::new(true);
    let dir = Dir::new("case_law_managed_late", &[]);
    let (mcp, _alpha) = lawyer(&dir, &json!({}));
    let codex = template(
        &dir,
        "cl-late",
        &codex_lines("cl-late", &json!({"mcpServers": []})),
    );
    settings(&dir, agents(&[(CODEX, &codex, json!({}))]), mcp);
    let wired = Wired::new(&dir);
    let launcher = Arc::new(ManagedOnLaunch {
        inner: Arc::clone(&wired),
        file: dir.data().parent().unwrap().join("managed-mcp.json"),
    });
    let host = dir.start_launching(0, launcher, |config| wire(config, Some(&connector.relays)));
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CODEX);
    turn(&host, thread);
    let tools = first_frame_tools(&host, thread);
    assert!(
        tools.contains(&case_law(Some(ToolsUnavailableReason::ManagedByFirm))),
        "{tools:?}"
    );
    let seen = wired.seen();
    let token = token_of(&seen[0]).expect("the prep opened the relay");
    let addr = connector.relays.opened_for_test()[0].1;
    assert_eq!(hello(addr, &token).1, CLOSED, "the relay was not ended");
    let config = seen[0].codex_config().expect("a Codex launch");
    assert_eq!(config["developer_instructions"], json!(instructions(true)));
    host.shutdown();
}
