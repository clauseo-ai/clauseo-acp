//! Listen mode on Windows (`browser-guard --listen-pipe <name> --client-sid <SID>`), with the
//! stubs, the twin of `listen.rs`'s cycle: a client connects, its tool call starts the stub
//! Chrome, its disconnect stops the browser (the profile stays) and the guard stays up for the
//! next client, whose calls start it again; a second client while one is connected is refused;
//! a connection that sends nothing loses its place; the guard prints its pid and creation time;
//! and a pipe name that exists exits 3. The test's own user is the client SID. Waits poll as
//! `lifecycle.rs`'s do, or read the guard's stderr.

#![cfg(windows)]

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

use crate::lifecycle::{no_args, within};
use crate::support::{
    GUARD, Guard, Scratch, example, guard_args, host_env, open_call, profiles, run,
};

/// What the guard prints once it has handled a client's disconnect.
const LEFT: &str = "the client disconnected; stopped Chrome and agent-browser";

/// A pipe name unique to the test and this process.
pub(crate) fn pipe_name(test: &str) -> String {
    format!("clauseo-test-{test}-{}", std::process::id())
}

fn pipe_path(name: &str) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\{name}"))
}

/// The test's own user SID, from `whoami /user`: the one word of its table that is a SID.
pub(crate) fn own_sid() -> String {
    let mut command = Command::new("whoami");
    command.arg("/user");
    let output = run(command);
    assert!(output.status.success(), "whoami failed");
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .find(|word| word.starts_with("S-1-"))
        .unwrap_or_else(|| panic!("no SID in {text}"))
        .to_owned()
}

pub(crate) fn listen_command(scratch: &Scratch, name: &str) -> Command {
    let mut args = guard_args(scratch, &example("daemon_stub"), &no_args());
    args.splice(
        1..1,
        [
            "--listen-pipe".into(),
            name.into(),
            "--client-sid".into(),
            own_sid().into(),
        ],
    );
    let mut command = Command::new(GUARD);
    command.args(args).envs(host_env(scratch));
    command
}

/// A client of the guard's pipe, opened synchronously: it writes, then reads, on one thread.
struct Client {
    reader: BufReader<File>,
    writer: File,
}

impl Client {
    /// Connects once the pipe takes connections.
    fn connect(name: &str) -> Client {
        let mut file = None;
        assert!(
            within(|| {
                file = crate::write::open_read_write(pipe_path(name)).ok();
                file.is_some()
            }),
            "the guard never listened"
        );
        let file = file.unwrap();
        Client {
            writer: file.try_clone().unwrap(),
            reader: BufReader::new(file),
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
        match self.reader.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(serde_json::from_str(&line).unwrap()),
        }
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
    let scratch = Scratch::new("pipe_cycle");
    let name = pipe_name("cycle");
    let mut guard = Guard::start(listen_command(&scratch, &name));
    let mut client = Client::connect(&name);
    client.request(0, "initialize");
    client.call("open");
    assert!(
        within(|| profiles(&scratch.cwd()).len() == 1),
        "no profile after the call"
    );
    drop(client);
    guard.stderr_until(LEFT);
    assert_eq!(profiles(&scratch.cwd()).len(), 1, "the profile went");
    let mut client = Client::connect(&name);
    client.request(0, "initialize");
    client.request(1, "tools/list");
    client.call("again");
    drop(client);
    drop(guard);
}

#[test]
fn a_second_client_is_refused_while_one_is_connected() {
    let scratch = Scratch::new("pipe_busy");
    let name = pipe_name("busy");
    let mut guard = Guard::start(listen_command(&scratch, &name));
    let mut first = Client::connect(&name);
    first.request(0, "initialize");
    let mut second = Client::connect(&name);
    let refusal = second.read();
    assert_eq!(refusal["id"], Value::Null, "{refusal}");
    let message = refusal["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("in use by another connection"),
        "{refusal}"
    );
    assert!(second.next().is_none(), "the second connection stayed open");
    first.request(1, "tools/list");
    guard.stderr_until("refused a second connection");
    drop(guard);
}

#[test]
fn a_pipe_name_that_exists_exits_3_and_names_nothing() {
    let scratch = Scratch::new("pipe_taken");
    let name = pipe_name("taken");
    let _holder = Guard::start(listen_command(&scratch, &name));
    let _connected = Client::connect(&name);
    let second = Scratch::new("pipe_taken_second");
    let output = run(listen_command(&second, &name));
    assert_eq!(
        output.status.code(),
        Some(clauseo_mcp::browser_guard::EXIT_PIPE_TAKEN)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--listen-pipe name exists already"),
        "{stderr}"
    );
    assert!(!stderr.contains(&name), "the name in {stderr}");
    assert!(!stderr.contains(&own_sid()), "the SID in {stderr}");
}

/// A connection that sends nothing is closed once the first-line wait (shortened here) is over,
/// so it cannot keep the one client's place: the next client is served. The pipe's read timeout
/// is what bounds that wait.
#[test]
fn a_connection_that_sends_nothing_loses_its_place() {
    let scratch = Scratch::new("pipe_silent");
    let name = pipe_name("silent");
    let mut command = listen_command(&scratch, &name);
    command.env("CLAUSEO_TEST_GUARD_FIRST_LINE_MS", "300");
    let mut guard = Guard::start(command);
    let mut silent = Client::connect(&name);
    assert!(silent.next().is_none(), "the silent connection stayed open");
    guard.stderr_until("closed a client that sent no whole line in time");
    guard.stderr_until("the client disconnected");
    let mut client = Client::connect(&name);
    client.request(0, "initialize");
    client.request(1, "tools/list");
    drop(guard);
}

/// The guard prints its identity for the host, `<pid>/<creation time>`, as on Unix.
#[test]
fn the_guard_prints_its_pid_and_creation_time() {
    let scratch = Scratch::new("pipe_identity");
    let name = pipe_name("identity");
    let mut guard = Guard::start(listen_command(&scratch, &name));
    let line = guard.stderr_until("listening as");
    let identity = line.trim().rsplit(' ').next().unwrap();
    let (pid, version) = identity.split_once('/').unwrap();
    assert_eq!(pid.parse::<u32>().unwrap(), guard.pid());
    assert!(version.parse::<u64>().unwrap() > 0, "{line}");
    assert!(!line.contains(&name), "the pipe name in {line}");
    drop(guard);
}
