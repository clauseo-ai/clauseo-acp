//! The bridge against a stand-in guard on a real Unix socket: lines both ways, unchanged; the
//! caps; no guard at all; and a guard that drops the connection mid-session, then takes the
//! reconnect. Every wait is a blocking read or a join; nothing sleeps.

use std::cell::Cell;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use serde_json::{Value, json};

use super::{CONNECT_WAIT, Caps, Out, UNAVAILABLE, connect_within, parse, serve};
use crate::transport::Peer;
use crate::transport::fake::Fake;

/// The stand-in guard's peer: this test process, which binds its socket.
fn own() -> Peer {
    Peer::Process(crate::transport::unix::own_identity().unwrap())
}

/// A folder for the stand-in guard's socket, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new(test: &str) -> Dir {
        let path =
            std::env::temp_dir().join(format!("clauseo-bridge-{test}-{}", std::process::id()));
        drop(crate::write::remove_dir_all(&path));
        crate::write::create_dir_all(&path).unwrap();
        Dir(path)
    }

    fn socket(&self) -> PathBuf {
        self.0.join("g.sock")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        drop(crate::write::remove_dir_all(&self.0));
    }
}

fn listen(path: &Path) -> UnixListener {
    crate::write::bind_unix_socket(path).unwrap()
}

/// The bridge on a thread, its stdin and stdout as the test's ends of two socket pairs.
struct Bridge {
    stdin: Option<UnixStream>,
    stdout: BufReader<UnixStream>,
    thread: JoinHandle<std::io::Result<()>>,
}

impl Bridge {
    fn start(socket: PathBuf, caps: Caps) -> Bridge {
        let (stdin, their_stdin) = UnixStream::pair().unwrap();
        let (their_stdout, stdout) = UnixStream::pair().unwrap();
        let thread = thread::spawn(move || {
            let out: Out = Arc::new(Mutex::new(their_stdout));
            let connect = || connect_within(&socket, CONNECT_WAIT);
            serve(BufReader::new(their_stdin), &out, &connect, own(), caps)
        });
        Bridge {
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            thread,
        }
    }

    fn send(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(format!("{line}\n").as_bytes()).unwrap();
    }

    fn read_line(&mut self) -> String {
        let mut line = String::new();
        assert!(
            self.stdout.read_line(&mut line).unwrap() > 0,
            "stdout ended"
        );
        line
    }

    fn read(&mut self) -> Value {
        serde_json::from_str(&self.read_line()).unwrap()
    }

    /// Closes stdin and waits for the bridge to return.
    fn finish(mut self) {
        self.stdin = None;
        self.thread.join().unwrap().unwrap();
    }
}

/// One accepted connection on the stand-in guard's side.
struct Remote {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Remote {
    fn accept(listener: &UnixListener) -> Remote {
        let (stream, _) = listener.accept().unwrap();
        Remote {
            writer: stream.try_clone().unwrap(),
            reader: BufReader::new(stream),
        }
    }

    /// The next line, without its newline; `None` at the end.
    fn line(&mut self) -> Option<String> {
        let mut line = String::new();
        (self.reader.read_line(&mut line).unwrap() > 0)
            .then(|| line.trim_end_matches('\n').to_owned())
    }

    fn write(&mut self, line: &str) {
        self.writer
            .write_all(format!("{line}\n").as_bytes())
            .unwrap();
    }
}

fn unavailable(answer: &Value, id: &Value) {
    assert_eq!(&answer["id"], id, "{answer}");
    assert_eq!(answer["error"]["message"], UNAVAILABLE, "{answer}");
}

#[test]
fn lines_go_both_ways_unchanged() {
    let dir = Dir::new("both");
    let listener = listen(&dir.socket());
    let guard = thread::spawn(move || {
        let mut peer = Remote::accept(&listener);
        let mut received = Vec::new();
        while let Some(line) = peer.line() {
            if line.contains("\"id\"") {
                peer.write(r#"{"jsonrpc":"2.0","id":1,  "result":{"said":"hi"}}"#);
                peer.write(r#"{"jsonrpc":"2.0","method":"notifications/message"}"#);
            }
            received.push(line);
        }
        received
    });
    let mut bridge = Bridge::start(dir.socket(), Caps::GUARD);
    let request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"a":  "  b  "}}"#;
    let notice = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    bridge.send(request);
    assert_eq!(
        bridge.read_line(),
        "{\"jsonrpc\":\"2.0\",\"id\":1,  \"result\":{\"said\":\"hi\"}}\n"
    );
    assert_eq!(
        bridge.read_line(),
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\"}\n"
    );
    bridge.send(notice);
    bridge.finish();
    assert_eq!(guard.join().unwrap(), [request, notice]);
}

#[test]
fn lines_over_the_caps_are_refused_or_end_the_link() {
    let dir = Dir::new("caps");
    let listener = listen(&dir.socket());
    let long = "x".repeat(80);
    let guard = {
        let long = long.clone();
        thread::spawn(move || {
            let mut peer = Remote::accept(&listener);
            let first = peer.line();
            // The second request: a line over the cap back, which ends the link.
            let second = peer.line();
            peer.write(&long);
            let rest = peer.line();
            (first, second, rest)
        })
    };
    let caps = Caps {
        agent: 64,
        guard: 64,
    };
    let mut bridge = Bridge::start(dir.socket(), caps);
    bridge.send(&format!(r#"{{"id":9,"method":"ping","p":"{long}"}}"#));
    let refused = bridge.read();
    assert_eq!(refused["id"], Value::Null, "{refused}");
    assert_eq!(refused["error"]["code"], -32700, "{refused}");
    bridge.send(r#"{"id":1,"method":"ping"}"#);
    bridge.send(r#"{"id":2,"method":"ping"}"#);
    // Neither is answered by the guard; the oversize line ends the link and the bridge answers
    // both.
    let mut ids = [bridge.read(), bridge.read()];
    ids.sort_by_key(|answer| answer["id"].as_i64());
    unavailable(&ids[0], &json!(1));
    unavailable(&ids[1], &json!(2));
    bridge.finish();
    let (first, second, rest) = guard.join().unwrap();
    assert_eq!(
        first.as_deref(),
        Some(r#"{"id":1,"method":"ping"}"#),
        "the long line went"
    );
    assert_eq!(second.as_deref(), Some(r#"{"id":2,"method":"ping"}"#));
    assert_eq!(rest, None, "the link stayed open");
}

#[test]
fn with_no_guard_each_request_gets_an_error_and_stdin_is_read_to_its_end() {
    let dir = Dir::new("none");
    let socket = dir.socket();
    let tries = Cell::new(0);
    let connect = || {
        tries.set(tries.get() + 1);
        connect_within(&socket, CONNECT_WAIT)
    };
    let stdin = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":"a","method":"tools/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call"}"#,
    );
    let written = Arc::new(Mutex::new(Vec::<u8>::new()));
    let out: Out = Arc::<Mutex<Vec<u8>>>::clone(&written);
    serve(Cursor::new(stdin), &out, &connect, own(), Caps::GUARD).unwrap();
    let text = String::from_utf8(written.lock().unwrap().clone()).unwrap();
    let answers: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(answers.len(), 3, "{text}");
    for (answer, id) in answers.iter().zip([json!(1), json!("a"), json!(3)]) {
        unavailable(answer, &id);
    }
    assert_eq!(
        tries.get(),
        3,
        "one try per request, none for the notification"
    );
}

#[test]
fn a_guard_gone_mid_session_answers_what_was_pending_and_the_next_request_reconnects() {
    let dir = Dir::new("gone");
    let listener = listen(&dir.socket());
    let (dropped_tx, dropped) = std::sync::mpsc::channel::<()>();
    let guard = thread::spawn(move || {
        let mut first = Remote::accept(&listener);
        assert!(first.line().is_some_and(|line| line.contains("\"open\"")));
        first.write(r#"{"jsonrpc":"2.0","id":"open","result":{}}"#);
        assert!(first.line().is_some_and(|line| line.contains("\"held\"")));
        // The guard goes without answering.
        drop(first);
        dropped_tx.send(()).unwrap();
        let mut second = Remote::accept(&listener);
        assert!(second.line().is_some_and(|line| line.contains("\"again\"")));
        second.write(r#"{"jsonrpc":"2.0","id":"again","result":{}}"#);
        // Gone for good, without answering "late".
        assert!(second.line().is_some_and(|line| line.contains("\"late\"")));
        drop((second, listener));
    });
    let mut bridge = Bridge::start(dir.socket(), Caps::GUARD);
    bridge.send(r#"{"jsonrpc":"2.0","id":"open","method":"tools/call"}"#);
    assert_eq!(bridge.read()["id"], "open");
    bridge.send(r#"{"jsonrpc":"2.0","id":"held","method":"tools/call"}"#);
    unavailable(&bridge.read(), &json!("held"));
    dropped.recv().unwrap();
    bridge.send(r#"{"jsonrpc":"2.0","id":"again","method":"tools/call"}"#);
    let again = bridge.read();
    assert_eq!(again["id"], "again", "{again}");
    assert!(again.get("error").is_none(), "{again}");
    bridge.send(r#"{"jsonrpc":"2.0","id":"late","method":"tools/call"}"#);
    unavailable(&bridge.read(), &json!("late"));
    guard.join().unwrap();
    // The guard is gone for good, its socket too: the next request's try fails.
    crate::write::remove_file(&dir.socket()).unwrap();
    bridge.send(r#"{"jsonrpc":"2.0","id":"after","method":"tools/call"}"#);
    unavailable(&bridge.read(), &json!("after"));
    bridge.finish();
}

/// A peer that fails the peer check (a squatter posing as the guard): the bridge sends it nothing
/// and answers the request itself, as though no guard were there.
#[test]
fn a_peer_that_fails_the_check_is_no_guard() {
    use std::io::Read;
    let (theirs, ours) = UnixStream::pair().unwrap();
    let squatter = Mutex::new(Some(ours));
    let connect = || {
        let stream = squatter.lock().unwrap().take().unwrap();
        Ok(Fake {
            stream,
            passes: false,
        })
    };
    let stdin = concat!(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#, "\n");
    let written = Arc::new(Mutex::new(Vec::<u8>::new()));
    let out: Out = Arc::<Mutex<Vec<u8>>>::clone(&written);
    serve(Cursor::new(stdin), &out, &connect, own(), Caps::GUARD).unwrap();
    let text = String::from_utf8(written.lock().unwrap().clone()).unwrap();
    unavailable(&serde_json::from_str(text.trim_end()).unwrap(), &json!(1));
    let mut sent = Vec::new();
    (&theirs).read_to_end(&mut sent).unwrap();
    assert!(sent.is_empty(), "the squatter got {sent:?}");
}

/// `--socket <path> --guard-pid <pid> --guard-pidversion <version>` exactly; both numbers
/// required, plain decimal, the pid over 1.
#[test]
fn the_guards_identity_is_required() {
    let args = |list: &[&str]| -> Vec<std::ffi::OsString> { list.iter().map(Into::into).collect() };
    let with = |pid: &str, version: &str| {
        parse(&args(&[
            "--socket",
            "/tmp/g.sock",
            "--guard-pid",
            pid,
            "--guard-pidversion",
            version,
        ]))
    };
    assert_eq!(
        with("4242", "7"),
        Ok((
            PathBuf::from("/tmp/g.sock"),
            Peer::Process(crate::transport::Identity {
                pid: 4242,
                version: 7
            })
        ))
    );
    assert!(with("4242", "0").is_ok(), "a version of 0");
    assert!(
        parse(&args(&["--socket", "/tmp/g.sock", "--guard-pid", "4242"])).is_err(),
        "no version"
    );
    assert!(
        parse(&args(&["--socket", "/tmp/g.sock"])).is_err(),
        "no pid"
    );
    for bad in ["", "0", "1", "04242", "+4242", "-1", "42x", "4294967296"] {
        assert!(with(bad, "7").is_err(), "pid {bad:?}");
    }
    for bad in ["", "07", "+7", "-7", "7x"] {
        assert!(with("4242", bad).is_err(), "version {bad:?}");
    }
}
