//! clauseo-reducer's instruction counts under Valgrind (gungraun, Callgrind): folding a busy
//! turn, one event or one text delta on a long thread, a 20,000-event replay and the view
//! snapshot of that thread.
//!
//! Each case builds its events and its state in gungraun's `setup`, which Callgrind does not
//! count, so only the fold is counted. Hard limits are below; the soft limit (2% more `ir` than
//! the `main` baseline) applies when the run passes `--baseline=main`, as CI's `reducer-bench`
//! job does. On systems other than Linux, `main` says gungraun needs Valgrind and exits 0.

#[cfg(target_os = "linux")]
#[path = "../../reducer/tests/support/fixtures.rs"]
pub mod fixtures;

#[cfg(target_os = "linux")]
#[path = "../../reducer/tests/support/events.rs"]
pub mod events;

#[cfg(target_os = "linux")]
mod linux {
    use std::fmt::Display;
    use std::hint::black_box;

    use clauseo_protocol::{DomainEvent, Envelope, MessageId, Seq, TextKind, TurnId, ViewWindow};
    use clauseo_reducer::{
        Applied, Error, StreamState, TextDelta, apply, apply_text_delta, view_snapshot,
    };
    use gungraun::{
        Callgrind, EventKind, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group,
    };

    use crate::fixtures::{self, Fx, OpenTurn, SCRIPTED_TURN_EVENTS};

    /// 50,000 instructions stand for 50 µs: the reference laptops (i5-8250U, i5-1135G7) retire
    /// at least one instruction per nanosecond on code like this, even at an IPC of 0.5 and
    /// 2 GHz. The budget is 50 µs per event (`reducer_apply` in budgets.toml).
    const IR_PER_EVENT: u64 = 50_000;

    /// The hard limit of `scripted_turn`: [`IR_PER_EVENT`] for each of its events. The setup
    /// fails the run when the fixture's length differs from [`SCRIPTED_TURN_EVENTS`].
    const SCRIPTED_TURN_IR: u64 = IR_PER_EVENT * SCRIPTED_TURN_EVENTS as u64;

    /// Events in the long thread the one-event cases, `replay_20k` and `view_snapshot_20k` use.
    const LONG_THREAD_EVENTS: usize = 20_000;

    /// The soft limit, in percent more `ir` than the baseline the run compares against.
    const SOFT_IR_PERCENT: f64 = 2.0;

    /// Callgrind with the soft limit and, when given, a hard limit on `ir`.
    fn limits(hard_ir: Option<u64>) -> LibraryBenchmarkConfig {
        let mut callgrind = Callgrind::default();
        callgrind.soft_limits([(EventKind::Ir, SOFT_IR_PERCENT)]);
        if let Some(limit) = hard_ir {
            callgrind.hard_limits([(EventKind::Ir, limit)]);
        }
        let mut config = LibraryBenchmarkConfig::default();
        config.tool(callgrind);
        config
    }

    /// Stops the run: a case whose input cannot be built must fail, never measure something
    /// else.
    #[expect(
        clippy::panic,
        reason = "a bench whose input cannot be built must fail the run"
    )]
    fn fail(what: &str) -> ! {
        panic!("apply bench setup: {what}")
    }

    /// The value of `result`, or a failed run that says what could not be built.
    fn built<T, E: Display>(what: &str, result: Result<T, E>) -> T {
        result.unwrap_or_else(|error| fail(&format!("{what}: {error}")))
    }

    /// The state `events` fold to, from the stream of the first.
    fn fold(events: &[Envelope]) -> StreamState {
        let Some(first) = events.first() else {
            fail("no events to fold")
        };
        let mut state = StreamState::new(first.stream);
        for envelope in events {
            built("folding the setup's events", apply(&mut state, envelope));
        }
        state
    }

    /// A fresh state and the events to fold into it.
    struct Fold {
        state: StreamState,
        events: Vec<Envelope>,
    }

    /// A state and the one event to apply to it.
    struct OneEvent {
        state: StreamState,
        next: Envelope,
    }

    /// A state and a delta's text for its open answer, with the host's pending head.
    struct OneDelta {
        state: StreamState,
        turn: TurnId,
        message: MessageId,
        text: String,
        pending_seq: Seq,
    }

    fn fresh(events: Vec<Envelope>) -> Fold {
        let Some(first) = events.first() else {
            fail("no events to fold")
        };
        Fold {
            state: StreamState::new(first.stream),
            events,
        }
    }

    fn scripted_turn_input() -> Fold {
        let events = built("scripted_turn", fixtures::scripted_turn());
        if events.len() != SCRIPTED_TURN_EVENTS {
            fail(&format!(
                "scripted_turn has {} events, SCRIPTED_TURN_EVENTS says {SCRIPTED_TURN_EVENTS}",
                events.len()
            ));
        }
        fresh(events)
    }

    fn replay_input() -> Fold {
        fresh(built(
            "long_thread",
            fixtures::long_thread(LONG_THREAD_EVENTS),
        ))
    }

    fn long_state() -> StreamState {
        fold(&built(
            "long_thread",
            fixtures::long_thread(LONG_THREAD_EVENTS),
        ))
    }

    /// The long thread with an open turn whose answer holds `answer_bytes`, folded, and the
    /// event `next` builds, at the next seq.
    fn on_open_turn(
        answer_bytes: usize,
        next: impl FnOnce(&mut OpenTurn) -> Fx<DomainEvent>,
    ) -> OneEvent {
        let mut open = built(
            "the open turn",
            OpenTurn::on_long_thread(LONG_THREAD_EVENTS, answer_bytes),
        );
        let state = fold(open.log.events());
        let event = built("the next event", next(&mut open));
        let next = built("its envelope", open.log.push(event));
        OneEvent { state, next }
    }

    fn text_on_8k() -> OneEvent {
        on_open_turn(8 * 1024, |open| Ok(open.text_appended()))
    }

    fn text_on_256k() -> OneEvent {
        on_open_turn(256 * 1024, |open| Ok(open.text_appended()))
    }

    /// The long thread with an open turn whose answer holds 256 KiB, and a 40-byte delta onto
    /// that answer: the streaming hot path, where `from_len` must come from the cached count.
    fn delta_on_256k() -> OneDelta {
        let open = built(
            "the open turn",
            OpenTurn::on_long_thread(LONG_THREAD_EVENTS, 256 * 1024),
        );
        let state = fold(open.log.events());
        let Some(pending_seq) = state.seq() else {
            fail("the open turn folded to an empty state")
        };
        OneDelta {
            state,
            turn: open.turn,
            message: open.answer,
            text: fixtures::prose(40),
            pending_seq,
        }
    }

    fn tool_call_updated() -> OneEvent {
        on_open_turn(8 * 1024, |open| open.tool_call_updated())
    }

    fn permission_requested() -> OneEvent {
        on_open_turn(8 * 1024, OpenTurn::permission_requested)
    }

    fn turn_ended() -> OneEvent {
        on_open_turn(8 * 1024, OpenTurn::turn_ended)
    }

    // Folds every event; returns the input so that dropping it is not counted.
    #[library_benchmark]
    #[bench::scripted_turn(setup = scripted_turn_input, config = limits(Some(SCRIPTED_TURN_IR)))]
    #[bench::replay_20k(setup = replay_input, config = limits(None))]
    fn fold_events(mut input: Fold) -> Fold {
        for envelope in &input.events {
            let applied: Result<Applied, Error> = apply(&mut input.state, envelope);
            drop(black_box(applied));
        }
        input
    }

    // Applies one event to the long thread.
    #[library_benchmark]
    #[bench::text_appended_8k(setup = text_on_8k, config = limits(Some(IR_PER_EVENT)))]
    #[bench::text_appended_256k(setup = text_on_256k, config = limits(Some(IR_PER_EVENT)))]
    #[bench::tool_call_updated(setup = tool_call_updated, config = limits(Some(IR_PER_EVENT)))]
    #[bench::permission_requested(setup = permission_requested, config = limits(Some(IR_PER_EVENT)))]
    #[bench::turn_ended(setup = turn_ended, config = limits(Some(IR_PER_EVENT)))]
    fn one_event_on_long_thread(mut input: OneEvent) -> (OneEvent, Result<Applied, Error>) {
        let applied = apply(&mut input.state, &input.next);
        (input, applied)
    }

    // Applies one text delta to the long thread's open answer.
    #[library_benchmark]
    #[bench::text_delta_256k(setup = delta_on_256k, config = limits(Some(IR_PER_EVENT)))]
    fn text_delta_on_long_thread(mut input: OneDelta) -> (OneDelta, Result<Applied, Error>) {
        let delta = TextDelta {
            turn: input.turn,
            message: input.message,
            kind: TextKind::Answer,
            text: &input.text,
            pending_seq: input.pending_seq,
        };
        let Some(thread) = input.state.as_thread_mut() else {
            fail("the open turn's state is not a thread")
        };
        let applied = apply_text_delta(thread, &delta);
        (input, applied)
    }

    // The view snapshot of the long thread.
    #[library_benchmark]
    #[bench::view_snapshot_20k(setup = long_state, config = limits(None))]
    fn view_snapshot_20k(state: StreamState) -> (StreamState, ViewWindow) {
        let window = view_snapshot(&state);
        (state, window)
    }

    library_benchmark_group!(
        name = apply_group,
        benchmarks = [
            fold_events,
            one_event_on_long_thread,
            text_delta_on_long_thread,
            view_snapshot_20k
        ]
    );
}

#[cfg(target_os = "linux")]
use linux::apply_group;

#[cfg(target_os = "linux")]
gungraun::main!(library_benchmark_groups = apply_group);

/// gungraun counts instructions under Valgrind, which this bench supports on Linux only.
#[cfg(not(target_os = "linux"))]
#[expect(
    clippy::print_stdout,
    reason = "a bench's one line of output goes to stdout"
)]
fn main() {
    println!("apply: gungraun needs Valgrind; runs on Linux only");
}
