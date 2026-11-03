//! Builders for the model's tests: limits values, read outcomes, live signals and a manual
//! time, a plain `Timestamp` the test moves forward. No clock.

use clauseo_limits::{Action, Input, Limits, LiveSignal, LiveStatus, ReadOutcome};
use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AgentLimitsView, AtLimitBehaviour, Billing, Finite,
    LimitSource, LimitStopReason, LimitWindow, LimitWindowView, PlanLabel, Timestamp, WindowKind,
};

/// The tests' start time, in milliseconds.
const T0_MS: i64 = 1_700_000_000_000;
/// One second, in milliseconds.
pub(crate) const SECOND: i64 = 1_000;
/// One minute, in milliseconds.
pub(crate) const MINUTE: i64 = 60 * SECOND;

/// The start time plus `ms` milliseconds.
pub(crate) fn at_ms(ms: i64) -> Timestamp {
    Timestamp::new(T0_MS + ms).expect("a test time is in range")
}

/// The start time plus `minutes`.
pub(crate) fn at(minutes: i64) -> Timestamp {
    at_ms(minutes * MINUTE)
}

/// The start time.
pub(crate) fn t0() -> Timestamp {
    at(0)
}

/// `value` as a percentage.
pub(crate) fn pct(value: f64) -> Finite {
    Finite::new(value).expect("a test percentage is finite")
}

/// A window `id` of `kind`, `used` percent used, labelled by its id, with no reset time.
pub(crate) fn window(id: &str, kind: WindowKind, used: f64) -> LimitWindow {
    LimitWindow {
        id: id.to_owned(),
        kind,
        label: format!("{id} label"),
        used_percent: pct(used),
        resets_at: None,
        duration_mins: None,
        warning: false,
        model: None,
    }
}

/// Claude's 5-hour window, `used` percent used.
pub(crate) fn five_hour(used: f64) -> LimitWindow {
    window("five_hour", WindowKind::Session, used)
}

/// Claude's weekly window, `used` percent used.
pub(crate) fn seven_day(used: f64) -> LimitWindow {
    window("seven_day", WindowKind::Weekly, used)
}

/// `window` resetting at `resets_at`.
pub(crate) fn resetting(window: LimitWindow, resets_at: Timestamp) -> LimitWindow {
    LimitWindow {
        resets_at: Some(resets_at),
        ..window
    }
}

/// A plan account of `agent` with `windows`, read at the start time.
pub(crate) fn account(agent: AgentKind, windows: Vec<LimitWindow>) -> AccountLimits {
    AccountLimits {
        agent,
        account: AccountRef("lawyer@example.com".to_owned()),
        plan: Some(PlanLabel("Max".to_owned())),
        billing: Billing::Plan,
        windows,
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::Fresh { checked_at: t0() },
    }
}

/// A good read of `limits`, with no limit reached.
pub(crate) fn usage(limits: AccountLimits) -> ReadOutcome {
    ReadOutcome::Usage {
        limits,
        reached: None,
    }
}

/// A good read of `limits` that says the account reached `reached`.
pub(crate) fn usage_reached(limits: AccountLimits, reached: LimitStopReason) -> ReadOutcome {
    ReadOutcome::Usage {
        limits,
        reached: Some(reached),
    }
}

/// A live signal for window `id`.
pub(crate) fn live(
    id: &str,
    status: LiveStatus,
    used: Option<f64>,
    resets_at: Option<Timestamp>,
) -> LiveSignal {
    LiveSignal {
        window: Some(id.to_owned()),
        status,
        used_percent: used.map(pct),
        resets_at,
    }
}

/// The live input of `agent` carrying `signal`.
pub(crate) fn live_input(agent: AgentKind, signal: LiveSignal) -> Input {
    Input::Live { agent, signal }
}

/// The input of a read of `agent` ending with `outcome`.
pub(crate) fn done(agent: AgentKind, outcome: ReadOutcome) -> Input {
    Input::ReadDone { agent, outcome }
}

/// A model of `agent` alone, with no cache, whose first read ended at the start time with
/// `limits`.
pub(crate) fn read_once(limits: AccountLimits) -> Limits {
    read_with(usage(limits))
}

/// A model of the read's agent alone, with no cache, whose first read ended at the start time
/// with `outcome` (a Claude model unless the outcome names Codex).
pub(crate) fn read_with(outcome: ReadOutcome) -> Limits {
    let agent = match &outcome {
        ReadOutcome::Usage { limits, .. } => limits.agent,
        _ => AgentKind::Claude,
    };
    let (mut model, actions) = Limits::new(Vec::new(), &[agent], t0());
    assert_eq!(actions, vec![Action::Read(agent)]);
    model.input(done(agent, outcome), t0());
    model
}

/// `agent`'s entry in the view at `now`.
pub(crate) fn view_of(model: &Limits, agent: AgentKind, now: Timestamp) -> AgentLimitsView {
    model
        .view(now)
        .0
        .remove(&agent)
        .expect("the agent has a view entry")
}

/// The row `id` of `view`, found by id.
pub(crate) fn row<'a>(view: &'a AgentLimitsView, id: &str) -> &'a LimitWindowView {
    view.windows
        .iter()
        .find(|row| row.id == id)
        .expect("the view has the row")
}

/// The limits a `Changed` action carries; fails on any other action list.
pub(crate) fn changed(actions: &[Action]) -> &AccountLimits {
    match actions {
        [Action::Changed(limits)] => limits,
        other => panic!("expected one Changed, got {other:?}"),
    }
}
