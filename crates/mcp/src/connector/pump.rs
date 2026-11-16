//! The line pump. Agent lines go to the relay unchanged, one whole line each, with the newline
//! the relay expects. Relay lines go to the agent the same way, from a thread of their own. With
//! no link, or once the link fails, each request the agent sends gets the "not reachable" error
//! and the rest of stdin is still read: the mode exits when stdin closes, never sooner.
//!
//! A request forwarded to the relay is pending (`pending.rs`) until its reply line comes back.
//! When the link ends, the relay thread answers each pending request with the same error, so an
//! agent never waits for a reply that will not come.
//!
//! The stdin side ends a link by dropping its [`Writer`]: the relay thread's read sees that at
//! once (`socket.rs`), and the socket closes when the thread lets go of it. No `shutdown` is
//! needed for that, as none wakes a blocked read on Windows; the one `shutdown` that matters is
//! the half-close at stdin's end, and if it does not reach the relay the grace still ends the link.

use std::io::{self, BufRead, BufReader};
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::MAX_LINE_BYTES;
use super::answer;
use super::handshake::{CONNECT_FAILED, Link};
use super::head::{self, Head};
use super::line::{self, Line, Out};
use super::pending::{Added, Pending};
use super::socket::{self, Polled, Shut};

/// How long, after the agent closes stdin, the mode waits for the relay to finish its last
/// answers and close. The agent has closed its side, so a longer wait only delays the exit.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// The stdin side's end of a link. Dropping it ends the link: the relay thread's read fails at
/// once, as its stop's sender is gone, and the socket closes when that thread drops it too.
struct Writer {
    socket: Arc<TcpStream>,
    stop: Sender<()>,
}

impl Writer {
    /// Tells the relay thread a request has gone out: its reader stops backing off and tries at
    /// once, so the reply is not left waiting out an idle wait.
    fn nudge(&self) {
        if self.stop.send(()).is_err() {
            // The relay thread has ended: nothing is waiting to be nudged.
        }
    }
}

/// Pumps stdin to the relay and the relay to `out` until stdin closes. `refusal` is the reason
/// the agent gets with its errors when `link` is `None`; `shut` is the `shutdown` used (tests
/// give one that does nothing). An `Err` means stdout (or stdin) failed.
///
/// When it returns, the link has been ended: the relay thread's read has failed or will at its
/// next try. On `Ok` the thread has also been waited for, within a bound: it has ended and the
/// socket is closed, unless a stdout write held it (an agent that stopped reading). Then, as on
/// an `Err`, the thread may still hold the socket, and the process's exit closes it.
pub(crate) fn serve(
    mut stdin: impl BufRead,
    out: &Out,
    link: Option<Link>,
    mut refusal: String,
    shut: Shut,
) -> io::Result<()> {
    let pending = Arc::new(Pending::default());
    let (mut writer, relay_done) = match link {
        Some(link) => {
            let (writer, done) = start_relay(link, out, &pending, shut);
            (Some(writer), Some(done))
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
                writer = None;
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
    if let Some(link) = &writer
        && shut(&link.socket, Shutdown::Write).is_err()
    {
        // Already gone, or the relay never sees it: the grace below ends the link either way.
    }
    if let Some(done) = relay_done {
        wait_for_relay(&done, EXIT_GRACE);
        // Done or out of time: end the link, and wait for the relay thread to let go of the
        // socket. Its read fails at once; what is left is writing its last answers to stdout,
        // which a stuck stdout can hold for ever, so this wait is bounded too.
        drop(writer);
        wait_for_relay(&done, EXIT_GRACE);
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
    writer: &mut Option<Writer>,
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
            *writer = None;
            return line::write_line(out, answer::unreachable_for(id, refusal).as_bytes());
        }
    }
    if forward(writer, line) {
        if let Some(link) = writer {
            link.nudge();
        }
        return Ok(());
    }
    if pending.release(id) {
        line::write_line(out, answer::unreachable_for(id, refusal).as_bytes())?;
    }
    Ok(())
}

/// Writes one line to the relay. `false` when there is no link or the write fails; a failed link
/// is dropped here, so no later line goes into it.
fn forward(writer: &mut Option<Writer>, line: &[u8]) -> bool {
    let Some(link) = writer.as_ref() else {
        return false;
    };
    if send(&link.socket, line).is_ok() {
        return true;
    }
    *writer = None;
    false
}

/// Writes `line` and a newline.
fn send(socket: &TcpStream, line: &[u8]) -> io::Result<()> {
    socket::write_all(socket, line, None)?;
    socket::write_all(socket, b"\n", None)
}

/// Starts the relay thread on `link`'s reader, and returns the stdin side's [`Writer`] with the
/// receiver whose disconnect says the thread has ended.
fn start_relay(
    link: Link,
    out: &Out,
    pending: &Arc<Pending>,
    shut: Shut,
) -> (Writer, Receiver<()>) {
    let Link { mut reader, socket } = link;
    let (stop, stopped) = mpsc::channel::<()>();
    reader.get_mut().stop_on(stopped);
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let (out, pending) = (Arc::clone(out), Arc::clone(pending));
    thread::spawn(move || {
        relay_to_agent(reader, &out, &pending, shut);
        // The sender's drop is the signal: the receiver sees the disconnect.
        drop(done_tx);
    });
    let writer = Writer { socket, stop };
    (writer, done_rx)
}

/// Copies the relay's lines to the agent until the relay ends, the stdin side ends the link, or
/// a write fails. A reply releases its pending request. Then the link is shut down and this
/// thread's handle on the socket dropped, and each request still pending is answered with the
/// not-reachable error; the set is closed, so the stdin side answers any later one.
fn relay_to_agent(mut reader: BufReader<Polled>, out: &Out, pending: &Pending, shut: Shut) {
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
    // The shutdown tells the relay at once, and makes the stdin side's next write fail; the link
    // ends without it too, when the stdin side drops its handle.
    if shut(reader.get_ref().socket(), Shutdown::Both).is_err() {
        // Already gone.
    }
    drop(reader);
    for id in pending.close() {
        if line::write_line(out, answer::unreachable_for(&id, CONNECT_FAILED).as_bytes()).is_err() {
            break;
        }
    }
}

/// Waits up to `within` for the relay thread to end, which its sender's drop says. It ends when
/// the relay closes, or when the link is ended and nothing holds it in a stdout write.
fn wait_for_relay(done: &Receiver<()>, within: Duration) {
    if done.recv_timeout(within).is_err() {
        // The thread has ended (the sender is gone) or did not within the bound: go on either way.
    }
}

#[cfg(test)]
mod tests;
