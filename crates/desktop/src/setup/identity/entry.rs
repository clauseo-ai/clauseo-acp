//! One agent's identity state ([`Entry`]) and a no-email sign-in's window ([`Window`]).

use std::time::Duration;

use super::{Stamp, Verdict};

/// One agent's state.
#[derive(Debug, Default)]
pub(super) struct Entry {
    pub(super) verdict: Verdict,
    /// A sign-in or reuse made through Clauseo is under way until this time on the clock's
    /// monotonic time (its run's bound, moved on at each running state): its `SignedIn`
    /// records. Past it, the agent is checked as any other.
    pub(super) expect: Option<Duration>,
    /// The login files' stamps from before the pending sign-in's last step, so the confirming
    /// status read comes after them: what its record holds for.
    pub(super) pending: Option<Vec<Stamp>>,
    /// The login files' stamps the verdict holds for, taken before the status read that gave
    /// it. `None` only when none was taken: with an account recorded, the next look checks.
    pub(super) stamps: Option<Vec<Stamp>>,
    /// When the last check or record was, on the clock's monotonic time.
    pub(super) checked_at: Option<Duration>,
    /// Bumped at each record, sign-out, and key login's start or end, so a check that started
    /// before is dropped.
    pub(super) generation: u64,
    /// A launch of the agent passed the launch gate: from then on an account with no record is
    /// never trusted on first use.
    pub(super) launched: bool,
    /// The login files moved from `pending` while the sign-in was pending: the verdict is due
    /// (`agent.identity_checking`) until the record lands; past the bound, unknown and checked.
    pub(super) held: bool,
    /// An unknown verdict is checked again every [`UNKNOWN_AGAIN`](super::UNKNOWN_AGAIN): it
    /// came from a status that failed or timed out, or from a panic (`fail_closed`).
    pub(super) again: bool,
    /// The window of a no-email sign-in through Clauseo
    /// ([`Identities::forget`](super::Identities::forget)).
    pub(super) window: Window,
}

/// A no-email sign-in's window: while open, a check records the first account it reads (email or
/// id), launched or not, while the login files are as the sign-in left them. The record shuts it;
/// a stamp change or its bound breaks it. Trust on first use stays startup-only otherwise.
#[derive(Debug, Clone, Default)]
pub(super) enum Window {
    #[default]
    Shut,
    Open(Awaiting),
    /// No account is recorded until a sign-in through Clauseo records again, launched or not.
    Broken,
}

/// A no-email sign-in's window: until when, and the login files' stamps it left.
#[derive(Debug, Clone)]
pub(super) struct Awaiting {
    pub(super) until: Duration,
    pub(super) stamps: Vec<Stamp>,
}

impl Entry {
    /// Whether a pending sign-in through Clauseo holds at `now`.
    pub(super) fn expecting(&self, now: Duration) -> bool {
        self.expect.is_some_and(|until| now < until)
    }

    /// Whether a no-email sign-in's window lets a read at `now` record, the login files at
    /// `stamps` (after the read) and at the entry's (before it) both as the sign-in left them.
    pub(super) fn awaiting(&self, now: Duration, stamps: &[Stamp]) -> bool {
        let Window::Open(awaiting) = &self.window else {
            return false;
        };
        now < awaiting.until && awaiting.stamps == stamps && self.stamps.as_deref() == Some(stamps)
    }

    /// Breaks a no-email sign-in's window when the login files at `stamps` are not as it left
    /// them, or `now` is past its bound: nothing is recorded after. Whether it broke now.
    pub(super) fn break_window(&mut self, stamps: &[Stamp], now: Duration) -> bool {
        let broke = matches!(&self.window, Window::Open(awaiting)
            if awaiting.stamps != stamps || now >= awaiting.until);
        if broke {
            self.window = Window::Broken;
        }
        broke
    }

    /// A pending sign-in is no longer under way: whether its files had moved (`held`).
    pub(super) fn expect_end(&mut self) -> bool {
        self.expect = None;
        self.pending = None;
        std::mem::take(&mut self.held)
    }

    /// At a look or a launch while a sign-in is pending, the login files at `stamps`: when
    /// they differ from those the sign-in took, a verdict that refuses nothing becomes due,
    /// held until the record lands (R17 review M1). Whether it did now.
    pub(super) fn hold_if_moved(&mut self, stamps: &[Stamp]) -> bool {
        let rests = matches!(
            self.verdict,
            Verdict::Idle | Verdict::Same | Verdict::SignedOut
        );
        let moved = self
            .pending
            .as_deref()
            .is_none_or(|before| before != stamps);
        if rests && moved {
            self.verdict = Verdict::Due;
            self.held = true;
            self.generation += 1;
        }
        rests && moved
    }
}
