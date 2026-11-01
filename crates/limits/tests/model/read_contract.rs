//! The read contract: one `ReadDone` per `Read`, a `ReadDone` nobody asked for does nothing, and
//! a failing read that changes nothing sends nothing.

use clauseo_limits::{Action, Input, Limits, ReadFailure, ReadOutcome};
use clauseo_protocol::{AgentKind, Freshness};

use crate::support::{account, at, done, five_hour, read_once, t0, usage, view_of};

const CLAUDE: AgentKind = AgentKind::Claude;

#[test]
fn read_done_with_no_read_in_flight_is_ignored() {
    let mut model = read_once(account(CLAUDE, vec![five_hour(10.0)]));
    let failed = ReadOutcome::Failed(ReadFailure::TimedOut);
    assert_eq!(model.input(done(CLAUDE, failed), at(1)), vec![]);
    assert_eq!(view_of(&model, CLAUDE, at(1)).freshness, Freshness::Fresh);
    let figures = usage(account(CLAUDE, vec![five_hour(90.0)]));
    assert_eq!(model.input(done(CLAUDE, figures), at(2)), vec![]);
    let meter = view_of(&model, CLAUDE, at(2)).meter.expect("a meter");
    assert_eq!(
        meter.used_percent.get(),
        10.0,
        "the stray figures were not merged"
    );
}

#[test]
fn second_method_missing_in_a_row_sends_nothing() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    assert_eq!(
        model.input(done(CLAUDE, ReadOutcome::MethodMissing), at(1)),
        vec![Action::ViewChanged]
    );
    let again = model.input(
        Input::TryAgain {
            agent: Some(CLAUDE),
        },
        at(2),
    );
    assert_eq!(again, vec![Action::Read(CLAUDE)]);
    assert_eq!(
        model.input(done(CLAUDE, ReadOutcome::MethodMissing), at(3)),
        vec![]
    );
}
