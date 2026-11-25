//! A remote MCP server from Settings › Tools at a launch: sent as an http server, its keychain
//! header filled, to an agent whose `initialize` advertises `mcpCapabilities.http`; left out,
//! by name in the log, for one that does not. No header value reaches a log.
//!
//! Each test installs the log file, the process's global subscriber: nextest runs each test in
//! a process of its own.

use clauseo_host::log_file;
use clauseo_settings::Locations;
use serde_json::{Value, json};

use crate::mcp_tools::{finds_npx, npx, read_logs, store, wired};
use crate::profile_rig::{agent_lines, template, turn};
use crate::support::{Dir, FAKE, Running, Scenario};
use crate::write;

/// The header value the store holds for `far/header/Authorization`: plain words, distinctive
/// enough that finding it in a log means a leak.
const HEADER: &str = "Bearer saffron-gondola-hillock-ember";

/// What the host logs for a remote server an agent cannot take (`clauseo-acp`'s driver).
const LEFT_OUT: &str =
    "left out an http MCP server: the agent did not advertise mcpCapabilities.http";

/// The lawyer's remote server `far`, turned on, its `Authorization` header in the keychain.
fn servers() -> Value {
    json!({"far": {"url": "https://mcp.example.com/mcp",
        "headers": {"Authorization": {"keychain": true}}, "enabled": true}})
}

/// A one-agent scenario whose `session/new` carries exactly `servers`; its `initialize`
/// answer advertises `mcpCapabilities.http` when `http`.
fn scenario(dir: &Dir, name: &str, servers: &Value, http: bool) -> Scenario {
    let params = json!({"cwd": "{{any}}", "mcpServers": servers});
    let mut lines = agent_lines(name, "session/new", &params, &[]);
    if http {
        let plain = r#""agentCapabilities":{"loadSession":true}"#;
        assert!(lines.contains(plain), "the rig's initialize answer changed");
        lines = lines.replace(
            plain,
            r#""agentCapabilities":{"loadSession":true,"mcpCapabilities":{"http":true}}"#,
        );
    }
    template(dir, name, &lines)
}

/// Starts the host on `scenario` with [`servers`] and the header in the keychain double, runs
/// one turn (the scenario checks `session/new`), and gives back the log.
fn launch(dir: &Dir, scenario: Scenario) -> String {
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");
    dir.set_settings(&[(FAKE, scenario)], json!({"mcp": {"servers": servers()}}));
    let store = store(&[("far/header/Authorization", HEADER)]);
    let npx = npx(dir);
    let host: Running = dir.start_with(0, |config| wired(config, &store, finds_npx(&npx)));
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, FAKE));
    host.shutdown();
    drop(guard);
    let log = read_logs(&locations.log_dir());
    assert!(
        log.contains("session attached"),
        "the log holds no launch:\n{log}"
    );
    assert!(!log.contains(HEADER), "the log holds the header value");
    assert!(
        !log.contains("saffron-gondola"),
        "the log holds the header value"
    );
    log
}

/// An agent that takes remote servers gets `far` as an http server, its header filled from
/// the keychain; nothing is left out.
#[test]
fn mcp_remote_server_reaches_an_agent_that_takes_http() {
    let dir = Dir::new("mcp_remote_sent", &[]);
    let sent = json!([{"type": "http", "name": "far", "url": "https://mcp.example.com/mcp",
        "headers": [{"name": "Authorization", "value": HEADER}]}]);
    let log = launch(&dir, scenario(&dir, "mcp-remote-sent", &sent, true));
    assert!(!log.contains(LEFT_OUT), "a server was left out:\n{log}");
}

/// An agent that does not advertise `mcpCapabilities.http` gets no server, and the log names
/// the one left out.
#[test]
fn mcp_remote_server_is_left_out_without_the_capability() {
    let dir = Dir::new("mcp_remote_left_out", &[]);
    let log = launch(
        &dir,
        scenario(&dir, "mcp-remote-left-out", &json!([]), false),
    );
    let line = log
        .lines()
        .find(|line| line.contains(LEFT_OUT))
        .unwrap_or_else(|| panic!("no left-out line:\n{log}"));
    assert!(
        line.contains("far"),
        "the line does not name the server: {line}"
    );
}
