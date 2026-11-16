//! The turn path's timers: what each one armed with `Actor::arm_timer` is for, and where it goes
//! when it fires.

use clauseo_protocol::{ThreadId, TurnId};

use super::super::Actor;

/// A turn timer: the display and durable batches, the rate window, the cancel, a limits read.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TurnTimer {
    pub(super) thread: ThreadId,
    pub(super) kind: TimerKind,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum TimerKind {
    Display,
    Durable,
    Rate,
    Cancel(TurnId),
    LimitRead(TurnId),
    /// The wait after a connection failure ran out (`offline.rs`).
    Offline(TurnId),
    /// The quiet arm's check of a live turn (`offline/quiet.rs`).
    Quiet(TurnId),
    /// A steer's answer did not come in time (`queue/steer.rs`).
    Steer(clauseo_protocol::MessageId),
    /// The next read of Claude's transcript for unconfirmed steers (`queue/confirm.rs`).
    SteerCheck,
    /// A `stop_agents` wait ran out (`sign_in/stops.rs`), for any thread of its agent.
    StopWait(u64),
    /// An agent's start ran out of its bound (`agents/start_timer.rs`).
    Start(u64),
    /// An agent-started turn went quiet (`own_turn.rs`).
    OwnQuiet(TurnId),
    /// A cancelled agent-started turn's late window may close (`own_turn/late.rs`).
    OwnLate,
}

/// A timer armed with `Actor::arm_timer` fired.
pub(crate) fn timer(actor: &mut Actor, timer: TurnTimer) {
    let TurnTimer { thread, kind } = timer;
    match kind {
        TimerKind::Display => super::text::on_display_timer(actor, thread),
        TimerKind::Durable => super::text::on_durable_timer(actor, thread),
        TimerKind::Rate => super::updates::on_rate_timer(actor, thread),
        TimerKind::Cancel(turn) => super::cancel::on_cancel_timer(actor, thread, turn),
        TimerKind::LimitRead(turn) => super::end::on_limit_read_timer(actor, thread, turn),
        TimerKind::Offline(turn) => super::offline::on_timer(actor, thread, turn),
        TimerKind::Quiet(turn) => super::offline::quiet::on_timer(actor, thread, turn),
        TimerKind::Steer(message) => super::queue::on_steer_timer(actor, thread, message),
        TimerKind::SteerCheck => super::queue::on_steer_check_timer(actor, thread),
        TimerKind::StopWait(id) => super::sign_in::stop_timer(actor, id),
        TimerKind::OwnQuiet(turn) => super::own_turn::on_quiet(actor, thread, turn),
        TimerKind::OwnLate => super::own_turn::on_late_timer(actor, thread),
        TimerKind::Start(id) => super::agents::start_timer::on_timer(actor, thread, id),
    }
}
