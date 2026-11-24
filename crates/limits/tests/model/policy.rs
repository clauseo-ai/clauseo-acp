//! The read policy: the reads at start, the triggers and their ages, one read in flight per
//! account, no read while the agent asks a question.

use clauseo_limits::{Action, Input, Limits, LiveStatus, ReadFailure, ReadOutcome};
use clauseo_protocol::{
    AccountLimits, AgentKind, Billing, BillingProvider, Freshness, LimitSource, StaleReason,
};

use crate::support::{
    MINUTE, SECOND, account, at, at_ms, done, five_hour, live, live_input, read_once, t0, usage,
    view_of,
};

const CLAUDE: AgentKind = AgentKind::Claude;
const CODEX: AgentKind = AgentKind::Codex;

/// A model of `agent` whose first read started and ended at the start time.
fn idle(agent: AgentKind) -> Limits {
    read_once(account(agent, vec![five_hour(10.0)]))
}

fn shown(agent: AgentKind) -> Input {
    Input::Shown { agent: Some(agent) }
}

fn turn_ended(agent: AgentKind, read_now: bool) -> Input {
    Input::TurnEnded { agent, read_now }
}

fn try_again(agent: Option<AgentKind>) -> Input {
    Input::TryAgain { agent }
}

#[test]
fn new_turns_cached_values_stale_aged_and_asks_one_read_per_agent() {
    let claude = AccountLimits {
        source: LimitSource::Fresh {
            checked_at: at(-60),
        },
        ..account(CLAUDE, vec![five_hour(30.0)])
    };
    let codex = AccountLimits {
        billing: Billing::PayAsYouGo {
            via: BillingProvider::OpenAi,
        },
        source: LimitSource::NotApplicable,
        ..account(CODEX, vec![])
    };
    let (model, actions) = Limits::new(vec![claude, codex], &[CLAUDE, CODEX], t0());
    assert_eq!(actions, vec![Action::Read(CLAUDE), Action::Read(CODEX)]);
    let view = view_of(&model, CLAUDE, t0());
    assert_eq!(view.freshness, Freshness::Stale);
    assert_eq!(view.checked_at, Some(at(-60)));
    assert_eq!(
        view_of(&model, CODEX, t0()).freshness,
        Freshness::NotApplicable
    );
}

#[test]
fn new_turns_a_live_only_cache_stale_aged() {
    let cached = AccountLimits {
        source: LimitSource::LiveOnly { updated_at: at(-5) },
        ..account(CLAUDE, vec![five_hour(30.0)])
    };
    let (mut model, _) = Limits::new(vec![cached], &[CLAUDE], t0());
    let view = view_of(&model, CLAUDE, t0());
    assert_eq!(view.freshness, Freshness::Stale);
    assert_eq!(view.checked_at, Some(at(-5)));
    let actions = model.input(
        done(CLAUDE, ReadOutcome::Failed(ReadFailure::TimedOut)),
        at(1),
    );
    let [Action::Changed(limits)] = actions.as_slice() else {
        panic!("{actions:?}")
    };
    assert_eq!(
        limits.source,
        LimitSource::Stale {
            checked_at: at(-5),
            reason: StaleReason::ReadFailed
        },
        "Aged becomes ReadFailed, the numbers' time stays"
    );
}

#[test]
fn new_without_a_cache_is_loading() {
    let (model, actions) = Limits::new(Vec::new(), &[CODEX], t0());
    assert_eq!(actions, vec![Action::Read(CODEX)]);
    assert_eq!(view_of(&model, CODEX, t0()).freshness, Freshness::Loading);
}

#[test]
fn new_asks_no_read_for_a_cached_agent_not_given() {
    let cached = vec![account(CODEX, vec![five_hour(10.0)])];
    let (model, actions) = Limits::new(cached, &[CLAUDE], t0());
    assert_eq!(actions, vec![Action::Read(CLAUDE)]);
    assert!(!model.view(t0()).0.contains_key(&CODEX));
    assert_eq!(model.at_limit(CODEX, t0()), None);
}

#[test]
fn new_counts_an_agent_given_twice_once() {
    let (model, actions) = Limits::new(Vec::new(), &[CLAUDE, CLAUDE], t0());
    assert_eq!(actions, vec![Action::Read(CLAUDE)]);
    assert_eq!(model.view(t0()).0.len(), 1);
}

/// A cached Claude entry of `email`, checked at `checked_at` with the 5-hour window at `used`.
fn cached(email: &str, checked_at: clauseo_protocol::Timestamp, used: f64) -> AccountLimits {
    AccountLimits {
        account: clauseo_protocol::AccountRef(email.to_owned()),
        source: LimitSource::Fresh { checked_at },
        ..account(CLAUDE, vec![five_hour(used)])
    }
}

/// The used percentage of Claude's meter at the start time.
fn meter_percent(model: &Limits) -> f64 {
    let meter = view_of(model, CLAUDE, t0()).meter.expect("a meter");
    meter.used_percent.get()
}

#[test]
fn new_keeps_the_newest_cached_entry_of_an_agent_in_store_order() {
    let alice = cached("alice@example.com", at(-10), 10.0);
    let zed = cached("zed@example.com", at(-30 * 24 * 60), 99.0);
    let (model, _) = Limits::new(vec![alice, zed], &[CLAUDE], t0());
    assert_eq!(view_of(&model, CLAUDE, t0()).checked_at, Some(at(-10)));
    assert_eq!(meter_percent(&model), 10.0);
}

#[test]
fn new_keeps_the_later_of_two_cached_entries_checked_at_the_same_time() {
    let first = cached("alice@example.com", at(-1), 10.0);
    let second = cached("zed@example.com", at(-1), 20.0);
    let (model, _) = Limits::new(vec![first, second], &[CLAUDE], t0());
    assert_eq!(meter_percent(&model), 20.0);
}

#[test]
fn new_ranks_a_cached_entry_with_no_time_below_a_timed_one() {
    let timed = AccountLimits {
        source: LimitSource::LiveOnly {
            updated_at: at(-60),
        },
        ..cached("alice@example.com", at(-60), 10.0)
    };
    let untimed = AccountLimits {
        billing: Billing::PayAsYouGo {
            via: BillingProvider::Anthropic,
        },
        source: LimitSource::NotApplicable,
        ..cached("zed@example.com", at(0), 0.0)
    };
    let (model, _) = Limits::new(vec![timed, untimed], &[CLAUDE], t0());
    let view = view_of(&model, CLAUDE, t0());
    assert_eq!(view.freshness, Freshness::Stale);
    assert_eq!(view.checked_at, Some(at(-60)));
}

#[test]
fn claude_shown_reads_at_two_minutes_not_before() {
    let mut model = idle(CLAUDE);
    assert_eq!(
        model.input(shown(CLAUDE), at_ms(2 * MINUTE - SECOND)),
        vec![]
    );
    assert_eq!(
        model.input(shown(CLAUDE), at(2)),
        vec![Action::Read(CLAUDE)]
    );
}

#[test]
fn codex_shown_reads_at_one_minute_not_before() {
    let mut model = idle(CODEX);
    assert_eq!(model.input(shown(CODEX), at_ms(MINUTE - SECOND)), vec![]);
    assert_eq!(model.input(shown(CODEX), at(1)), vec![Action::Read(CODEX)]);
}

/// Settings' minute tick sends `Shown` each minute: Claude reads every second tick, and a tick
/// while a read runs joins it, so ticks never stack reads.
#[test]
fn shown_each_minute_reads_claude_every_two_minutes_one_at_a_time() {
    let mut model = idle(CLAUDE);
    assert_eq!(model.input(shown(CLAUDE), at(1)), vec![]);
    assert_eq!(
        model.input(shown(CLAUDE), at(2)),
        vec![Action::Read(CLAUDE)]
    );
    assert_eq!(model.input(shown(CLAUDE), at(3)), vec![], "a read runs");
    assert_eq!(model.input(shown(CLAUDE), at(4)), vec![], "a read runs");
}

#[test]
fn shown_for_every_agent_reads_each_one_due() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE, CODEX], t0());
    model.input(done(CLAUDE, usage(account(CLAUDE, vec![]))), t0());
    model.input(done(CODEX, usage(account(CODEX, vec![]))), t0());
    let actions = model.input(Input::Shown { agent: None }, at(5));
    assert_eq!(actions, vec![Action::Read(CLAUDE), Action::Read(CODEX)]);
}

/// A live 5-hour signal of `agent` at 20%.
fn signal(agent: AgentKind) -> Input {
    live_input(
        agent,
        live("five_hour", LiveStatus::Allowed, Some(20.0), None),
    )
}

#[test]
fn claude_turn_end_with_a_live_signal_reads_at_a_minute_not_before() {
    let mut model = idle(CLAUDE);
    model.input(signal(CLAUDE), at_ms(30 * SECOND));
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at_ms(MINUTE - SECOND)),
        vec![]
    );
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at(1)),
        vec![Action::Read(CLAUDE)]
    );
}

#[test]
fn claude_turn_end_with_no_live_signal_reads_at_a_minute_not_before() {
    let mut model = idle(CLAUDE);
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at_ms(MINUTE - SECOND)),
        vec![]
    );
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at(1)),
        vec![Action::Read(CLAUDE)]
    );
}

#[test]
fn claude_turn_end_with_no_live_signal_reads_when_the_last_read_started_three_minutes_ago() {
    let mut model = idle(CLAUDE);
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at(3)),
        vec![Action::Read(CLAUDE)]
    );
}

#[test]
fn claude_turn_end_61_seconds_after_the_last_read_reads_with_or_without_a_live_signal() {
    let mut without = idle(CLAUDE);
    assert_eq!(
        without.input(turn_ended(CLAUDE, false), at_ms(61 * SECOND)),
        vec![Action::Read(CLAUDE)],
        "no live signal"
    );
    let mut with = idle(CLAUDE);
    with.input(signal(CLAUDE), at_ms(30 * SECOND));
    assert_eq!(
        with.input(turn_ended(CLAUDE, false), at_ms(61 * SECOND)),
        vec![Action::Read(CLAUDE)],
        "a live signal came"
    );
}

#[test]
fn claude_turn_end_30_seconds_after_the_last_read_reads_none_at_once_with_or_without_a_signal() {
    let mut without = idle(CLAUDE);
    assert_eq!(
        without.input(turn_ended(CLAUDE, false), at_ms(30 * SECOND)),
        vec![],
        "no live signal"
    );
    let mut with = idle(CLAUDE);
    with.input(signal(CLAUDE), at_ms(10 * SECOND));
    assert_eq!(
        with.input(turn_ended(CLAUDE, false), at_ms(30 * SECOND)),
        vec![],
        "a live signal came"
    );
}

#[test]
fn claude_turn_end_30_seconds_after_the_last_read_asks_one_read_for_a_minute_after_it() {
    let mut model = idle(CLAUDE);
    model.input(Input::Watching { any: true }, at_ms(SECOND));
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at_ms(30 * SECOND)),
        vec![]
    );
    assert_eq!(model.next_deadline(), Some(at_ms(MINUTE)));
    assert_eq!(
        model.input(Input::Deadline, at_ms(59 * SECOND)),
        vec![],
        "not due yet"
    );
    assert_eq!(
        model.input(Input::Deadline, at_ms(MINUTE)),
        vec![Action::Read(CLAUDE)],
        "the turn end's read, a minute after the last one"
    );
    assert_eq!(
        model.input(Input::Deadline, at_ms(MINUTE + SECOND)),
        vec![],
        "asked once"
    );
}

#[test]
fn a_read_started_before_the_deferred_turn_end_read_is_due_clears_it() {
    let mut model = idle(CLAUDE);
    model.input(Input::Watching { any: true }, at_ms(SECOND));
    model.input(turn_ended(CLAUDE, false), at_ms(30 * SECOND));
    assert_eq!(
        model.input(try_again(Some(CLAUDE)), at_ms(40 * SECOND)),
        vec![Action::Read(CLAUDE)]
    );
    model.input(
        done(CLAUDE, usage(account(CLAUDE, vec![]))),
        at_ms(41 * SECOND),
    );
    assert_eq!(
        model.input(Input::Deadline, at_ms(MINUTE)),
        vec![],
        "the read at 40 s covers the turn end"
    );
}

#[test]
fn a_turn_end_read_now_leaves_the_next_turn_end_its_own_minute() {
    let mut model = idle(CLAUDE);
    assert_eq!(
        model.input(turn_ended(CLAUDE, true), at(2)),
        vec![Action::Read(CLAUDE)]
    );
    model.input(done(CLAUDE, usage(account(CLAUDE, vec![]))), at(2));
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at(4)),
        vec![Action::Read(CLAUDE)],
        "the read started 2 minutes ago"
    );
}

#[test]
fn turn_ends_in_quick_succession_start_one_read_until_it_is_done() {
    let mut model = idle(CLAUDE);
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at(3)),
        vec![Action::Read(CLAUDE)]
    );
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at_ms(3 * MINUTE + SECOND)),
        vec![],
        "the read in flight counts"
    );
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at(6)),
        vec![],
        "old enough, but the read is still in flight: none stacks"
    );
    model.input(done(CLAUDE, usage(account(CLAUDE, vec![]))), at(6));
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at_ms(6 * MINUTE + SECOND)),
        vec![Action::Read(CLAUDE)],
        "once it is done, the next turn end reads: its age counts from 3 minutes"
    );
}

#[test]
fn codex_turn_end_reads_at_sixty_seconds_not_before() {
    let mut model = idle(CODEX);
    assert_eq!(
        model.input(turn_ended(CODEX, false), at_ms(59 * SECOND)),
        vec![]
    );
    assert_eq!(
        model.input(turn_ended(CODEX, false), at_ms(60 * SECOND)),
        vec![Action::Read(CODEX)]
    );
}

#[test]
fn turn_end_age_counts_from_when_the_last_read_started() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    model.input(done(CLAUDE, usage(account(CLAUDE, vec![]))), at(3));
    assert_eq!(
        model.input(turn_ended(CLAUDE, false), at(10)),
        vec![Action::Read(CLAUDE)]
    );
}

#[test]
fn turn_end_read_now_reads_at_once_whatever_the_age() {
    let mut model = idle(CLAUDE);
    assert_eq!(
        model.input(turn_ended(CLAUDE, true), at_ms(SECOND)),
        vec![Action::Read(CLAUDE)]
    );
}

#[test]
fn turn_end_read_now_asks_none_while_one_is_in_flight() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    assert_eq!(model.input(turn_ended(CLAUDE, true), at(1)), vec![]);
}

#[test]
fn at_most_one_read_is_in_flight_per_account() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    assert_eq!(model.input(Input::Shown { agent: None }, at(10)), vec![]);
    assert_eq!(
        model.input(
            Input::TryAgain {
                agent: Some(CLAUDE)
            },
            at(10)
        ),
        vec![]
    );
    model.input(done(CLAUDE, usage(account(CLAUDE, vec![]))), at(11));
    assert_eq!(
        model.input(Input::TryAgain { agent: None }, at(11)),
        vec![Action::Read(CLAUDE)],
        "try again reads whatever the age once nothing is in flight"
    );
}

#[test]
fn no_read_while_asking_and_the_asked_for_read_starts_on_the_answer() {
    let mut model = idle(CLAUDE);
    assert_eq!(
        model.input(
            Input::Asking {
                agent: CLAUDE,
                asking: true
            },
            at(1)
        ),
        vec![]
    );
    assert_eq!(
        model.input(
            Input::TryAgain {
                agent: Some(CLAUDE)
            },
            at(2)
        ),
        vec![]
    );
    assert_eq!(model.input(turn_ended(CLAUDE, true), at(3)), vec![]);
    assert_eq!(
        model.input(
            Input::Asking {
                agent: CLAUDE,
                asking: false
            },
            at(4)
        ),
        vec![Action::Read(CLAUDE)]
    );
    assert_eq!(
        model.input(
            Input::TryAgain {
                agent: Some(CLAUDE)
            },
            at(5)
        ),
        vec![]
    );
}

#[test]
fn answering_when_no_read_was_asked_for_reads_nothing() {
    let mut model = idle(CLAUDE);
    model.input(
        Input::Asking {
            agent: CLAUDE,
            asking: true,
        },
        at(1),
    );
    assert_eq!(
        model.input(
            Input::Asking {
                agent: CLAUDE,
                asking: false
            },
            at(20)
        ),
        vec![]
    );
}

#[test]
fn inputs_for_an_agent_not_given_do_nothing() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    let inputs = vec![
        shown(CODEX),
        Input::TryAgain { agent: Some(CODEX) },
        turn_ended(CODEX, true),
        Input::Asking {
            agent: CODEX,
            asking: false,
        },
        live_input(
            CODEX,
            live("five_hour", LiveStatus::Allowed, Some(30.0), None),
        ),
        done(CODEX, usage(account(CODEX, vec![five_hour(10.0)]))),
    ];
    for input in inputs {
        assert_eq!(model.input(input.clone(), at(30)), vec![], "{input:?}");
    }
    assert!(!model.view(at(30)).0.contains_key(&CODEX));
    assert_eq!(model.next_read_at(CODEX), None);
}

#[test]
fn two_tries_a_second_apart_read_once() {
    let mut model = idle(CLAUDE);
    assert_eq!(
        model.input(try_again(Some(CLAUDE)), at_ms(11 * SECOND)),
        vec![Action::Read(CLAUDE)]
    );
    model.input(
        done(CLAUDE, usage(account(CLAUDE, vec![]))),
        at_ms(12 * SECOND),
    );
    assert_eq!(
        model.input(try_again(Some(CLAUDE)), at_ms(12 * SECOND)),
        vec![Action::ViewChanged],
        "the read started a second ago: none starts, the view is published again"
    );
}

#[test]
fn try_again_eleven_seconds_after_the_last_read_started_reads_again() {
    let mut model = idle(CLAUDE);
    model.input(try_again(Some(CLAUDE)), at_ms(11 * SECOND));
    model.input(
        done(CLAUDE, usage(account(CLAUDE, vec![]))),
        at_ms(12 * SECOND),
    );
    assert_eq!(
        model.input(try_again(Some(CLAUDE)), at_ms(22 * SECOND)),
        vec![Action::Read(CLAUDE)]
    );
}

#[test]
fn try_again_within_ten_seconds_of_a_turn_end_read_starts_none() {
    let mut model = idle(CLAUDE);
    assert_eq!(
        model.input(turn_ended(CLAUDE, true), at_ms(20 * SECOND)),
        vec![Action::Read(CLAUDE)]
    );
    model.input(
        done(CLAUDE, usage(account(CLAUDE, vec![]))),
        at_ms(21 * SECOND),
    );
    assert_eq!(
        model.input(try_again(Some(CLAUDE)), at_ms(22 * SECOND)),
        vec![Action::ViewChanged]
    );
}

#[test]
fn try_again_within_ten_seconds_of_a_shown_read_starts_none() {
    let mut model = idle(CLAUDE);
    assert_eq!(
        model.input(shown(CLAUDE), at(5)),
        vec![Action::Read(CLAUDE)]
    );
    model.input(
        done(CLAUDE, usage(account(CLAUDE, vec![]))),
        at_ms(5 * MINUTE + SECOND),
    );
    assert_eq!(
        model.input(try_again(Some(CLAUDE)), at_ms(5 * MINUTE + 2 * SECOND)),
        vec![Action::ViewChanged]
    );
}

#[test]
fn try_again_while_a_read_is_in_flight_joins_it_without_a_second_read() {
    let mut model = idle(CLAUDE);
    assert_eq!(
        model.input(try_again(Some(CLAUDE)), at_ms(11 * SECOND)),
        vec![Action::Read(CLAUDE)]
    );
    assert_eq!(
        model.input(try_again(Some(CLAUDE)), at_ms(30 * SECOND)),
        vec![],
        "in flight and past the window: joined, nothing published"
    );
    assert_eq!(
        model.input(try_again(Some(CLAUDE)), at_ms(16 * SECOND)),
        vec![Action::ViewChanged],
        "in flight and within the window: none starts, the view is published again"
    );
}

#[test]
fn try_again_for_every_agent_throttles_only_the_recent_one_and_publishes_once() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE, CODEX], t0());
    model.input(done(CLAUDE, usage(account(CLAUDE, vec![]))), t0());
    model.input(done(CODEX, usage(account(CODEX, vec![]))), t0());
    assert_eq!(
        model.input(turn_ended(CLAUDE, true), at_ms(5 * SECOND)),
        vec![Action::Read(CLAUDE)]
    );
    model.input(
        done(CLAUDE, usage(account(CLAUDE, vec![]))),
        at_ms(6 * SECOND),
    );
    assert_eq!(
        model.input(try_again(None), at_ms(11 * SECOND)),
        vec![Action::Read(CODEX), Action::ViewChanged],
        "Claude's read started 6 s ago: only Codex reads, and one view goes out"
    );
}

#[test]
fn try_again_for_every_agent_within_the_window_publishes_one_view() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE, CODEX], t0());
    model.input(done(CLAUDE, usage(account(CLAUDE, vec![]))), t0());
    model.input(done(CODEX, usage(account(CODEX, vec![]))), t0());
    assert_eq!(
        model.input(try_again(None), at_ms(SECOND)),
        vec![Action::ViewChanged]
    );
}
