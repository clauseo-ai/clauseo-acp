//! The deadlines: each row's 15-minute stale mark and each shown reset time, only while someone
//! watches; marks that passed unwatched act once watching starts.

use clauseo_limits::{Action, Input, Limits, ReadFailure, ReadOutcome};
use clauseo_protocol::{AgentKind, WindowFreshness};

use crate::support::{account, at, done, five_hour, read_once, resetting, row, seven_day, view_of};

const CLAUDE: AgentKind = AgentKind::Claude;

/// A model read at the start time with the 5-hour window resetting at minute `five` and the
/// weekly window at minute `week`.
fn resets_at(five: i64, week: i64) -> Limits {
    read_once(account(
        CLAUDE,
        vec![
            resetting(five_hour(40.0), at(five)),
            resetting(seven_day(20.0), at(week)),
        ],
    ))
}

fn watching(model: &mut Limits, any: bool, minute: i64) -> Vec<Action> {
    model.input(Input::Watching { any }, at(minute))
}

#[test]
fn next_deadline_is_the_earliest_stale_mark_while_watching() {
    let mut model = resets_at(180, 10_000);
    watching(&mut model, true, 0);
    assert_eq!(model.next_deadline(), Some(at(15)));
}

#[test]
fn next_deadline_is_the_earliest_shown_reset_while_watching() {
    let mut model = resets_at(10, 10_000);
    watching(&mut model, true, 0);
    assert_eq!(model.next_deadline(), Some(at(10)));
}

#[test]
fn next_deadline_is_none_while_nobody_watches() {
    let mut model = resets_at(10, 10_000);
    assert_eq!(model.next_deadline(), None, "nobody watched yet");
    watching(&mut model, true, 0);
    assert_eq!(watching(&mut model, false, 1), vec![]);
    assert_eq!(model.next_deadline(), None);
}

#[test]
fn next_deadline_is_none_with_no_rows() {
    let mut model = read_once(account(CLAUDE, vec![]));
    watching(&mut model, true, 0);
    assert_eq!(model.next_deadline(), None);
}

#[test]
fn passed_reset_marks_the_row_reset_passed_and_asks_one_read() {
    let mut model = resets_at(10, 10);
    watching(&mut model, true, 0);
    let actions = model.input(Input::Deadline, at(10));
    assert_eq!(
        actions,
        vec![Action::Read(CLAUDE), Action::ViewChanged],
        "one read for two rows"
    );
    let view = view_of(&model, CLAUDE, at(10));
    for id in ["five_hour", "seven_day"] {
        assert_eq!(row(&view, id).freshness, WindowFreshness::ResetPassed);
        assert_eq!(
            row(&view, id).used_percent,
            None,
            "no old percentage shown as current"
        );
    }
    assert_eq!(view.meter, None);
    assert_eq!(
        model.next_deadline(),
        Some(at(15)),
        "the stale marks remain"
    );
}

#[test]
fn deadline_before_any_mark_does_nothing() {
    let mut model = resets_at(10, 10_000);
    watching(&mut model, true, 0);
    assert_eq!(model.input(Input::Deadline, at(9)), vec![]);
}

#[test]
fn stale_mark_publishes_the_view_once() {
    let mut model = resets_at(180, 10_000);
    watching(&mut model, true, 0);
    assert_eq!(
        model.input(Input::Deadline, at(15)),
        vec![Action::ViewChanged]
    );
    assert_eq!(model.input(Input::Deadline, at(15)), vec![]);
    assert_eq!(model.next_deadline(), Some(at(180)));
}

#[test]
fn watching_acts_at_once_on_marks_that_passed_unwatched() {
    let mut model = resets_at(10, 10_000);
    let actions = watching(&mut model, true, 20);
    assert_eq!(actions, vec![Action::Read(CLAUDE), Action::ViewChanged]);
    let view = view_of(&model, CLAUDE, at(20));
    assert_eq!(
        row(&view, "five_hour").freshness,
        WindowFreshness::ResetPassed
    );
    assert_eq!(row(&view, "seven_day").freshness, WindowFreshness::Stale);
    assert_eq!(
        model.next_deadline(),
        Some(at(10_000)),
        "both stale marks and the reset acted"
    );
}

#[test]
fn watching_with_nothing_passed_publishes_the_view_only() {
    let mut model = resets_at(10, 10_000);
    assert_eq!(watching(&mut model, true, 1), vec![Action::ViewChanged]);
}

#[test]
fn reset_that_passed_before_the_last_read_started_asks_no_read() {
    let mut model = read_once(account(CLAUDE, vec![resetting(five_hour(40.0), at(-1))]));
    assert_eq!(watching(&mut model, true, 0), vec![Action::ViewChanged]);
    let view = view_of(&model, CLAUDE, at(0));
    assert_eq!(
        row(&view, "five_hour").freshness,
        WindowFreshness::ResetPassed
    );
}

#[test]
fn next_read_at_is_the_next_shown_reset() {
    let mut model = resets_at(60, 10);
    assert_eq!(model.next_read_at(CLAUDE), Some(at(10)));
    model.input(Input::Deadline, at(10));
    assert_eq!(
        model.next_read_at(CLAUDE),
        Some(at(60)),
        "a passed reset is no longer next"
    );
}

#[test]
fn next_read_at_is_none_without_reset_times() {
    let model = read_once(account(CLAUDE, vec![five_hour(40.0)]));
    assert_eq!(model.next_read_at(CLAUDE), None);
}

#[test]
fn next_read_at_gives_nothing_new_after_a_failed_read() {
    let mut model = read_once(account(CLAUDE, vec![resetting(five_hour(40.0), at(10))]));
    watching(&mut model, true, 0);
    assert_eq!(
        model.input(Input::Deadline, at(10)),
        vec![Action::Read(CLAUDE), Action::ViewChanged]
    );
    model.input(
        done(CLAUDE, ReadOutcome::Failed(ReadFailure::TimedOut)),
        at(11),
    );
    assert_eq!(model.next_read_at(CLAUDE), None);
    let view = view_of(&model, CLAUDE, at(11));
    assert_eq!(
        row(&view, "five_hour").freshness,
        WindowFreshness::ResetPassed,
        "still no old figure"
    );
    assert_eq!(
        model.input(Input::Deadline, at(12)),
        vec![],
        "the model starts no retry"
    );
}
