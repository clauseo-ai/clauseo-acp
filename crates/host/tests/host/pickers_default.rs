//! The draft's default (`actor/agent_options.rs`): the options `ListAgents` keeps for an agent
//! hold each current value its latest session started with, before Clauseo set any pick. A
//! matter's remembered pick is set and the kept current stays the start's; a pick in an open
//! conversation leaves it too, across a restart; a later session that starts on another default
//! moves it, and the values still refresh from every report.

use clauseo_protocol::{CommandOutcome, FieldChange, HostCommand};

use crate::observe::Events;
use crate::pickers::{
    CLAUDE, agent_options, claude_prefs, create_thread, current, logged, set, turn, values,
};
use crate::support::{Dir, Scenario};

/// Waits until the thread logs options whose `option` is `value`.
fn until_current(events: &mut Events, option: &str, value: &str) {
    events.until(value, |e| {
        current(&logged(e), option).as_deref() == Some(value)
    });
}

/// (1) A matter's remembered model and effort are set before the first prompt, and the kept
/// options still hold the session's starting defaults.
#[test]
fn a_remembered_pick_is_set_and_the_kept_current_stays_the_start() {
    let dir = Dir::new(
        "pickers_default_remembered",
        &[(CLAUDE, Scenario::Bundled("pickers-picked"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    host.ok(HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        skill_packs: FieldChange::Keep,
        pinned: None,
        agent_prefs: FieldChange::Set(claude_prefs(&[("model", "opus"), ("effort", "high")])),
    });
    let thread = create_thread(&host, matter, None);
    let events = turn(&host, thread, "Summarise clause 4.");
    let options = logged(&events);
    assert_eq!(current(&options, "model").as_deref(), Some("opus"));
    assert_eq!(current(&options, "effort").as_deref(), Some("high"));
    let kept = agent_options(&host);
    assert_eq!(current(&kept, "model").as_deref(), Some("default"));
    assert_eq!(current(&kept, "effort").as_deref(), Some("default"));
    host.shutdown();
}

/// (2) A `SetConfigOption` in an open conversation reaches the agent and the thread's options,
/// and leaves the kept current at the start's, after a restart too.
#[test]
fn a_pick_in_a_conversation_leaves_the_kept_current() {
    let dir = Dir::new(
        "pickers_default_open",
        &[(CLAUDE, Scenario::Bundled("pickers-first"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = create_thread(&host, matter, None);
    let mut events = turn(&host, thread, "Summarise clause 4.");
    assert_eq!(host.ok(set(thread, "model", "opus")), CommandOutcome::Done);
    until_current(&mut events, "model", "opus");
    assert_eq!(host.ok(set(thread, "effort", "high")), CommandOutcome::Done);
    until_current(&mut events, "effort", "high");
    events.assert_no_mismatch();
    let kept = agent_options(&host);
    assert_eq!(current(&kept, "model").as_deref(), Some("default"));
    assert_eq!(current(&kept, "effort").as_deref(), Some("default"));
    host.shutdown();

    let host = dir.start(1);
    let kept = agent_options(&host);
    assert_eq!(current(&kept, "model").as_deref(), Some("default"));
    assert_eq!(current(&kept, "effort").as_deref(), Some("default"));
    host.shutdown();
}

/// (3) A later session that starts on another default moves the kept current to it, with the
/// values it reports; (4) a pick's answer that offers a new value refreshes the values and
/// keeps the current.
#[test]
fn a_later_start_moves_the_kept_current_and_values_refresh() {
    let dir = Dir::new(
        "pickers_default_moved",
        &[(CLAUDE, Scenario::Bundled("pickers-first"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let first = create_thread(&host, matter, None);
    drop(turn(&host, first, "Summarise clause 4."));
    let kept = agent_options(&host);
    assert_eq!(current(&kept, "model").as_deref(), Some("default"));
    assert_eq!(values(&kept, "effort"), ["default", "low", "high", "max"]);
    host.shutdown();

    dir.set_agents(&[(CLAUDE, Scenario::Bundled("pickers-default-moved"))]);
    let host = dir.start(1);
    let second = create_thread(&host, matter, None);
    let mut events = turn(&host, second, "And clause 5?");
    let kept = agent_options(&host);
    assert_eq!(current(&kept, "model").as_deref(), Some("sonnet"));
    assert_eq!(current(&kept, "effort").as_deref(), Some("low"));
    assert_eq!(
        values(&kept, "model"),
        ["default", "opus", "haiku", "sonnet"]
    );
    assert_eq!(values(&kept, "effort"), ["default", "low", "high"]);

    assert_eq!(host.ok(set(second, "model", "opus")), CommandOutcome::Done);
    until_current(&mut events, "model", "opus");
    events.assert_no_mismatch();
    let kept = agent_options(&host);
    assert_eq!(current(&kept, "model").as_deref(), Some("sonnet"));
    assert_eq!(
        values(&kept, "model"),
        ["default", "opus", "haiku", "sonnet", "fable"]
    );
    host.shutdown();
}
