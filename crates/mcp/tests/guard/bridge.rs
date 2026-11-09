//! The built `clauseo-mcp browser-bridge` in front of a real `browser-guard --listen` (with the
//! stubs). Told the identity the guard prints (`listening as <pid>/<version>`) it relays a
//! handshake and a tool call. Told another process's pid, or the guard's pid with another
//! version (a reused pid), it refuses the process on the socket and answers each request itself.
//! Without the identity it does not start.

#![cfg(unix)]

use std::process::Command;

use serde_json::json;

use crate::lifecycle::{assert_nothing_left, session_pids, sigkill};
use crate::listen::{listen_command, socket_of};
use crate::support::{GUARD, Guard, Scratch, open_call, run};

/// The bridge's error for a request it answers itself.
const UNAVAILABLE: &str = "The browser isn't available right now.";

/// The bridge, over pipes, for the socket at `socket` and the guard identity `pid`/`version`.
fn bridge(socket: &std::path::Path, pid: u32, version: u64) -> Guard {
    let mut command = Command::new(GUARD);
    command
        .arg("browser-bridge")
        .arg("--socket")
        .arg(socket)
        .args(["--guard-pid", &pid.to_string()])
        .args(["--guard-pidversion", &version.to_string()]);
    Guard::start(command)
}

/// A guard listening at `socket`, and the identity it printed.
fn listening(scratch: &Scratch, socket: &std::path::Path) -> (Guard, u32, u64) {
    let mut guard = Guard::start(listen_command(scratch, socket));
    let line = guard.stderr_until("listening as ");
    let identity = line.trim_end().rsplit(' ').next().unwrap_or_default();
    let (pid, version) = identity.split_once('/').expect("<pid>/<version>");
    let pid: u32 = pid.parse().unwrap();
    assert_eq!(pid, guard.pid(), "the guard printed another pid: {line}");
    assert!(!line.contains(socket.to_str().unwrap()), "a path in {line}");
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
    let scratch = Scratch::new("bridge_relays");
    let socket = socket_of(&scratch);
    let (mut guard, pid, version) = listening(&scratch, &socket);
    let mut bridge = bridge(&socket, pid, version);
    bridge.send(&json!({"jsonrpc": "2.0", "id": 0, "method": "initialize"}));
    let answer = bridge.read();
    assert_eq!(answer["id"], 0, "{answer}");
    assert!(answer.get("error").is_none(), "{answer}");
    bridge.send(&open_call("open"));
    let answer = bridge.read();
    assert_eq!(answer["id"], "open", "{answer}");
    assert!(answer.get("error").is_none(), "{answer}");
    let pids = session_pids(&scratch);
    let (status, rest) = bridge.finish();
    assert_eq!(status.code(), Some(0));
    assert_eq!(rest, "");
    guard.stderr_until("the client disconnected; stopped Chrome");
    sigkill(i32::try_from(guard.pid()).unwrap());
    assert_nothing_left(&scratch, pids);
    drop(guard);
}

/// The process on the socket is not the one the bridge was told (this test process's pid
/// instead), so the bridge refuses it on every try.
#[test]
fn another_process_on_the_socket_is_refused() {
    let scratch = Scratch::new("bridge_other_pid");
    let socket = socket_of(&scratch);
    let (mut squatter, _, version) = listening(&scratch, &socket);
    refused(bridge(&socket, std::process::id(), version));
    sigkill(i32::try_from(squatter.pid()).unwrap());
    assert_eq!(squatter.wait().code(), None, "killed by the signal");
    assert!(
        !scratch.path().join("chrome.pid").exists(),
        "the request reached it"
    );
}

/// The guard's pid with another version: what a process that reused the guard's pid looks like.
#[test]
fn a_reused_pid_is_refused() {
    let scratch = Scratch::new("bridge_reused_pid");
    let socket = socket_of(&scratch);
    let (mut squatter, pid, version) = listening(&scratch, &socket);
    refused(bridge(&socket, pid, version + 1));
    sigkill(i32::try_from(squatter.pid()).unwrap());
    assert_eq!(squatter.wait().code(), None, "killed by the signal");
    assert!(
        !scratch.path().join("chrome.pid").exists(),
        "the request reached it"
    );
}

#[test]
fn without_the_guards_identity_the_bridge_does_not_start() {
    for args in [
        &["--socket", "/tmp/clauseo-no.sock"][..],
        &["--socket", "/tmp/clauseo-no.sock", "--guard-pid", "4242"],
    ] {
        let mut command = Command::new(GUARD);
        command.arg("browser-bridge").args(args);
        let output = run(command);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.ends_with(
                "usage: clauseo-mcp browser-bridge --socket <path> --guard-pid <pid> \
                 --guard-pidversion <version>\n"
            ),
            "{stderr}"
        );
        assert_eq!(output.stdout, Vec::<u8>::new());
    }
}
