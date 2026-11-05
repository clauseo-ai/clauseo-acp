//! The line pump. Agent lines go to the relay unchanged, one whole line each, with the newline
//! the relay expects. Relay lines go to the agent the same way, from a thread of their own. With
//! no link, or once the link fails, each request the agent sends gets the "not reachable" error
//! and the rest of stdin is still read: the mode exits when stdin closes, never sooner.
//!
//! A request forwarded to the relay is pending (`pending.rs`) until its reply line comes back.
//! When the link ends, the relay thread answers each pending request with the same error, so an
//! agent never waits for a reply that will not come.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::MAX_LINE_BYTES;
use super::answer;
use super::handshake::{CONNECT_FAILED, Link};
use super::head::{self, Head};
use super::line::{self, Line, Out};
use super::pending::{Added, Pending};

/// How long, after the agent closes stdin, the mode waits for the relay to finish its last
/// answers and close. The agent has closed its side, so a longer wait only delays the exit.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// Pumps stdin to the relay and the relay to `out` until stdin closes. `refusal` is the reason
/// the agent gets with its errors when `link` is `None`. An `Err` means stdout (or stdin) failed.
pub(crate) fn serve(
    mut stdin: impl BufRead,
    out: &Out,
    link: Option<Link>,
    mut refusal: String,
) -> io::Result<()> {
    let pending = Arc::new(Pending::default());
    let (mut writer, relay_done) = match link {
        Some(Link { reader, writer }) => {
            let (done_tx, done_rx) = mpsc::channel::<()>();
            let (out, pending) = (Arc::clone(out), Arc::clone(&pending));
            thread::spawn(move || {
                relay_to_agent(reader, &out, &pending);
                // The sender's drop is the signal: the receiver sees the disconnect.
                drop(done_tx);
            });
            (Some(writer), Some(done_rx))
        }
        None => (None, None),
    };
    loop {
        match line::read(&mut stdin, MAX_LINE_BYTES)? {
            Line::Eof => break,
            Line::TooLong => {
                // The host would close on such a line too; the link ends here. Its pending
                // requests are answered by the relay thread, and the agent's later requests get
                // the not-reachable errors.
                line::write_line(out, answer::line_too_long().as_bytes())?;
                if let Some(socket) = writer.take() {
                    hang_up(&socket);
                }
                refusal = String::from(CONNECT_FAILED);
            }
            Line::Text(text) if line::is_blank(&text) => {}
            Line::Text(text) => match (writer.is_some(), request_id(&text)) {
                (true, Some(id)) => {
                    send_request(&mut writer, out, &pending, &refusal, &id, &text)?;
                }
                _ => {
                    if !forward(&mut writer, &text) {
                        for reply in answer::not_reachable(&text, &refusal) {
                            line::write_line(out, reply.as_bytes())?;
                        }
                    }
                }
            },
        }
    }
    // The agent is done: half-close so the relay can finish its last answers, then wait for it.
    if let Some(socket) = writer.take() {
        shut_write(&socket);
    }
    if let Some(done) = relay_done {
        wait_for_relay(&done);
    }
    // A relay that did not close within the grace leaves requests unanswered: answer them now.
    // Closing the set is atomic, so the relay thread, if it ends later, answers none of them again.
    for id in pending.close() {
        line::write_line(out, answer::unreachable_for(&id, CONNECT_FAILED).as_bytes())?;
    }
    Ok(())
}

/// The `id` of a request (a line with a `method` and an `id`); `None` for anything else.
fn request_id(line: &[u8]) -> Option<Value> {
    head::read(line).and_then(|head| if head.method { head.id } else { None })
}

/// Forwards one request, and keeps it pending until its reply. One past the cap is answered here
/// and not forwarded. A request the link refuses is answered here too, unless the relay thread
/// has already answered it: [`Pending::release`] says which side took it. A request that comes
/// after the relay thread's last answers (the set is closed) is answered here and not forwarded:
/// a write into a link the relay has closed may still succeed, and no one would answer it.
fn send_request(
    writer: &mut Option<TcpStream>,
    out: &Out,
    pending: &Pending,
    refusal: &str,
    id: &Value,
    line: &[u8],
) -> io::Result<()> {
    match pending.add(id) {
        Added::Pending => {}
        Added::Full => return line::write_line(out, answer::busy_for(id).as_bytes()),
        Added::Closed => {
            // The link is over; later lines take the no-link path.
            if let Some(socket) = writer.take() {
                hang_up(&socket);
            }
            return line::write_line(out, answer::unreachable_for(id, refusal).as_bytes());
        }
    }
    if forward(writer, line) {
        return Ok(());
    }
    if pending.release(id) {
        line::write_line(out, answer::unreachable_for(id, refusal).as_bytes())?;
    }
    Ok(())
}

/// Writes one line to the relay. `false` when there is no link or the write fails; a failed link
/// is dropped here, so no later line goes into it.
fn forward(writer: &mut Option<TcpStream>, line: &[u8]) -> bool {
    let Some(socket) = writer.as_ref() else {
        return false;
    };
    if send(socket, line).is_ok() {
        return true;
    }
    if let Some(socket) = writer.take() {
        hang_up(&socket);
    }
    false
}

/// Writes `line` and a newline, flushed.
fn send(socket: &TcpStream, line: &[u8]) -> io::Result<()> {
    let mut writer = socket;
    writer.write_all(line)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Ends both halves of a link, so its reader ends too.
fn hang_up(socket: &TcpStream) {
    if socket.shutdown(Shutdown::Both).is_err() {
        // Already gone: the reader sees that and ends.
    }
}

/// Ends the write half of a link: the relay sees the agent is done and may finish its answers.
fn shut_write(socket: &TcpStream) {
    if socket.shutdown(Shutdown::Write).is_err() {
        // Already gone: nothing is left to finish.
    }
}

/// Copies the relay's lines to the agent until the relay ends or a write fails. A reply releases
/// its pending request. Then the link is shut down, and each request still pending is answered
/// with the not-reachable error; the set is closed, so the stdin side answers any later one.
fn relay_to_agent(mut reader: BufReader<TcpStream>, out: &Out, pending: &Pending) {
    // The end, a read error, or a line over the cap: the link is gone either way.
    while let Ok(Line::Text(text)) = line::read(&mut reader, MAX_LINE_BYTES) {
        if line::is_blank(&text) {
            continue;
        }
        if let Some(Head {
            id: Some(id),
            method: false,
        }) = head::read(&text)
        {
            pending.release(&id);
        }
        if line::write_line(out, &text).is_err() {
            break;
        }
    }
    hang_up(reader.get_ref());
    for id in pending.close() {
        if line::write_line(out, answer::unreachable_for(&id, CONNECT_FAILED).as_bytes()).is_err() {
            break;
        }
    }
}

/// Waits up to [`EXIT_GRACE`] for the relay thread to end. It ends when the relay closes, or when
/// the grace runs out; either way the mode exits.
fn wait_for_relay(done: &Receiver<()>) {
    if done.recv_timeout(EXIT_GRACE).is_err() {
        // The relay closed (the sender is gone) or did not close in time: exit either way.
    }
}

#[cfg(test)]
mod tests;
