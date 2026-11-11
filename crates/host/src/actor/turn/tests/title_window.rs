//! The name's grace window (D-070) on the manual clock: an agent's title that arrives within
//! `TITLE_GRACE` after the first answered turn ends is taken, one that arrives after it, or
//! after the next prompt went out, is not. The agent's title is held back by the rig until the
//! test has moved the clock, so its arrival time is the test's.

use std::time::Duration;

use clauseo_acp::{AcpEvent, AgentUpdate};
use clauseo_protocol::{CommandOutcome, DomainEvent, HostCommand, RenameSource, ThreadId, Title};

use super::super::TurnPhase;
use super::super::state::TITLE_GRACE;
use super::agents::send_prompt;
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::agent::AgentInput;

/// The title the agent sends in these tests.
const AGENT_TITLE: &str = "Indemnity cap review";

/// The lawyer's hand-typed title.
const TYPED: &str = "Acme MSA";

/// One answered turn, then the agent's title after its `end_turn`, as the adapters send it.
const ANSWER_THEN_TITLE: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-answer-1","content":{"type":"text","text":"First answer."}}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"update":{"update":{"sessionUpdate":"session_info_update","title":"Indemnity cap review"}}}
"#;

/// The same, and a second prompt that the agent answers after its title.
const ANSWER_THEN_TITLE_AND_SECOND_PROMPT: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-answer-1","content":{"type":"text","text":"First answer."}}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"update":{"update":{"sessionUpdate":"session_info_update","title":"Indemnity cap review"}}}
{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-answer-2","content":{"type":"text","text":"Second answer."}}}}
{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}
"#;

/// A first turn that fails before any answer text, then the agent's title after its end.
const ERROR_THEN_TITLE: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","error":{"code":-32603,"message":"Internal error"}}}
{"update":{"update":{"sessionUpdate":"session_info_update","title":"Indemnity cap review"}}}
"#;

/// Whether `input` is an agent's session title, held or not.
fn is_title(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Agent {
            input: AgentInput::Event(event),
            ..
        } if matches!(
            event.as_ref(),
            AcpEvent::Update {
                update: AgentUpdate::SessionTitle { .. },
                ..
            }
        )
    )
}

/// The renames of `thread` the log holds, in order: each title and who set it.
fn renames(rig: &mut Rig, thread: ThreadId) -> Vec<(String, RenameSource)> {
    rig.log(thread)
        .iter()
        .filter_map(|event| {
            if let DomainEvent::ThreadRenamed(renamed) = event {
                Some((renamed.title.to_string(), renamed.by))
            } else {
                None
            }
        })
        .collect()
}

/// Runs the first answered turn of `thread` until it has ended, with the agent's title held
/// back. Its end is in the log's events, and the title is in `rig.held`.
fn answered_turn_with_title_held(rig: &mut Rig, thread: ThreadId) {
    let _first = send_prompt(rig, 10, thread);
    rig.hold_next("the agent's title", is_title);
    rig.run_holding("the first turn's end", is_title, |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && actor.work.commits == 0
    });
    assert_eq!(rig.held.len(), 1, "the agent's title is held back");
    assert!(
        renames(rig, thread).is_empty(),
        "no rename before the title is handled"
    );
}

/// Handles the held title and every commit it makes.
fn release_title(rig: &mut Rig) {
    rig.release_held();
    rig.settle_commits();
}

#[test]
fn a_title_within_the_grace_window_after_the_first_answer_is_taken() {
    let mut rig = Rig::start("title-window-taken", ANSWER_THEN_TITLE);
    let thread = rig.thread();
    answered_turn_with_title_held(&mut rig, thread);
    rig.clock.advance(Duration::from_secs(3));
    release_title(&mut rig);
    assert_eq!(
        renames(&mut rig, thread),
        [(AGENT_TITLE.to_owned(), RenameSource::Agent)],
        "{:?}",
        rig.log(thread)
    );
    rig.finish();
}

#[test]
fn a_title_after_the_grace_window_after_the_first_answer_is_ignored() {
    let mut rig = Rig::start("title-window-expired", ANSWER_THEN_TITLE);
    let thread = rig.thread();
    answered_turn_with_title_held(&mut rig, thread);
    rig.clock.advance(TITLE_GRACE + Duration::from_secs(1));
    release_title(&mut rig);
    assert_eq!(renames(&mut rig, thread), [], "{:?}", rig.log(thread));
    rig.finish();
}

#[test]
fn the_next_prompt_closes_the_window_within_the_grace_time() {
    let mut rig = Rig::start(
        "title-window-next-prompt",
        ANSWER_THEN_TITLE_AND_SECOND_PROMPT,
    );
    let thread = rig.thread();
    answered_turn_with_title_held(&mut rig, thread);
    // The clock has not moved: the second prompt alone closes the window.
    let second = send_prompt(&mut rig, 11, thread);
    rig.run_until("the second prompt sent", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.turn == second && current.phase == TurnPhase::Prompting)
    });
    release_title(&mut rig);
    assert_eq!(renames(&mut rig, thread), [], "{:?}", rig.log(thread));
    rig.finish();
}

#[test]
fn a_lawyers_rename_within_the_grace_window_keeps_its_title() {
    let mut rig = Rig::start("title-window-typed", ANSWER_THEN_TITLE);
    let thread = rig.thread();
    answered_turn_with_title_held(&mut rig, thread);
    let typed = HostCommand::RenameThread {
        thread,
        title: Title::new(TYPED).unwrap(),
        placeholder: false,
    };
    assert_eq!(rig.dispatch(11, typed).unwrap(), CommandOutcome::Done);
    release_title(&mut rig);
    assert_eq!(
        renames(&mut rig, thread),
        [(TYPED.to_owned(), RenameSource::Lawyer)],
        "{:?}",
        rig.log(thread)
    );
    rig.finish();
}

/// Between turns the rig's stream is unloaded, so the window reads the folded history: an
/// unanswered first turn settles nothing, and its title is taken (D-070).
#[test]
fn a_title_between_turns_after_an_unanswered_first_turn_is_taken() {
    let mut rig = Rig::start("title-window-unanswered", ERROR_THEN_TITLE);
    let thread = rig.thread();
    let _first = send_prompt(&mut rig, 10, thread);
    rig.run_holding("the first turn's end", is_title, |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && actor.work.commits == 0
    });
    if rig.held.is_empty() {
        rig.hold_next("the agent's title", is_title);
    }
    release_title(&mut rig);
    assert_eq!(
        renames(&mut rig, thread),
        [(AGENT_TITLE.to_owned(), RenameSource::Agent)],
        "{:?}",
        rig.log(thread)
    );
    rig.finish();
}
