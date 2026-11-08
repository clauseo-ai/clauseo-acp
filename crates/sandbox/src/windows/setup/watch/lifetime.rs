//! The watcher's lifetime (the orchestrator's condition): it runs only while a client is
//! connected. With no Windows call; [`serve`](super::serve) steps it once per tick.
//!
//! - **Waiting:** from the start until the first client connects; none within
//!   [`FIRST_CLIENT_WAIT`] ends the run ([`Verdict::NoClient`]).
//! - **Serving:** while at least one client is connected.
//! - **Leaving:** once the last client has gone; one connecting within [`LAST_CLIENT_GRACE`]
//!   serves again, else the run ends ([`Verdict::LastLeft`]).
//!
//! Whatever the clients do, the run ends after [`MAX_RUN`] ([`Verdict::TooLong`]), inside the
//! task's own limit, so the watcher stops its session itself rather than be killed with it
//! running (a killed watcher leaves the session until the next start, `--remove` or a restart).

use std::time::Duration;

/// How long the watcher waits for its first client.
pub const FIRST_CLIENT_WAIT: Duration = Duration::from_secs(30);
/// How long the watcher stays after its last client left.
pub const LAST_CLIENT_GRACE: Duration = Duration::from_secs(3);
/// The longest run: an hour inside the task's 12-hour limit (`refresh::WATCH_EXECUTION_LIMIT`).
pub const MAX_RUN: Duration = Duration::from_hours(11);

/// Where the run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No client yet, since the start.
    Waiting,
    /// At least one client.
    Serving,
    /// The last client left at this time.
    Leaving(Duration),
}

/// What the run does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Go on.
    Go,
    /// No client came within [`FIRST_CLIENT_WAIT`].
    NoClient,
    /// The last client left more than [`LAST_CLIENT_GRACE`] ago.
    LastLeft,
    /// The run reached [`MAX_RUN`].
    TooLong,
}

/// The lifetime's state.
#[derive(Debug, Clone)]
pub struct Lifetime {
    began: Duration,
    phase: Phase,
    /// How many clients connected over the run, for the log line.
    served: usize,
    clients: usize,
}

impl Lifetime {
    /// A run that began at `now`.
    pub fn new(now: Duration) -> Lifetime {
        Lifetime {
            began: now,
            phase: Phase::Waiting,
            served: 0,
            clients: 0,
        }
    }

    /// How many times the client count rose over the run.
    pub fn served(&self) -> usize {
        self.served
    }

    /// One tick at `now`, with `clients` connected.
    pub fn step(&mut self, now: Duration, clients: usize) -> Verdict {
        if clients > self.clients {
            self.served += clients - self.clients;
        }
        self.clients = clients;
        if now.saturating_sub(self.began) >= MAX_RUN {
            return Verdict::TooLong;
        }
        self.phase = match (self.phase, clients) {
            (_, 1..) => Phase::Serving,
            (Phase::Waiting, 0) => {
                if now.saturating_sub(self.began) >= FIRST_CLIENT_WAIT {
                    return Verdict::NoClient;
                }
                Phase::Waiting
            }
            (Phase::Serving, 0) => Phase::Leaving(now),
            (Phase::Leaving(since), 0) => {
                if now.saturating_sub(since) >= LAST_CLIENT_GRACE {
                    return Verdict::LastLeft;
                }
                Phase::Leaving(since)
            }
        };
        Verdict::Go
    }
}
