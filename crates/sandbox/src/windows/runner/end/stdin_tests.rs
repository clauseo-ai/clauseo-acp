//! The host's stdin end is kept when it is read, not once the agent took what came before it:
//! with an agent that never reads its stdin, the deadline starts at the end of a large write, and
//! the job's end is what unblocks the writer. [`Fed`] serves `tests.rs` too.

use std::io;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;

/// The test's clock.
#[expect(
    clippy::disallowed_methods,
    reason = "the test measures how soon after the host's stdin end the deadline starts"
)]
fn clock() -> Instant {
    Instant::now()
}

/// Every event, with its time since the test began.
type Events = Arc<Mutex<Vec<(&'static str, Duration)>>>;

fn note(events: &Events, origin: Instant, what: &'static str) {
    events
        .lock()
        .unwrap()
        .push((what, clock().saturating_duration_since(origin)));
}

/// Real-clock steps: the job's end lets the blocked write fail, as the agent's end does to the
/// pipe on Windows.
struct Steps {
    origin: Instant,
    events: Events,
    unblock: Mutex<Option<mpsc::Sender<()>>>,
}

impl SweeperSteps for Steps {
    type Sweeper = ();
    fn empty_job(&self) -> Result<(), String> {
        note(&self.events, self.origin, "job ended");
        drop(self.unblock.lock().unwrap().take());
        Ok(())
    }
    fn log_on(&self) -> Result<(), String> {
        Ok(())
    }
    fn shut(&self, (): &()) -> Result<(), String> {
        Ok(())
    }
    fn resume(&self, (): &()) -> Result<(), String> {
        Ok(())
    }
    fn kill(&self, (): &()) {}
    fn keep(&self, (): ()) {}
    fn log(&self, _: &str) {}
    fn now(&self) -> Duration {
        clock().saturating_duration_since(self.origin)
    }
    fn wait(&self) -> Swept {
        Swept::Unclean
    }
}

/// 8 MiB, then the end; the end's time is noted.
struct Large {
    left: usize,
    origin: Instant,
    events: Events,
}

impl io::Read for Large {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 {
            note(&self.events, self.origin, "stdin end");
            return Ok(0);
        }
        let len = buf.len().min(self.left);
        buf[..len].fill(b'x');
        self.left -= len;
        Ok(len)
    }
}

/// A source fed piece by piece; it ends once the feed is dropped.
pub(super) struct Fed(pub(super) mpsc::Receiver<Vec<u8>>);

impl io::Read for Fed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Ok(piece) = self.0.recv() else {
            return Ok(0);
        };
        let len = piece.len().min(buf.len());
        buf[..len].copy_from_slice(&piece[..len]);
        Ok(len)
    }
}

/// An agent that never reads: the first write blocks until the job ends, then fails.
struct Stuck {
    unblocked: mpsc::Receiver<()>,
    origin: Instant,
    events: Events,
}

impl io::Write for Stuck {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        // Nothing is ever sent: the job's end drops the sender.
        let _ended = self.unblocked.recv_timeout(Duration::from_secs(10));
        note(&self.events, self.origin, "write failed");
        Err(io::Error::from(io::ErrorKind::BrokenPipe))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_deadline_starts_at_the_hosts_stdin_end_while_the_write_to_the_agent_is_blocked() {
    let origin = clock();
    let events = Events::default();
    let (unblock, unblocked) = mpsc::channel();
    let end = EndSweep::new(Steps {
        origin,
        events: Arc::clone(&events),
        unblock: Mutex::new(Some(unblock)),
    });
    let from = Large {
        left: 8 << 20,
        origin,
        events: Arc::clone(&events),
    };
    let to = Stuck {
        unblocked,
        origin,
        events: Arc::clone(&events),
    };
    let grace = Duration::from_millis(300);
    let closing = Arc::clone(&events);
    relay_stdin(from, to, &end, grace, move |_| {
        note(&closing, origin, "agent stdin closed");
    });
    let events = events.lock().unwrap().clone();
    let at = |what: &str| {
        events
            .iter()
            .find(|(event, _)| *event == what)
            .map_or_else(|| panic!("no {what}: {events:?}"), |(_, at)| *at)
    };
    let deadline = *end.stdin_ended.get().expect("the stdin end was kept");
    // Kept within 100 ms of the end's read, before the write could finish.
    let late = deadline.saturating_sub(at("stdin end"));
    assert!(
        late < Duration::from_millis(100),
        "{late:?} late: {events:?}"
    );
    assert!(deadline < at("write failed"), "{events:?}");
    // The grace counted from the stdin end; the job's end, then the failed write and the close.
    assert!(at("job ended") >= deadline + grace, "{events:?}");
    assert!(at("job ended") <= at("write failed"), "{events:?}");
    assert!(at("write failed") <= at("agent stdin closed"), "{events:?}");
    assert_eq!(end.began(), Some(EndSignal::Stdin));
}
