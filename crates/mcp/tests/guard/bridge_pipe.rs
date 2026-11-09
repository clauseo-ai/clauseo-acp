//! Windows: the built `clauseo-mcp browser-bridge --pipe` in front of a real
//! `browser-guard --listen-pipe` (with the stubs), the twin of `bridge.rs`. Told the identity the
//! guard prints (`listening as <pid>/<creation time>`) it relays a handshake and a tool call.
//! Told another process's pid, or the guard's pid with another creation time, it refuses the
//! pipe's server and answers each request itself: what an instance that another process of the
//! browser's account added looks like. Without the identity it does not start.

#![cfg(windows)]

use std::process::Command;

use serde_json::json;

use crate::listen_pipe::{listen_command, own_sid, pipe_name};
use crate::support::{GUARD, Guard, Scratch, open_call, run};

/// The bridge's error for a request it answers itself.
const UNAVAILABLE: &str = "The browser isn't available right now.";

/// The bridge, over pipes, for the pipe `name` and the guard identity `pid`/`version`.
fn bridge(name: &str, pid: u32, version: u64) -> Guard {
    let mut command = Command::new(GUARD);
    command
        .arg("browser-bridge")
        .args(["--pipe", name, "--server-sid", &own_sid()])
        .args(["--guard-pid", &pid.to_string()])
        .args(["--guard-pidversion", &version.to_string()]);
    Guard::start(command)
}

/// A guard listening on the pipe `name`, and the identity it printed.
fn listening(scratch: &Scratch, name: &str) -> (Guard, u32, u64) {
    let mut guard = Guard::start(listen_command(scratch, name));
    let line = guard.stderr_until("listening as ");
    let identity = line.trim_end().rsplit(' ').next().unwrap_or_default();
    let (pid, version) = identity.split_once('/').expect("<pid>/<version>");
    let pid: u32 = pid.parse().unwrap();
    assert_eq!(pid, guard.pid(), "the guard printed another pid: {line}");
    assert!(!line.contains(name), "the pipe name in {line}");
    (guard, pid, version.parse().unwrap())
}

/// Two requests, each answered by the bridge itself.
fn refused(mut bridge: Guard) {
    for id in [1, 2] {
        bridge.send(&json!({"jsonrpc": "2.0", "id": id, "method": "initialize"}));
        let answer = bridge.read();
        assert_eq!(answer["id"], id, "{answer}");
        assert_eq!(answer["error"]["message"], UNAVAILABLE, "{answer}");
    }
    let (status, rest) = bridge.finish();
    assert_eq!(status.code(), Some(0));
    assert_eq!(rest, "");
}

#[test]
fn told_the_guards_identity_the_bridge_relays() {
    let scratch = Scratch::new("pipe_bridge_relays");
    let name = pipe_name("bridge-relays");
    let (mut guard, pid, version) = listening(&scratch, &name);
    let mut bridge = bridge(&name, pid, version);
    bridge.send(&json!({"jsonrpc": "2.0", "id": 0, "method": "initialize"}));
    let answer = bridge.read();
    assert_eq!(answer["id"], 0, "{answer}");
    assert!(answer.get("error").is_none(), "{answer}");
    bridge.send(&open_call("open"));
    let answer = bridge.read();
    assert_eq!(answer["id"], "open", "{answer}");
    assert!(answer.get("error").is_none(), "{answer}");
    let (status, rest) = bridge.finish();
    assert_eq!(status.code(), Some(0));
    assert_eq!(rest, "");
    guard.stderr_until("the client disconnected; stopped Chrome");
    drop(guard);
}

/// The pipe's server is not the process the bridge was told (this test process's pid instead).
#[test]
fn another_server_process_is_refused() {
    let scratch = Scratch::new("pipe_bridge_other_pid");
    let name = pipe_name("bridge-other-pid");
    let (guard, _, version) = listening(&scratch, &name);
    refused(bridge(&name, std::process::id(), version));
    assert!(
        !scratch.path().join("chrome.pid").exists(),
        "the request reached it"
    );
    drop(guard);
}

/// The guard's pid with another creation time: a process that reused the guard's pid.
#[test]
fn another_creation_time_is_refused() {
    let scratch = Scratch::new("pipe_bridge_other_time");
    let name = pipe_name("bridge-other-time");
    let (guard, pid, version) = listening(&scratch, &name);
    refused(bridge(&name, pid, version + 1));
    assert!(
        !scratch.path().join("chrome.pid").exists(),
        "the request reached it"
    );
    drop(guard);
}

#[test]
fn without_the_guards_identity_the_bridge_does_not_start() {
    let sid = own_sid();
    for args in [
        &["--pipe", "clauseo-no", "--server-sid", sid.as_str()][..],
        &[
            "--pipe",
            "clauseo-no",
            "--server-sid",
            sid.as_str(),
            "--guard-pid",
            "4242",
        ],
    ] {
        let mut command = Command::new(GUARD);
        command.arg("browser-bridge").args(args);
        let output = run(command);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.ends_with(
                "usage: clauseo-mcp browser-bridge --pipe <name> --server-sid <sid> --guard-pid \
                 <pid> --guard-pidversion <version>\n"
            ),
            "{stderr}"
        );
        assert_eq!(output.stdout, Vec::<u8>::new());
    }
}
