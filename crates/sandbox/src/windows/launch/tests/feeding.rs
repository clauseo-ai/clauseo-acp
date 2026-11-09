//! The denial pipe's feed over the fake pipe: records in order, the outer alone, and nothing
//! after the outer's exit.

use std::sync::Mutex;

use super::pipe::FakePipe;
use super::*;
use crate::{Denial, DenialSink, SetAsideWhat};

/// Records the denials it gets, in order.
#[derive(Default)]
pub(super) struct Denials(pub(super) Mutex<Vec<Denial>>);

impl DenialSink for Denials {
    fn denied(&self, d: Denial) {
        self.0.lock().unwrap().push(d);
    }
}

/// A sink that panics on every denial.
struct Panics;

impl DenialSink for Panics {
    fn denied(&self, _denial: Denial) {
        panic!("a bad sink");
    }
}

/// One record of the stand-in format.
fn record(path: &str, to: &str, what: &str) -> Vec<u8> {
    format!(r#"{{"kind":"set_aside","path":"{path}","to":"{to}","what":"{what}"}}"#).into_bytes()
}

/// The outer, as the fake pipe opens it.
const OUTER: Peer = Peer {
    pid: 4242,
    started: 133_000_000_000_000_000,
};

/// A pipe the outer opened, holding `chunks`.
fn from_outer(chunks: Vec<Vec<u8>>) -> FakePipe {
    FakePipe {
        outer: Some(OUTER),
        client: Some(OUTER),
        chunks: chunks.into(),
        read_fails: false,
        exits_after: None,
        adopts: false,
        world: None,
    }
}

/// The outer's records reach the sink in order, however the reads split them; a line that
/// isn't a record (another kind, bad JSON, the old stderr line, one over the cap, one cut off
/// at the end) is dropped, and a sink that panics doesn't end the feed.
#[test]
fn denial_records_reach_the_sink_in_order_and_others_dont() {
    let git = record(".git", r"set-aside\\1\\.git", "git_repository");
    let link = record("link", r"set-aside\\2\\link", "link_out");
    let mut stream = Vec::new();
    for line in [
        git.clone(),
        br#"{"kind":"notice","words":"hello"}"#.to_vec(),
        b"not json".to_vec(),
        format!(
            "clauseo-sandbox-run: set aside\tfoo\tbar\t{}",
            SetAsideWhat::LinkOut.why()
        )
        .into_bytes(),
        record(&"x".repeat(70 * 1024), "y", "link_out"),
        link,
    ] {
        stream.extend_from_slice(&line);
        stream.push(b'\n');
    }
    stream.extend_from_slice(&git[..10]);
    // Reads of 7 bytes, so records and the line over the cap come in pieces.
    let chunks = stream.chunks(7).map(<[u8]>::to_vec).collect();
    let sink = Denials::default();
    let mut connected = 0;
    let fed = feed::feed(from_outer(chunks), OUTER, &sink, || connected += 1);
    assert_eq!(connected, 1, "the outer connected once");
    assert_eq!(
        fed,
        feed::Fed::Ended {
            passed: 2,
            dropped: 5
        }
    );
    let set_aside = |path: &str, to: &str, what| Denial::SetAside {
        path: path.into(),
        to: to.into(),
        what,
    };
    assert_eq!(
        *sink.0.lock().unwrap(),
        [
            set_aside(".git", r"set-aside\1\.git", SetAsideWhat::GitRepository),
            set_aside("link", r"set-aside\2\link", SetAsideWhat::LinkOut),
        ]
    );
    let mut ends_badly = from_outer(vec![git.clone(), b"\n".to_vec()]);
    ends_badly.read_fails = true;
    assert_eq!(
        feed::feed(ends_badly, OUTER, &Panics, || {}),
        feed::Fed::Ended {
            passed: 1,
            dropped: 0
        }
    );
}

/// Only the outer is read: a client with another pid, or the outer's pid with another
/// creation time (a reused id), is refused before a byte is read. No client, or an outer that
/// can't be opened, feeds nothing either.
#[test]
fn the_pipe_reads_the_outer_alone() {
    let lines = vec![[record("a", "b", "link_out"), b"\n".to_vec()].concat()];
    for client in [
        Peer { pid: 7, ..OUTER },
        Peer {
            started: OUTER.started + 1,
            ..OUTER
        },
    ] {
        let sink = Denials::default();
        let pipe = FakePipe {
            client: Some(client),
            ..from_outer(lines.clone())
        };
        let fed = feed::feed(pipe, OUTER, &sink, || panic!("not the outer"));
        assert_eq!(fed, feed::Fed::Refused);
        assert_eq!(*sink.0.lock().unwrap(), []);
    }
    let sink = Denials::default();
    let nobody = FakePipe {
        client: None,
        ..from_outer(lines.clone())
    };
    let fed = feed::feed(nobody, OUTER, &sink, || panic!("nobody connected"));
    assert_eq!(fed, feed::Fed::NoClient);
    let gone = from_outer(lines);
    let mut gone = gone;
    assert_eq!(feed::watch(&mut gone, OUTER.pid + 1), None);
    assert_eq!(feed::watch(&mut gone, OUTER.pid), Some(OUTER));
    assert_eq!(*sink.0.lock().unwrap(), []);
}

/// The outer's exit ends the feed: a record that comes after it, and one it cut off, is never
/// delivered, though the pipe still holds them.
#[test]
fn records_after_the_outers_exit_are_dropped() {
    let line = |path: &str| [record(path, "aside", "link_out"), b"\n".to_vec()].concat();
    let cut = record("cut", "aside", "link_out");
    let chunks = vec![
        line("before"),
        cut[..8].to_vec(),
        cut[8..].to_vec(),
        line("after"),
    ];
    let sink = Denials::default();
    let pipe = FakePipe {
        exits_after: Some(2),
        ..from_outer(chunks)
    };
    assert_eq!(
        feed::feed(pipe, OUTER, &sink, || {}),
        feed::Fed::Exited {
            passed: 1,
            dropped: 1
        }
    );
    assert_eq!(
        *sink.0.lock().unwrap(),
        [Denial::SetAside {
            path: "before".into(),
            to: "aside".into(),
            what: SetAsideWhat::LinkOut,
        }]
    );
}
