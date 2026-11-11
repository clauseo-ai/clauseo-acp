//! Settings › Tools at the host's interface (PR A1): the lawyer's own MCP servers.
//!
//! - The launch: a local server's command is found by the resolver, its keychain values are
//!   read from the store and its `PATH` is built from the program's folder and the inherited
//!   `PATH`; a server with a value missing or a program not found is left out, and none goes in
//!   the Safe workspace. Each scenario is written from Rust and checks `session/new` exactly.
//! - The commands: `HostCommand::Mcp` against a `MemoryStore` (never the real keychain), the
//!   settings file and `McpToolsChanged` on the app subscription, and each refusal.
//!
//! The launch test installs the log file, the process's global subscriber: nextest runs each
//! test in a process of its own.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_connectors::MemoryStore;
use clauseo_host::{AppSubscription, HostConfig, log_file};
use clauseo_protocol::{
    ErrorCode, HostCommand, HostEvent, McpCommand, McpControl, McpSecretEdit, McpSecretName,
    McpSecretValue, McpServerEdit, McpServerKind, McpToolsView, RedactedString,
};
use clauseo_sandbox::SetupState;
use clauseo_settings::Locations;
use futures::StreamExt as _;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::profile_rig::{agent_lines, template, turn};
use crate::protection_rig::{Plan, b, rig, subscribe};
use crate::support::{Dir, FAKE, Running, Scenario, lawyer_env, wait};
use crate::write;

/// The value the store holds for `github/env/TOKEN`: plain words, so no scanner takes it for a
/// real credential, and distinctive enough that finding it in a log means a leak.
pub(crate) const HELD: &str = "marmalade-heron-quayside-lantern";

/// The MCP resolver seam's type.
pub(crate) type Resolver = Arc<dyn Fn(&str) -> Option<PathBuf> + Send + Sync>;

/// The `PATH` the host is given on unix.
const INHERITED_PATH: &str = "/usr/bin:/bin";

/// The keychain account of `server`'s variable `name`.
pub(crate) fn account(server: &str, name: &str) -> String {
    format!("{server}/env/{name}")
}

/// A store holding `items`.
pub(crate) fn store(items: &[(&str, &str)]) -> Arc<MemoryStore> {
    let store = Arc::new(MemoryStore::new());
    for (account, value) in items {
        store.put(account, value.as_bytes());
    }
    store
}

/// A resolver that finds `npx` at `npx` and nothing else.
pub(crate) fn finds_npx(npx: &Path) -> Resolver {
    let npx = npx.to_owned();
    Arc::new(move |typed: &str| (typed == "npx").then(|| npx.clone()))
}

/// The config with `store`, `resolver` and, on unix, the inherited `PATH` fixed.
pub(crate) fn wired(
    config: HostConfig,
    store: &Arc<MemoryStore>,
    resolver: Resolver,
) -> HostConfig {
    let config = config
        .with_mcp_secrets(Arc::<MemoryStore>::clone(store))
        .with_mcp_resolver(resolver);
    if cfg!(unix) {
        config.with_inherited_env(vec![(
            OsString::from("PATH"),
            OsString::from(INHERITED_PATH),
        )])
    } else {
        config
    }
}

/// `<work>/bin/npx`, made as an empty file; nothing launches it.
pub(crate) fn npx(dir: &Dir) -> PathBuf {
    dir.folder("bin", &[("npx", "")]).join("npx")
}

/// The `env` a server whose program is `command` gets: `own`, then its `PATH` (the program's
/// folder, then the inherited one) exactly on unix, and as `lawyer_env` has it on Windows.
fn server_env(command: &Path, own: Vec<Value>) -> Value {
    if cfg!(unix) {
        let folder = command.parent().unwrap().display().to_string();
        let mut env = own;
        env.push(json!({"name": "PATH", "value": format!("{folder}:{INHERITED_PATH}")}));
        Value::Array(env)
    } else {
        lawyer_env(own)
    }
}

/// A one-agent scenario whose `session/new` carries exactly `servers`.
fn scenario(dir: &Dir, name: &str, servers: &Value) -> Scenario {
    let params = json!({"cwd": "{{any}}", "mcpServers": servers});
    template(dir, name, &agent_lines(name, "session/new", &params, &[]))
}

/// Writes the settings: `FAKE` playing `scenario`, and `servers` as `mcp.servers`.
fn settings(dir: &Dir, scenario: Scenario, servers: &Value) {
    dir.set_settings(&[(FAKE, scenario)], json!({"mcp": {"servers": servers}}));
}

/// One turn on a new thread of `FAKE`: the scenario accepted `session/new`.
fn one_turn(dir: &Dir, host: &Running) {
    let matter = host.create_matter("m", &dir.folder("m", &[("brief.txt", "Clause 4\n")]));
    turn(host, host.create_thread(matter, FAKE));
}

/// The next `McpToolsChanged` on `sub` that `wanted` accepts, skipping every other event.
fn view_until(sub: &mut AppSubscription, wanted: impl Fn(&McpToolsView) -> bool) -> McpToolsView {
    wait("an McpToolsChanged", async {
        loop {
            let item = sub.next().await.expect("the app stream ended");
            if let HostEvent::McpToolsChanged(view) = crate::settings::decode(&item)
                && wanted(&view)
            {
                return view;
            }
        }
    })
}

/// The settings file as JSON.
fn settings_file(dir: &Dir) -> Value {
    serde_json::from_str(&dir.settings_json()).unwrap()
}

/// A variable set to `value`.
pub(crate) fn set(name: &str, value: &str) -> McpSecretEdit {
    McpSecretEdit {
        name: name.to_owned(),
        value: McpSecretValue::Set(RedactedString::new(Zeroizing::new(value.to_owned())).unwrap()),
    }
}

/// A variable keeping its held value.
pub(crate) fn keep(name: &str) -> McpSecretEdit {
    McpSecretEdit {
        name: name.to_owned(),
        value: McpSecretValue::Keep {},
    }
}

/// A local server running `command` with `env`, enabled.
pub(crate) fn local(name: &str, command: &str, env: Vec<McpSecretEdit>) -> McpServerEdit {
    McpServerEdit {
        name: name.to_owned(),
        kind: McpServerKind::Local,
        command: Some(command.to_owned()),
        args: vec!["-y".to_owned(), "server-github".to_owned()],
        url: None,
        env,
        headers: Vec::new(),
        enabled: true,
    }
}

/// A remote server at `url`, enabled.
fn remote(name: &str, url: &str) -> McpServerEdit {
    McpServerEdit {
        name: name.to_owned(),
        kind: McpServerKind::Remote,
        command: None,
        args: Vec::new(),
        url: Some(url.to_owned()),
        env: Vec::new(),
        headers: Vec::new(),
        enabled: true,
    }
}

pub(crate) fn save(previous: Option<&str>, server: McpServerEdit) -> HostCommand {
    HostCommand::Mcp(McpCommand::SaveServer {
        previous_name: previous.map(str::to_owned),
        server,
    })
}

fn remove(name: &str) -> HostCommand {
    HostCommand::Mcp(McpCommand::RemoveServer {
        name: name.to_owned(),
    })
}

fn set_enabled(name: &str, enabled: bool) -> HostCommand {
    HostCommand::Mcp(McpCommand::SetServerEnabled {
        name: name.to_owned(),
        enabled,
    })
}

/// The log files under `dir`, oldest first.
pub(crate) fn read_logs(dir: &Path) -> String {
    let mut text = String::new();
    for name in ["host.2.log", "host.1.log", "host.log"] {
        text.push_str(&std::fs::read_to_string(dir.join(name)).unwrap_or_default());
    }
    text
}

/// A local server's resolved command, its keychain and plain values and its `PATH` reach
/// `session/new`, and the keychain value reaches no log file.
#[test]
fn mcp_tools_launch_sends_the_resolved_server_and_logs_no_secret() {
    let dir = Dir::new("mcp_tools_launch", &[]);
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");

    let npx = npx(&dir);
    let own = vec![
        json!({"name": "REGION", "value": "uk"}),
        json!({"name": "TOKEN", "value": HELD}),
    ];
    let servers = json!([{"name": "github", "command": npx.display().to_string(),
        "args": ["-y", "server-github"], "env": server_env(&npx, own)}]);
    let scenario = scenario(&dir, "mcp-tools-launch", &servers);
    settings(
        &dir,
        scenario,
        &json!({"github": {"command": "npx", "args": ["-y", "server-github"],
            "env": {"TOKEN": {"keychain": true}, "REGION": "uk"}, "enabled": true}}),
    );
    let store = store(&[(&account("github", "TOKEN"), HELD)]);
    let host = dir.start_with(0, |config| wired(config, &store, finds_npx(&npx)));
    one_turn(&dir, &host);
    host.shutdown();
    drop(guard);

    let log = read_logs(&locations.log_dir());
    assert!(
        log.contains("session attached"),
        "the log holds no launch:\n{log}"
    );
    assert!(!log.contains(HELD), "the log holds the keychain value");
}

/// A server whose keychain value is missing is left out and the view then shows it unsaved; a
/// server whose program is not found is left out too; the server that has both goes.
#[test]
fn mcp_tools_launch_leaves_out_a_missing_value_and_an_unfound_command() {
    let dir = Dir::new("mcp_tools_left_out", &[]);
    let npx = npx(&dir);
    let servers = json!([{"name": "plain", "command": npx.display().to_string(),
        "args": [], "env": server_env(&npx, Vec::new())}]);
    let scenario = scenario(&dir, "mcp-tools-left-out", &servers);
    settings(
        &dir,
        scenario,
        &json!({
            "github": {"command": "npx", "env": {"TOKEN": {"keychain": true}}, "enabled": true},
            "ghost": {"command": "nowhere", "enabled": true},
            "plain": {"command": "npx", "enabled": true},
        }),
    );
    let store = store(&[]);
    let host = dir.start_with(0, |config| wired(config, &store, finds_npx(&npx)));
    let mut sub = subscribe(&host);
    let first = view_until(&mut sub, |_| true);
    let github = |view: &McpToolsView| {
        view.servers
            .iter()
            .find(|server| server.name == "github")
            .map(|server| server.env.clone())
    };
    assert_eq!(
        github(&first),
        Some(vec![McpSecretName {
            name: "TOKEN".to_owned(),
            saved: true,
        }]),
        "before a launch the settings' keychain value counts as saved"
    );
    one_turn(&dir, &host);
    let after = view_until(&mut sub, |view| {
        github(view).is_some_and(|env| env.iter().any(|var| !var.saved))
    });
    assert_eq!(
        github(&after),
        Some(vec![McpSecretName {
            name: "TOKEN".to_owned(),
            saved: false,
        }])
    );
    assert_eq!(after.control, McpControl::Lawyer);
    host.shutdown();
}

/// With a sandbox wired that finds this computer protected (the Safe workspace), no lawyer
/// server is sent.
#[test]
fn mcp_tools_launch_sends_no_lawyer_server_in_the_safe_workspace() {
    let dir = Dir::new("mcp_tools_safe", &[]);
    let npx = npx(&dir);
    let scenario = scenario(&dir, "mcp-tools-safe", &json!([]));
    settings(
        &dir,
        scenario,
        &json!({"plain": {"command": "npx", "env": {"REGION": "uk"}, "enabled": true}}),
    );
    let store = store(&[]);
    let sandbox = rig(Plan::new(vec![b()], vec![SetupState::Ready])).sandbox;
    let host = dir.start_with(0, |config| {
        wired(config, &store, finds_npx(&npx)).with_sandbox(sandbox)
    });
    assert!(
        matches!(
            crate::protection_rig::view(&host).computer,
            clauseo_protocol::Protection::Protected { .. }
        ),
        "the computer is not protected"
    );
    one_turn(&dir, &host);
    host.shutdown();
}

/// Add with a value, rename keeping it, turn off, remove: the store, the settings file and the
/// view follow each.
#[test]
fn mcp_tools_commands_add_rename_toggle_and_remove() {
    let dir = Dir::new("mcp_tools_commands", &[]);
    let store = store(&[]);
    let host = dir.start_with(0, |config| {
        config.with_mcp_secrets(Arc::<MemoryStore>::clone(&store))
    });
    // The lawyer's empty list is what a window starts with: the catch-up leaves it out.
    let mut sub = subscribe(&host);

    // Add, with a value set: the store holds it, the file only its marker.
    host.ok(save(
        None,
        local(
            "github",
            "npx",
            vec![set("TOKEN", HELD), set("REGION", "uk")],
        ),
    ));
    assert_eq!(
        store.peek(&account("github", "TOKEN")).as_deref(),
        Some(HELD.as_bytes())
    );
    let text = dir.settings_json();
    assert!(!text.contains(HELD), "settings.json holds the value");
    let file = settings_file(&dir);
    let entry = &file["mcp"]["servers"]["github"];
    assert_eq!(entry["env"]["TOKEN"], json!({"keychain": true}), "{entry}");
    assert_eq!(entry["command"], "npx", "{entry}");
    let view = view_until(&mut sub, |view| !view.servers.is_empty());
    let [server] = view.servers.as_slice() else {
        panic!("one server: {view:?}");
    };
    assert_eq!(server.name, "github");
    assert_eq!(server.command.as_deref(), Some("npx"));
    assert!(server.enabled);
    assert_eq!(
        server.env,
        ["REGION", "TOKEN"].map(|name| McpSecretName {
            name: name.to_owned(),
            saved: true,
        })
    );
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains(HELD), "the view holds the value");

    // Rename, keeping both: the items move to the new name's accounts.
    host.ok(save(
        Some("github"),
        local("gh", "npx", vec![keep("TOKEN"), keep("REGION")]),
    ));
    let view = view_until(&mut sub, |view| {
        view.servers.iter().any(|server| server.name == "gh")
    });
    assert_eq!(view.servers.len(), 1, "{view:?}");
    assert_eq!(
        store.peek(&account("gh", "TOKEN")).as_deref(),
        Some(HELD.as_bytes())
    );
    let file = settings_file(&dir);
    assert_eq!(file["mcp"]["servers"]["github"], Value::Null, "{file}");
    assert_eq!(
        file["mcp"]["servers"]["gh"]["env"]["TOKEN"],
        json!({"keychain": true})
    );

    // Turn it off. One command's keychain work runs at a time, so this one's answer also means
    // the rename's clean-up of the old accounts has ended.
    host.ok(set_enabled("gh", false));
    for name in ["TOKEN", "REGION"] {
        assert_eq!(
            store.peek(&account("github", name)),
            None,
            "{name} left behind"
        );
    }
    assert_eq!(
        settings_file(&dir)["mcp"]["servers"]["gh"]["enabled"],
        false
    );
    let view = view_until(&mut sub, |view| {
        view.servers.iter().any(|server| !server.enabled)
    });
    assert_eq!(view.servers[0].name, "gh");

    // Remove: the items are deleted once the clean-up ends, which the next command waits for.
    host.ok(remove("gh"));
    host.ok(remove("never-there"));
    for name in ["TOKEN", "REGION"] {
        assert_eq!(store.peek(&account("gh", name)), None, "{name} left behind");
    }
    assert_eq!(
        settings_file(&dir)["mcp"]["servers"]["gh"],
        Value::Null,
        "{}",
        dir.settings_json()
    );
    let view = view_until(&mut sub, |view| view.servers.is_empty());
    assert_eq!(view.control, McpControl::Lawyer);
    host.shutdown();
}

/// The firm's list (policy) and its `managed-mcp.json` lock the commands, with the view saying
/// whose list it is.
#[test]
fn mcp_tools_commands_are_locked_by_the_firm() {
    // Policy sets `mcp.servers`.
    let dir = Dir::new("mcp_tools_firm_list", &[]);
    dir.set_policy(&json!({"mcp": {"servers": {"firm": {"command": "/opt/firm"}}}}).to_string());
    let store = store(&[]);
    let host = dir.start_with(0, |config| {
        config.with_mcp_secrets(Arc::<MemoryStore>::clone(&store))
    });
    let mut sub = subscribe(&host);
    let view = view_until(&mut sub, |_| true);
    assert_eq!(view.control, McpControl::FirmList);
    host.refused(
        save(None, local("github", "npx", vec![set("TOKEN", HELD)])),
        ErrorCode::SettingsLocked,
    );
    host.refused(set_enabled("firm", false), ErrorCode::SettingsLocked);
    assert_eq!(store.peek(&account("github", "TOKEN")), None);
    host.shutdown();

    // Claude Code's managed-mcp.json is on this computer.
    let dir = Dir::new("mcp_tools_firm_managed", &[]);
    dir.set_settings(
        &[],
        json!({"mcp": {"servers": {"mine": {"command": "/opt/mine", "enabled": true}}}}),
    );
    dir.set_managed_mcp();
    let before = dir.settings_json();
    let host = dir.start_with(0, |config| {
        config.with_mcp_secrets(Arc::<MemoryStore>::clone(&store))
    });
    let mut sub = subscribe(&host);
    let view = view_until(&mut sub, |_| true);
    assert_eq!(view.control, McpControl::FirmManaged);
    assert!(view.servers.is_empty(), "{view:?}");
    host.refused(
        save(None, local("github", "npx", vec![set("TOKEN", HELD)])),
        ErrorCode::SettingsLocked,
    );
    host.refused(remove("mine"), ErrorCode::SettingsLocked);
    host.refused(set_enabled("mine", false), ErrorCode::SettingsLocked);
    assert_eq!(dir.settings_json(), before);
    assert_eq!(store.peek(&account("github", "TOKEN")), None);
    // The lock is checked before the keychain is touched: not one read, write or delete.
    assert_eq!((store.reads(), store.writes(), store.deletes()), (0, 0, 0));
    host.shutdown();
}

/// Each refusal of a save leaves the store and the file as they were; an unknown server cannot
/// be turned on.
#[test]
fn mcp_tools_commands_refuse_and_change_nothing() {
    let dir = Dir::new("mcp_tools_refusals", &[]);
    let store = store(&[]);
    let found = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let resolver = {
        let found = Arc::clone(&found);
        Arc::new(move |typed: &str| {
            found
                .load(std::sync::atomic::Ordering::SeqCst)
                .then(|| PathBuf::from(typed))
        })
    };
    let host = dir.start_with(0, |config| {
        config
            .with_mcp_secrets(Arc::<MemoryStore>::clone(&store))
            .with_mcp_resolver(resolver)
    });
    host.ok(save(None, local("github", "npx", vec![set("TOKEN", HELD)])));
    let before = dir.settings_json();

    let refusals = [
        (
            local("case-law", "npx", Vec::new()),
            ErrorCode::McpNameReserved,
        ),
        (local("github", "npx", Vec::new()), ErrorCode::McpNameTaken),
        (
            local("other", "bin/npx", Vec::new()),
            ErrorCode::McpCommandInvalid,
        ),
        (
            remote("web", "http://example.com"),
            ErrorCode::McpUrlInvalid,
        ),
    ];
    for (server, code) in refusals {
        let name = server.name.clone();
        host.refused(save(None, server), code);
        assert_eq!(dir.settings_json(), before, "after {name}");
    }

    // The program is not found: nothing in the store, the file unchanged.
    found.store(false, std::sync::atomic::Ordering::SeqCst);
    host.refused(
        save(None, local("lost", "npx", vec![set("TOKEN", HELD)])),
        ErrorCode::McpCommandNotFound,
    );
    assert_eq!(store.peek(&account("lost", "TOKEN")), None);
    assert_eq!(dir.settings_json(), before);
    found.store(true, std::sync::atomic::Ordering::SeqCst);

    // The keychain fails: no item left, the file unchanged.
    store.fail_next_writes(1);
    host.refused(
        save(None, local("flaky", "npx", vec![set("TOKEN", HELD)])),
        ErrorCode::McpSecretStoreFailed,
    );
    assert_eq!(store.peek(&account("flaky", "TOKEN")), None);
    assert_eq!(dir.settings_json(), before);

    host.refused(set_enabled("nobody", true), ErrorCode::McpServerUnknown);
    assert_eq!(dir.settings_json(), before);
    assert_eq!(
        store.peek(&account("github", "TOKEN")).as_deref(),
        Some(HELD.as_bytes()),
        "a refusal touched another server's item"
    );
    host.shutdown();
}
