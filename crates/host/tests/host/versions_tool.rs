//! The version-history tool at the session, through the host's real source
//! (`versions::source::HostSource`): the `clauseo-versions` entry in both agents' `session/new`
//! with Claude's pre-allow; the session's own listener and token file, reached as
//! `clauseo-mcp connector clauseo-versions` reaches it; each session reading only its own
//! matter's history; the token revoked before its file goes; and no entry, no listener and no
//! note line under the firm's `managed-mcp.json`.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{MAIN_SEPARATOR, Path};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use clauseo_host::connectors::versions_opened_for_test;
use clauseo_host::{HostConfig, McpServer};
use clauseo_protocol::{MatterId, StopReason, StreamId, ThreadId};
use serde_json::{Value, json};

use crate::profile_rig::{
    CLAUDE, CODEX, Hook, Wired, agent_lines, agents, codex_lines, settings, sidecar, template, turn,
};
use crate::support::Dir;
use crate::undo_note_restart::{NEXT, answered, edits, note, run_prompt, undo};

/// The entry as `clauseo_profile::servers::versions` builds it.
fn versions_server() -> McpServer {
    McpServer::Stdio {
        name: "clauseo-versions".to_owned(),
        command: sidecar(),
        args: vec!["connector".to_owned(), "clauseo-versions".to_owned()],
        env: vec![
            (
                "CLAUSEO_RELAY".to_owned(),
                "127.0.0.1:{versions_port}".to_owned(),
            ),
            (
                "CLAUSEO_RELAY_TOKEN_FILE".to_owned(),
                "{versions_token_file}".to_owned(),
            ),
        ],
    }
}

/// The entry `session/new` carries once filled: any port, the session's own token file, and on
/// Windows the variables the `case-law` entry gets too.
fn filled_versions() -> Value {
    let mut env = vec![
        json!({"name": "CLAUSEO_RELAY", "value": "{{any}}"}),
        json!({"name": "CLAUSEO_RELAY_TOKEN_FILE",
            "value": format!("@RUN@{MAIN_SEPARATOR}versions-token")}),
    ];
    if cfg!(windows) {
        env.push(json!({"name": "SystemRoot", "value": "{{any}}"}));
        env.push(json!({"name": "windir", "value": "{{any}}"}));
        for name in ["TEMP", "TMP", "PATHEXT"] {
            if std::env::var(name).is_ok() {
                env.push(json!({"name": name, "value": "{{any}}"}));
            }
        }
    }
    json!({"name": "clauseo-versions", "command": "{{any}}",
        "args": ["connector", "clauseo-versions"], "env": env})
}

fn wire(config: HostConfig) -> HostConfig {
    config.with_profile_hook(Box::new(Hook {
        codex: json!({"developer_instructions": "Tools."}),
        servers: vec![versions_server()],
        ..Hook::default()
    }))
}

/// Claude's `session/new` params: the entry and the two pre-allow rules, or neither.
fn claude_params(with_tool: bool) -> Value {
    let (servers, allowed) = if with_tool {
        (
            vec![filled_versions()],
            json!([
                "mcp__clauseo-versions__list_versions",
                "mcp__clauseo-versions__read_version"
            ]),
        )
    } else {
        (Vec::new(), json!([]))
    };
    json!({"mcpServers": servers,
        "_meta": {"claudeCode": {"options": {"allowedTools": allowed}}}})
}

/// Web search and the browser off, so Claude's `allowedTools` holds only the tool's rules.
fn quiet() -> Value {
    json!({"tools": {"web_search": false, "browser": false}})
}

/// The address of the listener of `matter`'s `thread`.
fn listener(matter: MatterId, thread: ThreadId) -> SocketAddr {
    versions_opened_for_test()
        .into_iter()
        .find(|(key, _)| key.matter == matter && key.thread == thread)
        .map(|(_, addr)| addr)
        .expect("a version-history listener for the session")
}

fn token_in(run: &Path) -> Option<String> {
    std::fs::read_to_string(run.join("versions-token"))
        .ok()
        .map(|text| text.trim_end().to_owned())
}

/// The agent's side, as `clauseo-mcp connector clauseo-versions` speaks to the listener.
struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Client {
    /// Connects and says hello with `token`; the client and the answer line.
    fn hello(addr: SocketAddr, token: &str) -> (Client, String) {
        let writer = TcpStream::connect(addr).unwrap();
        writer
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let mut client = Client {
            reader: BufReader::new(writer.try_clone().unwrap()),
            writer,
        };
        let line =
            format!(r#"{{"clauseo_relay":1,"token":"{token}","connector":"clauseo-versions"}}"#);
        client.send(&line);
        let mut answer = String::new();
        if client.reader.read_line(&mut answer).is_err() {
            answer.clear();
        }
        (client, answer.trim_end().to_owned())
    }

    fn send(&mut self, line: &str) {
        let _sent = self
            .writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.write_all(b"\n"));
    }

    fn ask(&mut self, id: u64, method: &str, params: &Value) -> Value {
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.send(&request.to_string());
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    fn call(&mut self, id: u64, tool: &str, arguments: &Value) -> Value {
        self.ask(
            id,
            "tools/call",
            &json!({"name": tool, "arguments": arguments}),
        )["result"]
            .clone()
    }
}

const OK: &str = r#"{"clauseo_relay":1,"ok":true}"#;
const REFUSED: &str = r#"{"clauseo_relay":1,"ok":false,"error":"bad_token"}"#;

/// The version ids and every file path `list_versions` lists.
fn listed(result: &Value) -> (Vec<String>, Vec<String>) {
    assert_ne!(result["isError"], true, "{result}");
    let versions = result["structuredContent"]["versions"].as_array().unwrap();
    let ids = versions
        .iter()
        .map(|version| version["id"].as_str().unwrap().to_owned())
        .collect();
    let paths = versions
        .iter()
        .flat_map(|version| version["files"].as_array().unwrap())
        .map(|file| file["path"].as_str().unwrap().to_owned())
        .collect();
    (ids, paths)
}

/// What the run-removal hook saw: whether the token file was still there, and the answer a
/// hello with its token got.
type Removal = (bool, String);

/// A hook that, just before a run folder goes, reads its versions token and says hello with it.
fn removal_hook(seen: mpsc::Sender<Removal>) -> Arc<dyn Fn(&Path) + Send + Sync> {
    let seen = Mutex::new(seen);
    Arc::new(move |root: &Path| {
        let Some(token) = token_in(root) else {
            seen.lock().unwrap().send((false, String::new())).unwrap();
            return;
        };
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        let addr = versions_opened_for_test()
            .into_iter()
            .find(|(key, _)| key.id() == name)
            .map(|(_, addr)| addr)
            .unwrap();
        let (_client, answer) = Client::hello(addr, &token);
        seen.lock().unwrap().send((true, answer)).unwrap();
    })
}

/// Both agents get the entry, Claude with one pre-allow rule per tool; each session's listener,
/// reached with its own token file, lists and reads its own matter's history through the host's
/// source and nothing of the other matter's; another session's token is refused; the token is in
/// the stderr redactor, revoked before its file goes, and the file is gone at the end.
#[test]
fn each_agent_reads_only_its_own_matters_history_through_its_own_listener() {
    let dir = Dir::new("versions_tool_both", &[]);
    let claude = template(
        &dir,
        "vt-claude",
        &agent_lines("vt-claude", "session/new", &claude_params(true), &[]),
    );
    let codex = template(
        &dir,
        "vt-codex",
        &codex_lines("vt-codex", &json!({"mcpServers": [filled_versions()]})),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({})), (CODEX, &codex, json!({}))]),
        quiet(),
    );
    let (sender, removals) = mpsc::channel();
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        wire(config).with_run_removal_hook(removal_hook(sender))
    });
    let alpha = host.create_matter("a", &dir.folder("a", &[("alpha.txt", "alpha draft\n")]));
    let beta = host.create_matter("b", &dir.folder("b", &[("beta.txt", "beta memo\n")]));
    let alpha_thread = host.create_thread(alpha, CLAUDE);
    let beta_thread = host.create_thread(beta, CODEX);
    turn(&host, alpha_thread);
    turn(&host, beta_thread);
    let seen = launcher.seen();
    assert_eq!(seen.len(), 2);
    let sessions = [
        (
            alpha,
            alpha_thread,
            "alpha.txt",
            "alpha draft\n",
            "beta.txt",
        ),
        (beta, beta_thread, "beta.txt", "beta memo\n", "alpha.txt"),
    ];
    let mut tokens = Vec::new();
    let mut ids = Vec::new();
    for (seen, (matter, thread, own, content, other)) in seen.iter().zip(sessions) {
        let token = token_in(seen.run.as_ref().unwrap()).expect("the session's token file");
        assert!(seen.redacted.contains(&token), "the stderr tail hides it");
        let (mut client, answer) = Client::hello(listener(matter, thread), &token);
        assert_eq!(answer, OK);
        let init = client.ask(1, "initialize", &json!({"protocolVersion": "2025-06-18"}));
        assert_eq!(init["result"]["serverInfo"]["name"], "clauseo-versions");
        let tools = client.ask(2, "tools/list", &json!({}));
        let names: Vec<&str> = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["list_versions", "read_version"]);
        let (versions, paths) = listed(&client.call(3, "list_versions", &json!({})));
        assert!(paths.iter().any(|path| path == own), "{paths:?}");
        assert!(!paths.iter().any(|path| path == other), "{paths:?}");
        let oldest = versions.last().unwrap().clone();
        let read = client.call(4, "read_version", &json!({"version": oldest, "path": own}));
        assert_eq!(read["structuredContent"]["text"], content, "{read}");
        tokens.push(token);
        ids.push(oldest);
    }
    // A version of the other matter is not in this matter's history, by id or by path.
    let (mut alpha_client, _) = Client::hello(listener(alpha, alpha_thread), &tokens[0]);
    let foreign = alpha_client.call(
        5,
        "read_version",
        &json!({"version": ids[1], "path": "beta.txt"}),
    );
    assert_eq!(foreign["isError"], true, "{foreign}");
    // Each listener takes only its own session's token.
    let (_, crossed) = Client::hello(listener(alpha, alpha_thread), &tokens[1]);
    assert_eq!(crossed, REFUSED);
    let (_, crossed) = Client::hello(listener(beta, beta_thread), &tokens[0]);
    assert_eq!(crossed, REFUSED);
    host.shutdown();
    for _ in 0..2 {
        let removal = removals.recv_timeout(Duration::from_secs(30)).unwrap();
        assert_eq!(
            removal,
            (true, REFUSED.to_owned()),
            "revoked before the file goes"
        );
    }
    for seen in &seen {
        let file = seen.run.as_ref().unwrap().join("versions-token");
        assert!(!file.exists(), "{} stays", file.display());
    }
}

/// One Claude session that edits brief.txt, has the edit undone, and is sent [`NEXT`]: under
/// the firm's `managed-mcp.json` (`managed`) or not. The second prompt's note must be `told`.
fn note_session(name: &str, managed: bool, told: &str) -> Dir {
    let dir = Dir::new(name, &[]);
    let lines = agent_lines(name, "session/new", &claude_params(!managed), &[]);
    let mut steps: Vec<String> = lines.lines().take(5).map(str::to_owned).collect();
    let rest = edits("p1", "brief.txt")
        .into_iter()
        .chain(answered("p2", &[told, NEXT]));
    steps.extend(rest.map(|step| step.to_string()));
    let claude = template(&dir, name, &steps.join("\n"));
    settings(&dir, agents(&[(CLAUDE, &claude, json!({}))]), quiet());
    if managed {
        dir.set_managed_mcp();
    }
    let launcher = Wired::new(&dir);
    let before = versions_opened_for_test().len();
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), wire);
    let matter = host.create_matter("m", &dir.folder("m", &[("brief.txt", "draft\n")]));
    let thread = host.create_thread(matter, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Finish the brief.");
    events.until_turn_ended(first);
    undo(&host, &mut events, thread, first, "brief.txt");
    assert_eq!(
        run_prompt(&host, &mut events, thread, NEXT),
        StopReason::EndTurn
    );
    events.assert_no_mismatch();
    let run = launcher.seen()[0].run.clone().unwrap();
    // Counted, not looked up: ids repeat across the test's two folders.
    let opened = versions_opened_for_test().len() > before;
    assert_eq!((token_in(&run).is_some(), opened), (!managed, !managed));
    host.shutdown();
    dir
}

/// The undo note names the tool when the session got its entry, and under the firm's
/// `managed-mcp.json` the session gets no entry, no listener, no token file and no such line.
#[test]
fn the_note_points_at_the_tool_only_when_its_entry_went() {
    let line = "\nTo see what the files held before, use the list_versions and read_version tools \
        (clauseo-versions): they list this matter's versions and read an earlier version of a \
        file it still holds.";
    let with = format!("{}{line}", note("brief.txt"));
    note_session("versions_tool_note", false, &with);
    note_session("versions_tool_managed", true, &note("brief.txt"));
}
