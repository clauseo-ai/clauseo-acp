//! Listen mode (`browser-guard --listen <socket>`, Unix only), with the stubs: a client connects,
//! its tool call starts the stub Chrome, its disconnect stops Chrome and the daemon (the profile
//! stays) and the guard stays up for the next client, whose calls start them again; a second
//! client while one is connected is refused; a socket path that exists is refused; and the
//! parent's going ends the guard and removes the socket. Waits poll as `lifecycle.rs`'s do, or
//! read the guard's stderr.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use crate::lifecycle::{
    alive, assert_nothing_left, no_args, pid_in, session_pids, sigkill, within,
};
use crate::support::{
    GUARD, Guard, Scratch, example, guard_args, host_env, open_call, profiles, run, run_files,
};

/// What the guard prints once it has handled a client's disconnect.
const LEFT: &str = "the client disconnected; stopped Chrome and agent-browser";

/// The listen socket the tests give the guard, in the host's socket folder.
fn socket_of(scratch: &Scratch) -> PathBuf {
    scratch.socket_dir().join("guard.sock")
}

/// The guard's arguments in front of `daemon_stub`, with `--listen <socket>` first.
fn listen_args(scratch: &Scratch, socket: &Path) -> Vec<std::ffi::OsString> {
    let mut args = guard_args(scratch, &example("daemon_stub"), &no_args());
    args.splice(1..1, ["--listen".into(), socket.into()]);
    args
}

fn listen_command(scratch: &Scratch, socket: &Path) -> Command {
    let mut command = Command::new(GUARD);
    command
        .args(listen_args(scratch, socket))
        .envs(host_env(scratch));
    command
}

/// A client of the guard's socket.
struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Client {
    /// Connects once the socket takes connections.
    fn connect(socket: &Path) -> Client {
        let mut stream = None;
        assert!(
            within(|| {
                stream = UnixStream::connect(socket).ok();
                stream.is_some()
            }),
            "the guard never listened"
        );
        let stream = stream.unwrap();
        Client {
            writer: stream.try_clone().unwrap(),
            reader: BufReader::new(stream),
        }
    }

    fn send(&mut self, message: &Value) {
        self.writer
            .write_all(format!("{message}\n").as_bytes())
            .unwrap();
    }

    /// The next line, parsed; `None` at the end.
    fn next(&mut self) -> Option<Value> {
        let mut line = String::new();
        (self.reader.read_line(&mut line).unwrap() > 0)
            .then(|| serde_json::from_str(&line).unwrap())
    }

    fn read(&mut self) -> Value {
        self.next().expect("the guard closed the connection")
    }

    fn request(&mut self, id: u64, method: &str) -> Value {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method}));
        let answer = self.read();
        assert_eq!(answer["id"], id, "{answer}");
        answer
    }

    fn call(&mut self, id: &str) -> Value {
        self.send(&open_call(id));
        let answer = self.read();
        assert_eq!(answer["id"], id, "{answer}");
        assert!(answer.get("error").is_none(), "{answer}");
        answer
    }
}

#[test]
fn a_disconnect_stops_the_browser_and_the_next_client_starts_it_again() {
    let scratch = Scratch::new("listen_cycle");
    let socket = socket_of(&scratch);
    let mut guard = Guard::start(listen_command(&scratch, &socket));
    let mut client = Client::connect(&socket);
    let metadata = std::fs::symlink_metadata(&socket).unwrap();
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    client.request(0, "initialize");
    client.call("open");
    let first = session_pids(&scratch);
    assert!(alive(first[1]), "no Chrome after the call");
    drop(client);
    guard.stderr_until(LEFT);
    assert!(within(|| !alive(first[1])), "Chrome {} is left", first[1]);
    assert!(
        within(|| !alive(first[0])),
        "the daemon {} is left",
        first[0]
    );
    assert_eq!(profiles(&scratch.cwd()).len(), 1, "the profile went");
    assert!(
        alive(i32::try_from(guard.pid()).unwrap()),
        "the guard ended"
    );
    let mut client = Client::connect(&socket);
    client.request(0, "initialize");
    client.request(1, "tools/list");
    client.call("again");
    let mut pids = [0, 0];
    assert!(within(|| {
        pids = session_pids(&scratch);
        pids[1] != first[1] && alive(pids[1])
    }));
    sigkill(i32::try_from(guard.pid()).unwrap());
    assert_nothing_left(&scratch, pids);
    drop(guard);
}

#[test]
fn a_second_client_is_refused_while_one_is_connected() {
    let scratch = Scratch::new("listen_busy");
    let socket = socket_of(&scratch);
    let mut guard = Guard::start(listen_command(&scratch, &socket));
    let mut first = Client::connect(&socket);
    first.request(0, "initialize");
    let mut second = Client::connect(&socket);
    let refusal = second.read();
    assert_eq!(refusal["id"], Value::Null, "{refusal}");
    let message = refusal["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("in use by another connection"),
        "{refusal}"
    );
    assert!(!message.contains('/'), "a path in {message:?}");
    assert!(second.next().is_none(), "the second connection stayed open");
    first.request(1, "tools/list");
    guard.stderr_until("refused a second connection");
    drop(guard);
}

#[test]
fn a_socket_path_that_exists_is_refused_and_left_alone() {
    let scratch = Scratch::new("listen_exists");
    let socket = socket_of(&scratch);
    crate::write::write(&socket, "someone else's").unwrap();
    let output = run(listen_command(&scratch, &socket));
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("socket path exists already"), "{stderr}");
    assert!(
        !stderr.contains(socket.to_str().unwrap()),
        "the path in {stderr}"
    );
    assert_eq!(std::fs::read_to_string(&socket).unwrap(), "someone else's");
    assert_eq!(
        run_files(&scratch.cwd()),
        Vec::<PathBuf>::new(),
        "a run was claimed"
    );
}

/// The test holds the guard's stdin and a client, so only the parent check can end it.
#[test]
fn the_parents_going_ends_the_guard_and_removes_the_socket() {
    let scratch = Scratch::new("listen_parent");
    let socket = socket_of(&scratch);
    let guard_pid_file = scratch.path().join("guard.pid");
    let mut command = Command::new(example("parent_stub"));
    command
        .arg(GUARD)
        .args(listen_args(&scratch, &socket))
        .envs(host_env(&scratch))
        .env("PARENT_STUB_CHILD_PID", &guard_pid_file);
    let parent = Guard::start(command);
    let mut client = Client::connect(&socket);
    client.call("open");
    let guard = pid_in(&guard_pid_file);
    let pids = session_pids(&scratch);
    sigkill(i32::try_from(parent.pid()).unwrap());
    assert!(within(|| !alive(guard)), "the guard {guard} is left");
    assert!(
        within(|| std::fs::symlink_metadata(&socket).is_err()),
        "the socket is left"
    );
    assert_nothing_left(&scratch, pids);
    assert!(client.next().is_none(), "the connection stayed open");
    drop(parent);
}
