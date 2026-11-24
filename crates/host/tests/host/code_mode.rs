//! Code mode per turn (`clauseo_protocol::is_code_mode_call`): a turn whose agent made a script
//! call (`toolu_cc_01`) ends with `TurnEnded::code_mode` and its step marked; a turn with none
//! is not; after a restart the view rebuilt from the log still marks only the first answer.

use clauseo_protocol::{StepEntry, StreamId, ThreadId, TurnId, ViewEntry, ViewPatchBody};

use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

/// The steps of a fresh view of `thread` (its `Reset`).
fn steps(host: &Running, thread: ThreadId) -> Vec<StepEntry> {
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until("the view's reset", |view| view.patches().next().is_some());
    let Some(ViewPatchBody::Reset { view_window }) = view.patches().next().map(|patch| &patch.body)
    else {
        panic!("the view did not start with a reset: {}", view.dump());
    };
    view_window
        .entries
        .iter()
        .filter_map(|entry| match entry {
            ViewEntry::Step(step) => Some(step.clone()),
            _ => None,
        })
        .collect()
}

fn marked(steps: &[StepEntry], turn: TurnId) -> bool {
    steps
        .iter()
        .find(|step| step.turn == turn)
        .unwrap_or_else(|| panic!("no step of {turn}"))
        .code_mode
}

#[test]
fn only_the_turn_with_a_script_call_is_in_code_mode_across_a_restart() {
    let dir = Dir::new(
        "code_mode_turns",
        &[(FAKE, Scenario::Local("code-mode-turns"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Count the clauses.");
    assert!(
        events.until_turn_ended(first).code_mode,
        "{}",
        events.dump()
    );
    let (second, _) = host.prompt(thread, "Now summarise.");
    assert!(
        !events.until_turn_ended(second).code_mode,
        "{}",
        events.dump()
    );
    events.assert_no_mismatch();
    let shown = steps(&host, thread);
    assert!(marked(&shown, first) && !marked(&shown, second));
    drop(events);
    host.shutdown();

    let host = dir.start(1);
    let shown = steps(&host, thread);
    assert!(marked(&shown, first), "the mark survives a restart");
    assert!(!marked(&shown, second));
    host.shutdown();
}
