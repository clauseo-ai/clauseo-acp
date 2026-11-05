//! The polled socket: a read ends on its limit with no `shutdown` and no help from the peer, and
//! a reply split across two writes is read whole.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use super::super::line::{self, Line};
use super::{FIRST_WAIT, IDLE_WAIT, Polled, stop_after, write_all};

/// How long a test waits for a read to end before it fails: far past the limits under test.
const BOUND: Duration = Duration::from_secs(10);

/// A connected pair: the mode's socket and the peer's.
fn pair() -> (Arc<TcpStream>, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    (Arc::new(socket), peer)
}

#[test]
fn dropping_the_stop_ends_a_read_that_has_no_data_without_a_shutdown() {
    let (socket, peer) = pair();
    let (stop, stopped) = mpsc::channel::<()>();
    let mut reader = Polled::new(Arc::clone(&socket), stopped).unwrap();
    let (started_tx, started) = mpsc::channel::<()>();
    let (result_tx, result) = mpsc::channel();
    thread::spawn(move || {
        started_tx.send(()).unwrap();
        let mut buf = [0_u8; 16];
        result_tx.send(reader.read(&mut buf)).unwrap();
    });
    started.recv().unwrap();
    // The peer stays open and silent; nothing calls `shutdown`. Only the stop's drop ends it.
    drop(stop);
    let read = result
        .recv_timeout(BOUND)
        .expect("the read went on after the stop was dropped");
    assert_eq!(read.unwrap_err().kind(), ErrorKind::ConnectionAborted);
    // The reader thread has let go of its handle; the last one closes the connection.
    drop(socket);
    let mut peer = peer;
    peer.set_read_timeout(Some(BOUND)).unwrap();
    assert_eq!(peer.read(&mut [0_u8; 1]).unwrap(), 0, "the peer saw no end");
}

#[test]
fn a_deadline_ends_a_read_from_a_silent_peer() {
    let (socket, _peer) = pair();
    let (_timer, deadline) = stop_after(Duration::from_millis(50));
    let mut reader = Polled::new(socket, deadline).unwrap();
    let (result_tx, result) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = [0_u8; 16];
        result_tx.send(reader.read(&mut buf)).unwrap();
    });
    let read = result
        .recv_timeout(BOUND)
        .expect("the read went on past its deadline");
    assert_eq!(read.unwrap_err().kind(), ErrorKind::ConnectionAborted);
}

#[test]
fn a_line_split_across_two_writes_with_nothing_after_it_is_read_whole() {
    let (socket, mut peer) = pair();
    let (_stop, stopped) = mpsc::channel::<()>();
    let mut reader = BufReader::new(Polled::new(Arc::clone(&socket), stopped).unwrap());
    // The handshake reply as the test relay writes it: the text, then its newline, apart.
    let (half_tx, half) = mpsc::channel::<()>();
    let writer = thread::spawn(move || {
        peer.write_all(br#"{"clauseo_relay":1,"ok":true}"#).unwrap();
        half.recv().unwrap();
        peer.write_all(b"\n").unwrap();
        peer
    });
    // The text alone is in the reader's buffer before the newline is written: the read must wait
    // for the newline, keeping what it has, instead of returning a part or dropping it.
    assert_ne!(reader.fill_buf().unwrap(), b"");
    assert!(!reader.buffer().contains(&b'\n'));
    half_tx.send(()).unwrap();
    assert_eq!(
        line::read(&mut reader, 1024).unwrap(),
        Line::Text(br#"{"clauseo_relay":1,"ok":true}"#.to_vec())
    );
    let _peer = writer.join().unwrap();
    // The same socket takes a write after the reads: no timeout fired, nothing to reset.
    write_all(&socket, b"after\n", None).unwrap();
}

#[test]
fn a_nudge_ends_an_idle_wait_at_once_and_resets_it_to_the_first() {
    let (socket, _peer) = pair();
    let (nudge, stop) = mpsc::channel::<()>();
    let mut reader = Polled::new(socket, stop).unwrap();
    // Idle until the wait is the longest: 1, 2, 4, 8 and 16 ms, then 20.
    while reader.backoff() < IDLE_WAIT {
        reader.pause().unwrap();
    }
    nudge.send(()).unwrap();
    reader.pause().unwrap();
    assert_eq!(
        reader.backoff(),
        FIRST_WAIT,
        "the nudge did not reset the wait"
    );
}
