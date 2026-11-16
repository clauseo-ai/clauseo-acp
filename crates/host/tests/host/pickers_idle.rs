//! The pickers across an idle stop (R20b): a Claude conversation sent on sonnet and low, whose
//! agent then stops idle, gets sonnet and low set again before its next prompt when the
//! relaunch loads the session at the agent's defaults (opus and medium, under the AIR
//! `recommendedValue` capability), and the header the pickers read shows them
//! (`pickers-idle-stop`, which takes each prompt only after both sets).

use clauseo_protocol::{
    ConfigOption, StopReason, StreamHeader, StreamId, ThreadId, ThreadStatus, ViewPatchBody,
};

use crate::idle_stop::{LIMIT, idle_stopped};
use crate::pickers::{CLAUDE, create_thread, current, logged, picks};
use crate::support::{Dir, Running, Scenario};

/// The options in the header of a fresh subscription's first frame: what the pickers show.
fn header_options(host: &Running, thread: ThreadId) -> Vec<ConfigOption> {
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until_items(1);
    let first = view.patches().next().expect("a first frame");
    match &first.body {
        ViewPatchBody::Reset { view_window } => match &view_window.header {
            Some(StreamHeader::Thread(header)) => header.config_options.clone(),
            other => panic!("no thread header: {other:?}"),
        },
        other => panic!("the first frame is not a reset: {other:?}"),
    }
}

#[test]
fn picks_are_set_again_after_an_idle_stop_and_the_header_shows_them() {
    let dir = Dir::new(
        "pickers_idle_stop",
        &[(CLAUDE, Scenario::Local("pickers-idle-stop"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "sonnet"), ("effort", "low")]);
    let thread = create_thread(&host, matter, Some(config));
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise clause 4.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    assert_eq!(
        current(&logged(&events), "model").as_deref(),
        Some("sonnet")
    );

    host.wait_armed(LIMIT);
    host.clock.advance(LIMIT);
    events.until("the idle stop", |events| {
        events.position(idle_stopped).is_some()
    });
    host.settle();
    assert_eq!(host.status(matter, thread), ThreadStatus::IdleStopped);

    let (second, _) = host.prompt(thread, "And clause 5?");
    let ended = events.until_turn_ended(second);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("sonnet"));
    assert_eq!(current(&options, "effort").as_deref(), Some("low"));
    let shown = header_options(&host, thread);
    assert_eq!(current(&shown, "model").as_deref(), Some("sonnet"));
    assert_eq!(current(&shown, "effort").as_deref(), Some("low"));
    host.shutdown();
}
