//! The accept loop against a stand-in [`Listener`] whose connections are socket pairs: a peer of
//! another user is closed with nothing written, the next one is served, and a second connection
//! while it is gets [`BUSY`](super::BUSY) once, with its first request's id.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, mpsc};

use serde_json::Value;

use super::super::super::parent::Parent;
use super::super::super::session::{Launcher, Shared};
use super::{BUSY, serve};
use crate::transport::Listener;
use crate::transport::fake::Fake;

/// Hands out the connections the test queues, in order.
struct Queue(Mutex<mpsc::Receiver<Fake>>);

impl Listener for Queue {
    type Connection = Fake;

    fn accept(&self) -> io::Result<Fake> {
        let queue = self
            .0
            .lock()
            .map_err(|_poisoned| io::Error::other("poisoned"))?;
        queue
            .recv()
            .map_err(|_closed| io::Error::other("no more connections"))
    }
}

/// Queues a connection whose peer check says `same_user`; returns the test's end.
fn connect(queue: &mpsc::Sender<Fake>, same_user: bool) -> UnixStream {
    let (ours, theirs) = UnixStream::pair().unwrap();
    queue
        .send(Fake {
            stream: theirs,
            same_user,
        })
        .unwrap();
    ours
}

fn read_json(reader: &mut BufReader<UnixStream>) -> Value {
    let mut line = String::new();
    assert!(reader.read_line(&mut line).unwrap() > 0, "closed");
    serde_json::from_str(&line).unwrap()
}

#[test]
fn a_stranger_is_closed_and_a_second_client_is_answered_busy_once() {
    let dir = std::env::temp_dir().join(format!("clauseo-clients-{}", std::process::id()));
    let shared = Arc::new(Shared::new(Launcher::for_tests(&dir, Vec::new())));
    let parent = Parent::now().unwrap();
    let (queue, waiting) = mpsc::channel();
    let (events, _inbox) = mpsc::channel();
    serve(Queue(Mutex::new(waiting)), &shared, &parent, events);

    let mut stranger = connect(&queue, false);
    let mut said = Vec::new();
    stranger.read_to_end(&mut said).unwrap();
    assert!(said.is_empty(), "the stranger was told {said:?}");

    let client = connect(&queue, true);
    let mut client_reader = BufReader::new(client.try_clone().unwrap());
    (&client).write_all(b"not json\n").unwrap();
    let answer = read_json(&mut client_reader);
    assert_eq!(answer["error"]["code"], -32700, "{answer}");

    let second = connect(&queue, true);
    (&second)
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/list\"}\n")
        .unwrap();
    let mut second_reader = BufReader::new(second);
    let busy = read_json(&mut second_reader);
    assert_eq!(busy["id"], 7, "{busy}");
    assert_eq!(busy["error"]["message"], BUSY, "{busy}");
    let mut rest = String::new();
    second_reader.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "", "more than one answer");

    (&client).write_all(b"still here\n").unwrap();
    assert_eq!(read_json(&mut client_reader)["error"]["code"], -32700);
}
