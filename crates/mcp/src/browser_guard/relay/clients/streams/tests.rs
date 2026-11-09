//! [`FirstLine`]'s deadlines on a real socket pair, with short waits: a client dripping bytes
//! is dropped at the idle deadline, one sending a whole line before each deadline stays, the
//! deadline holds while a request awaits its answer, and a long first line in small pieces is
//! read whole.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use super::FirstLine;
use crate::browser_guard::{now, pause};

const REQUEST: &[u8] = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n";
const WAIT: Duration = Duration::from_millis(400);
const IDLE: Duration = Duration::from_millis(400);

/// A connected pair: the guard's end in a `FirstLine` (in a `BufReader`), and the client's.
fn pair() -> (BufReader<FirstLine<UnixStream>>, UnixStream) {
    let (guard, client) = UnixStream::pair().unwrap();
    (BufReader::new(FirstLine::new(guard, WAIT, IDLE)), client)
}

/// Reads lines until the connection is taken as gone; the error's kind.
fn read_until_gone(input: &mut impl BufRead) -> ErrorKind {
    let mut line = String::new();
    loop {
        line.clear();
        match input.read_line(&mut line) {
            Ok(0) => return ErrorKind::UnexpectedEof,
            Ok(_) => {}
            Err(error) => return error.kind(),
        }
    }
}

#[test]
fn a_client_dripping_bytes_is_dropped_at_the_idle_deadline() {
    let (mut input, mut client) = pair();
    client.write_all(REQUEST).unwrap();
    let drip = thread::spawn(move || {
        // A byte every 100 ms, well inside each read's bound, never a newline: for 3 s at most.
        for _ in 0..30 {
            if client.write_all(b"x").is_err() {
                return;
            }
            pause(Duration::from_millis(100));
        }
    });
    let started = now();
    assert_eq!(read_until_gone(&mut input), ErrorKind::TimedOut);
    let took = now().saturating_duration_since(started);
    assert!(
        took < Duration::from_secs(2),
        "the drip held the client {took:?}"
    );
    drop(input);
    drip.join().unwrap();
}

#[test]
fn a_client_sending_a_whole_line_before_each_deadline_stays() {
    let (mut input, mut client) = pair();
    let steady = thread::spawn(move || {
        // Six lines 250 ms apart: 1.5 s in all, past the idle bound, never 400 ms without one.
        for _ in 0..6 {
            client.write_all(REQUEST).unwrap();
            pause(Duration::from_millis(250));
        }
        client
    });
    let mut line = String::new();
    for n in 0..6 {
        line.clear();
        assert!(input.read_line(&mut line).unwrap() > 0, "line {n}");
    }
    drop(steady.join().unwrap());
}

#[test]
fn the_idle_deadline_waits_while_a_request_awaits_its_answer() {
    let (guard, mut client) = UnixStream::pair().unwrap();
    let busy = Arc::new(AtomicBool::new(true));
    let waiting = Arc::clone(&busy);
    let mut input = BufReader::new(
        FirstLine::new(guard, WAIT, IDLE).paused_while(move || waiting.load(Ordering::SeqCst)),
    );
    client.write_all(REQUEST).unwrap();
    let mut line = String::new();
    assert!(input.read_line(&mut line).unwrap() > 0);
    let started = now();
    let reader = thread::spawn(move || read_until_gone(&mut input));
    // Three idle bounds with the request unanswered, then the answer.
    pause(Duration::from_millis(1200));
    busy.store(false, Ordering::SeqCst);
    assert_eq!(reader.join().unwrap(), ErrorKind::TimedOut);
    let took = now().saturating_duration_since(started);
    assert!(
        took >= Duration::from_millis(1200) + IDLE,
        "cut off {took:?} in, before the idle bound after the answer"
    );
    drop(client);
}

#[test]
fn a_long_first_line_in_small_pieces_is_read_whole() {
    let (mut input, mut client) = pair();
    let padding = "p".repeat(200_000);
    let line = format!("{{\"jsonrpc\":\"2.0\",\"method\":\"x\",\"pad\":\"{padding}\"}}\n");
    let sent = line.clone();
    let writer = thread::spawn(move || {
        for piece in sent.as_bytes().chunks(97) {
            client.write_all(piece).unwrap();
        }
        client
    });
    let mut read = String::new();
    input.read_line(&mut read).unwrap();
    assert_eq!(read, line);
    drop(writer.join().unwrap());
}
