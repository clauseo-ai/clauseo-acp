//! The events ETW itself lost, with no Windows call: every [`QUERY_EVERY`] the trace queries its
//! session (`ControlTraceW` with `EVENT_TRACE_CONTROL_QUERY`) and adds what `EventsLost` and
//! `RealTimeBuffersLost` grew by to the clients' `dropped` count. A lost event may have been a
//! pool user's refusal, so the client is told, as it is of the rate's drops.

use std::time::Duration;

/// How often the session is queried.
pub const QUERY_EVERY: Duration = Duration::from_secs(5);

/// The session's loss counters as last read.
#[derive(Debug, Clone, Default)]
pub struct Losses {
    events: u32,
    buffers: u32,
    queried_at: Option<Duration>,
}

impl Losses {
    /// Whether the session is due to be queried at `now`.
    pub fn due(&self, now: Duration) -> bool {
        self.queried_at
            .is_none_or(|at| now.saturating_sub(at) >= QUERY_EVERY)
    }

    /// The counters read at `now`: how much they grew since the last read. A counter lower than
    /// before (a session started again) counts from zero.
    pub fn read(&mut self, now: Duration, events: u32, buffers: u32) -> u64 {
        let grew = |was: u32, is: u32| u64::from(is.checked_sub(was).unwrap_or(is));
        let delta = grew(self.events, events) + grew(self.buffers, buffers);
        self.events = events;
        self.buffers = buffers;
        self.queried_at = Some(now);
        delta
    }
}
