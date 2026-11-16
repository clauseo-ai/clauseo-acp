//! The lawyer's MCP servers when something fails (Settings › Tools): an agent that dies before
//! its session is ready prints a server's values in its last stderr line, which reaches the log
//! hidden; the settings write fails after the keychain write, which is then put back; and two
//! saves at once run their keychain work one after the other.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use clauseo_connectors::{MemoryStore, SecretStore, StoreError};
use clauseo_host::log_file;
use clauseo_protocol::{CommandEnvelope, DomainEvent, ErrorCode, HostCommand, StreamId};
use clauseo_settings::Locations;
use serde_json::json;

use crate::mcp_tools::{HELD, account, finds_npx, local, npx, read_logs, save, set, store, wired};
use crate::observe::Events;
use crate::profile_rig::template;
use crate::support::{Dir, FAKE, command_id, wait};
use crate::write;

/// A plain value, long enough for the redactor to hide.
const PLAIN: &str = "pelican-orchard-tramline-velvet";

/// Whether the turn ended or failed.
fn turn_over(events: &Events) -> bool {
    events.events().any(|event| {
        matches!(
            event,
            DomainEvent::TurnEnded(_) | DomainEvent::TurnFailed(_)
        )
    })
}

/// An agent that takes `session/new`, prints a server's values in its last stderr line and exits
/// before answering: its start failure is logged with that line, and the log holds neither the
/// keychain value nor the policy's plain one.
#[test]
fn mcp_tools_a_start_failure_logs_no_server_value() {
    let dir = Dir::new("mcp_tools_start_failure", &[]);
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");

    let name = "mcp-tools-start-failure";
    let lines = [
        json!({"scenario": name, "description": "prints the server's values, then exits before \
            the session is ready", "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init"}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"expect": {"method": "session/new", "as": "new"}}),
        json!({"stderr": {"line": format!(
            "fatal: github could not start with TOKEN={HELD} API_PLAIN={PLAIN}"
        )}}),
        json!({"exit": {"code": 1}}),
    ]
    .map(|line| line.to_string())
    .join("\n");
    let scenario = template(&dir, name, &lines);
    let npx = npx(&dir);
    // The server is the firm's: a policy's plain value, as a hand-edited file's would be, and a
    // keychain value.
    dir.set_settings(&[(FAKE, scenario)], json!({}));
    dir.set_policy(
        &json!({"mcp": {"servers": {"github": {"command": "npx", "enabled": true,
            "env": {"TOKEN": {"keychain": true}, "API_PLAIN": PLAIN}}}}})
        .to_string(),
    );
    let store = store(&[(&account("github", "TOKEN"), HELD)]);
    let host = dir.start_with(0, |config| wired(config, &store, finds_npx(&npx)));
    let matter = host.create_matter("m", &dir.folder("m", &[("brief.txt", "Clause 4\n")]));
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    host.prompt(thread, "Summarise clause 4.");
    events.until("the failed start ends the turn", turn_over);
    host.shutdown();
    drop(guard);

    let log = read_logs(&locations.log_dir());
    assert!(
        log.contains("the agent ended before its session was ready")
            && log.contains("fatal: github could not start"),
        "the log holds no start failure with the agent's last line:\n{log}"
    );
    assert!(!log.contains(HELD), "the log holds the keychain value");
    assert!(!log.contains(PLAIN), "the log holds the plain value");
}

/// The settings write fails after the keychain write (`settings.json` became a folder): the
/// save answers the write's error, and the keychain is put back: the changed item holds its old
/// value again and the new item is deleted, not written empty.
#[test]
fn mcp_tools_a_failed_settings_write_puts_the_keychain_back() {
    let dir = Dir::new("mcp_tools_settings_fail", &[]);
    let npx = npx(&dir);
    let store = store(&[]);
    let host = dir.start_with(0, |config| wired(config, &store, finds_npx(&npx)));
    host.ok(save(None, local("github", "npx", vec![set("TOKEN", HELD)])));
    let settings = dir.data().join("settings.json");
    write::remove_file(&settings);
    write::create_dir_all(&settings);
    let writes = store.writes();

    host.refused(
        save(
            Some("github"),
            local(
                "github",
                "npx",
                vec![
                    set("TOKEN", "a-new-value-never-kept"),
                    set("NEW", "another-new-value"),
                ],
            ),
        ),
        ErrorCode::Internal,
    );
    assert!(
        store.writes() > writes,
        "the keychain was not written first"
    );
    assert_eq!(
        store.peek(&account("github", "TOKEN")).as_deref(),
        Some(HELD.as_bytes()),
        "the old value was not put back"
    );
    assert_eq!(
        store.peek(&account("github", "NEW")),
        None,
        "the new item stayed"
    );
    host.shutdown();
}

/// A store whose writes take a while and that counts how many run at once.
struct Slow {
    inner: MemoryStore,
    running: AtomicUsize,
    most: AtomicUsize,
}

impl SecretStore for Slow {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get(account)
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        // Hold the write a while, so a second one running beside it would overlap it.
        let (_held, wait) = std::sync::mpsc::channel::<()>();
        let _timed_out = wait.recv_timeout(Duration::from_millis(50));
        let result = self.inner.set(account, secret);
        self.running.fetch_sub(1, Ordering::SeqCst);
        result
    }

    fn delete(&self, account: &str) -> Result<(), StoreError> {
        self.inner.delete(account)
    }
}

/// Two saves sent at once: both succeed, each server's items are written, and no two keychain
/// writes ever ran at the same time (one command's keychain work at a time; the second waits).
#[test]
fn mcp_tools_two_saves_at_once_run_one_after_the_other() {
    let dir = Dir::new("mcp_tools_two_saves", &[]);
    let npx = npx(&dir);
    let slow = Arc::new(Slow {
        inner: MemoryStore::new(),
        running: AtomicUsize::new(0),
        most: AtomicUsize::new(0),
    });
    let host = dir.start_with(0, |config| {
        config
            .with_mcp_secrets(Arc::<Slow>::clone(&slow))
            .with_mcp_resolver(finds_npx(&npx))
    });
    let envelope = |command: HostCommand| CommandEnvelope {
        command_id: command_id(),
        command,
    };
    let first = host.handle.dispatch(envelope(save(
        None,
        local(
            "alpha",
            "npx",
            vec![set("A", "alpha-value-one"), set("B", "alpha-value-two")],
        ),
    )));
    let second = host.handle.dispatch(envelope(save(
        None,
        local(
            "beta",
            "npx",
            vec![set("A", "beta-value-one"), set("B", "beta-value-two")],
        ),
    )));
    let (first, second) = wait("both saves", futures::future::join(first, second));
    first.unwrap();
    second.unwrap();
    assert_eq!(
        slow.most.load(Ordering::SeqCst),
        1,
        "two keychain writes ran at once"
    );
    for (server, value) in [("alpha", "alpha-value-one"), ("beta", "beta-value-one")] {
        assert_eq!(
            slow.inner.peek(&account(server, "A")).as_deref(),
            Some(value.as_bytes())
        );
    }
    host.shutdown();
}
