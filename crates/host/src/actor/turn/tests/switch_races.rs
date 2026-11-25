//! A switch's interleavings (`switch.rs`): a `SetMode` that comes while the switch runs, at
//! each of the three points where it once relaunched the old agent, waits on the switch and
//! runs on the new agent; a cancel while the switch's commit is in flight leaves the handover
//! owed to the next turn; the handover settles `Delivered` only once its prompt goes out; and a
//! helper of the old session still open at the switch's commit ends `Stopped` with its detach,
//! or with the `Closed` detach after a refused commit.
//! The new agent plays its own scenario (session `sess-2`), so the sessions attached show which
//! agent each call reached.

use clauseo_protocol::{
    AgentChoiceId, AgentId, CommandEnvelope, CommandOutcome, DetachReason, DomainEvent, ErrorCode,
    HandoffStatus, HostCommand, NoticeKind, StreamId, ThreadId,
};
use futures::channel::oneshot;

use super::super::super::Actor;
use super::super::super::input::{Input, Request};
use super::agents::send_prompt;
use super::helpers_ends::open_start;
use super::rig::{Rig, command_id};
use super::switch::{OTHER, count, one_turn, send, two_agents, until_idle};
use crate::actor::turn::helpers as turn_helpers;
use crate::actor::turn::{AgentReply, LogFor, TurnInput, agents, switch};
use crate::agent::AgentInput;
use crate::error::Error;

/// The new agent's steps: a mode change to `plan`, then one prompt, answered `end_turn`.
const MODE_THEN_PROMPT: &str = concat!(
    r#"{"expect":{"method":"session/set_mode","as":"m1","params":{"sessionId":"sess-2","modeId":"plan"}}}"#,
    "\n",
    r#"{"respond":{"to":"m1","result":{}}}"#,
    "\n",
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// The new agent's steps: prompts, each answered `end_turn`.
const PROMPTS_END: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
    r#"{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// An agent's `Ended`.
fn ended(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Agent {
            input: AgentInput::Ended(_),
            ..
        }
    )
}

/// The whole-log read of a switch.
fn switch_log(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Turn(TurnInput::Log {
            why: LogFor::Switch(_),
            ..
        })
    )
}

/// The result of the commit logging `AgentSwitched`.
fn switch_commit(actor: &Actor, input: &Input) -> bool {
    matches!(input, Input::Committed { ticket, .. }
        if actor.pipeline.events(*ticket).iter()
            .any(|e| matches!(e.event, DomainEvent::AgentSwitched(_))))
}

/// The note that a prompt carrying a handover goes out.
fn prompt_out(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Turn(TurnInput::Agent {
            reply: AgentReply::PromptOut { .. },
            ..
        })
    )
}

/// Dispatches `command` as command `n` and holds back the first input `hold` picks, whether it
/// comes before the command's answer or after.
fn dispatch_then_hold(
    rig: &mut Rig,
    n: u128,
    command: HostCommand,
    what: &str,
    hold: fn(&Actor, &Input) -> bool,
) {
    let sent = rig.dispatch_holding(n, command, hold);
    assert!(
        matches!(sent, Ok(CommandOutcome::PromptAccepted { .. })),
        "{sent:?}"
    );
    if rig.held.is_empty() {
        rig.hold_next(what, hold);
    }
    assert_eq!(rig.held.len(), 1, "one input held");
}

/// `SetMode { plan }` as command `n`, not waited for: a switch may hold it.
fn set_mode(
    rig: &mut Rig,
    n: u128,
    thread: ThreadId,
) -> oneshot::Receiver<Result<CommandOutcome, Error>> {
    let (reply, answer) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(n),
        command: HostCommand::SetMode {
            thread,
            mode_id: AgentChoiceId::new("plan").unwrap(),
        },
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    answer
}

/// Handles inputs, holding back those `hold` picks, until `thread`'s switch holds a change.
fn held_on_the_switch(rig: &mut Rig, thread: ThreadId, hold: fn(&Actor, &Input) -> bool) {
    rig.run_holding("the change held on the switch", hold, |actor| {
        switch::seen(actor, thread)
            .0
            .is_some_and(|(_, held)| held > 0)
    });
}

/// The sessions attached in `log`, in order.
fn attached(log: &[DomainEvent]) -> Vec<String> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::AgentSessionAttached(attached) = event {
                Some(attached.session.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn detaches(log: &[DomainEvent]) -> Vec<DetachReason> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::AgentSessionDetached(detached) = event {
                Some(detached.reason.clone())
            } else {
                None
            }
        })
        .collect()
}

fn settled(log: &[DomainEvent]) -> Vec<HandoffStatus> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::HandoffSettled(settled) = event {
                Some(settled.status)
            } else {
                None
            }
        })
        .collect()
}

/// The switch ran on the new agent alone: one switch, the old session detached `Switched`
/// once, only the new session attached after it, the mode change answered and the thread on
/// the new agent.
fn ran_on_the_new_agent(
    rig: &mut Rig,
    thread: ThreadId,
    mut mode: oneshot::Receiver<Result<CommandOutcome, Error>>,
) {
    until_idle(rig, thread);
    let log = rig.log(thread);
    assert_eq!(attached(&log), ["sess-1", "sess-2"], "{log:?}");
    assert_eq!(detaches(&log), [DetachReason::Switched], "{log:?}");
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::AgentSwitched(_))),
        1
    );
    assert_eq!(settled(&log), [HandoffStatus::Delivered]);
    assert!(
        matches!(mode.try_recv(), Ok(Some(Ok(_)))),
        "the mode change was answered"
    );
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
}

/// Trigger A: the mode change comes while the old agent stops.
#[test]
fn a_mode_change_while_the_old_agent_stops_runs_on_the_new_one() {
    let mut rig = two_agents("switch-mode-stopping", Some(MODE_THEN_PROMPT));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the old agent's end",
        ended,
    );
    let mode = set_mode(&mut rig, 12, thread);
    held_on_the_switch(&mut rig, thread, ended);
    let (switch, _) = switch::seen(&rig.actor, thread);
    assert_eq!(
        switch,
        Some(("Stopping".to_owned(), 1)),
        "held on the switch"
    );
    rig.release_held();
    ran_on_the_new_agent(&mut rig, thread, mode);
    rig.finish();
}

/// Trigger B: the mode change comes while the switch reads the log.
#[test]
fn a_mode_change_while_the_log_is_read_runs_on_the_new_one() {
    let mut rig = two_agents("switch-mode-reading", Some(MODE_THEN_PROMPT));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the switch's log",
        switch_log,
    );
    let mode = set_mode(&mut rig, 12, thread);
    held_on_the_switch(&mut rig, thread, switch_log);
    let (switch, _) = switch::seen(&rig.actor, thread);
    assert_eq!(
        switch,
        Some(("Reading".to_owned(), 1)),
        "held on the switch"
    );
    assert!(
        rig.actor.threads[&thread].turn.agent.is_none(),
        "no agent launched"
    );
    assert!(rig.actor.threads[&thread].turn.preparing.is_none());
    rig.release_held();
    ran_on_the_new_agent(&mut rig, thread, mode);
    rig.finish();
}

/// Trigger C: an idle stop was under way, with the mode change waiting for the agent after it.
#[test]
fn a_mode_change_waiting_for_an_idle_stop_runs_on_the_new_one() {
    let mut rig = two_agents("switch-mode-idle", Some(MODE_THEN_PROMPT));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    agents::idle_stop(&mut rig.actor, thread);
    rig.hold_next("the idle stop's end", ended);
    let mode = set_mode(&mut rig, 12, thread);
    let sent = rig.dispatch(11, send(thread, Some(OTHER)));
    assert!(
        matches!(sent, Ok(CommandOutcome::PromptAccepted { .. })),
        "{sent:?}"
    );
    let (switch, _) = switch::seen(&rig.actor, thread);
    assert_eq!(
        switch,
        Some(("Stopping".to_owned(), 1)),
        "taken from the stopping agent"
    );
    rig.release_held();
    ran_on_the_new_agent(&mut rig, thread, mode);
    rig.finish();
}

/// A cancel while the switch's commit is in flight: the commit is durable all the same, so the
/// thread is on the new agent, the handover settles `Failed` and is owed, and the next turn
/// carries it, with no "context not restored" notice.
#[test]
fn a_cancel_during_the_switch_commit_owes_the_handover() {
    let mut rig = two_agents("switch-cancel-commit", Some(PROMPTS_END));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the switch's commit",
        switch_commit,
    );
    let (reply, _cancelled) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(12),
        command: HostCommand::Cancel { thread },
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.run_holding("the cancel", switch_commit, |actor| {
        actor.threads[&thread].turn.current.is_none()
    });
    rig.release_held();
    until_idle(&mut rig, thread);
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
    let (_, owed) = switch::seen(&rig.actor, thread);
    assert_eq!(owed.as_ref().map(AgentId::as_str), Some("fake-agent"));
    let log = rig.log(thread);
    assert_eq!(settled(&log), [HandoffStatus::Failed], "{log:?}");
    assert_eq!(detaches(&log), [DetachReason::Switched], "{log:?}");

    one_turn(&mut rig, 13, thread);
    let log = rig.log(thread);
    let handoffs = count(&log, |e| matches!(e, DomainEvent::ContextHandoff(_)));
    assert_eq!(handoffs, 2, "the next turn logs its own: {log:?}");
    assert_eq!(
        settled(&log),
        [HandoffStatus::Failed, HandoffStatus::Delivered]
    );
    let notices = count(
        &log,
        |e| matches!(e, DomainEvent::Notice(n) if n.kind == Some(NoticeKind::ContextNotRestored)),
    );
    assert_eq!(notices, 0, "{log:?}");
    assert_eq!(
        switch::seen(&rig.actor, thread).1,
        None,
        "nothing owed any more"
    );
    rig.finish();
}

/// `HandoffSettled { Delivered }` waits for the new agent to answer the prompt, past the
/// Codex recheck.
#[test]
fn the_handover_is_delivered_only_once_its_prompt_goes_out() {
    let mut rig = two_agents("switch-prompt-out", Some(PROMPTS_END));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the prompt going out",
        prompt_out,
    );
    // The note comes with the agent's answer, which waits too: until then nothing settles.
    rig.hold_next("the prompt's answer", |_, input| {
        matches!(
            input,
            Input::Turn(TurnInput::Agent {
                reply: AgentReply::Prompt { .. },
                ..
            })
        )
    });
    rig.settle_commits();
    let log = rig.log(thread);
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::ContextHandoff(_))),
        1
    );
    assert_eq!(settled(&log), [], "pending until the prompt goes out");
    rig.release_held();
    until_idle(&mut rig, thread);
    assert_eq!(settled(&rig.log(thread)), [HandoffStatus::Delivered]);
    rig.finish();
}

/// A prompt the Codex recheck refuses never went out: the handover settles `Failed` and stays
/// owed. The refusal is put in place of the note that the prompt goes out, as the recheck
/// sends it.
#[test]
fn a_refused_prompt_leaves_the_handover_owed() {
    let mut rig = two_agents("switch-prompt-refused", Some(PROMPTS_END));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the prompt going out",
        prompt_out,
    );
    let Some(Input::Turn(TurnInput::Agent {
        agent,
        reply: AgentReply::PromptOut { turn },
        ..
    })) = rig.held.pop()
    else {
        panic!("not the prompt going out");
    };
    // The refusal comes in place of the prompt going out: its task never marked it out.
    switch::not_out(&mut rig.actor, thread);
    let error = Error::refused(ErrorCode::CommandInvalid, "the folder has a .codex");
    let reply = AgentReply::PromptRefused { turn, error };
    rig.actor.on_input(Input::Turn(TurnInput::Agent {
        thread,
        agent,
        reply,
    }));
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(settled(&log), [HandoffStatus::Failed], "{log:?}");
    let (_, owed) = switch::seen(&rig.actor, thread);
    assert_eq!(owed.as_ref().map(AgentId::as_str), Some("fake-agent"));
    rig.finish();
}

/// A turn that ends before its `PromptOut` arrives, though its prompt went out (the note is
/// held until the prompt's answer ended the turn), settles the handover `Delivered` and owes
/// nothing: the next turn carries no second handover.
#[test]
fn a_prompt_out_after_the_turn_ended_still_delivers_the_handover() {
    let mut rig = two_agents("switch-late-out", Some(PROMPTS_END));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the prompt going out",
        prompt_out,
    );
    rig.run_holding("the turn's end", prompt_out, |actor| {
        actor.threads[&thread].turn.current.is_none()
    });
    rig.release_held();
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(settled(&log), [HandoffStatus::Delivered], "{log:?}");
    assert_eq!(switch::seen(&rig.actor, thread).1, None, "nothing owed");

    one_turn(&mut rig, 12, thread);
    let log = rig.log(thread);
    let handoffs = count(&log, |e| matches!(e, DomainEvent::ContextHandoff(_)));
    assert_eq!(handoffs, 1, "no second handover: {log:?}");
    rig.finish();
}

/// A mode change held on a switch whose turn is cancelled while its commit is in flight waits
/// for the commit: nothing launches meanwhile, and when the store refuses the commit the row is
/// back on the old agent and the change runs there; the new agent never starts. The old
/// session's detach, refused with the commit, is logged `Closed` once the stream drains.
#[test]
fn a_change_held_on_a_refused_commit_runs_on_the_old_agent() {
    let mut rig = two_agents("switch-refused-held", Some(MODE_THEN_PROMPT));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    rig.fail(1, |e| matches!(e, DomainEvent::AgentSwitched(_)));
    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the switch's commit",
        switch_commit,
    );
    let mut mode = set_mode(&mut rig, 12, thread);
    held_on_the_switch(&mut rig, thread, switch_commit);
    let (switch, _) = switch::seen(&rig.actor, thread);
    assert_eq!(
        switch,
        Some(("Committing".to_owned(), 1)),
        "held on the switch"
    );
    let (reply, _cancelled) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(13),
        command: HostCommand::Cancel { thread },
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.run_holding("the cancel", switch_commit, |actor| {
        actor.threads[&thread].turn.current.is_none()
    });
    let state = &rig.actor.threads[&thread].turn;
    assert!(
        state.agent.is_none() && state.preparing.is_none(),
        "nothing launches before the commit returns"
    );
    rig.release_held();
    until_idle(&mut rig, thread);
    // The old agent's scenario expects a prompt, so it answers the change by exiting.
    rig.run_until("the mode change's answer", |_| {
        !matches!(mode.try_recv(), Ok(None))
    });
    rig.settle_commits();
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), "fake-agent");
    let log = rig.log(thread);
    assert_eq!(
        attached(&log),
        ["sess-1", "sess-1"],
        "the change relaunched the old agent, the new one never started: {log:?}"
    );
    assert_eq!(
        detaches(&log).first(),
        Some(&DetachReason::Closed),
        "the old session's end is logged once the refused commit drains: {log:?}"
    );
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::AgentSwitched(_))),
        0
    );
    rig.finish();
}

/// The log's helper starts and ends, detaches and switches, in order.
fn helper_story(log: &[DomainEvent]) -> Vec<String> {
    log.iter()
        .filter_map(|event| {
            if matches!(event, DomainEvent::HelperStarted(_)) {
                Some("started".to_owned())
            } else if let DomainEvent::HelperEnded(ended) = event {
                Some(format!("ended {:?}", ended.outcome))
            } else if let DomainEvent::AgentSessionDetached(detached) = event {
                Some(format!("detached {:?}", detached.reason))
            } else if matches!(event, DomainEvent::AgentSwitched(_)) {
                Some("switched".to_owned())
            } else {
                None
            }
        })
        .collect()
}

/// Whether `thread`'s view holds no helper open.
fn no_helper_open(rig: &mut Rig, thread: ThreadId) -> bool {
    rig.actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .is_some_and(|state| state.open_helpers() == 0)
}

/// A switch whose old agent has stopped, with a helper of that agent's session open (put in
/// once the switch is past its check for background work): the turn's first prompt, then the
/// switch held at its log read and the helper started.
fn switch_with_a_helper_open(rig: &mut Rig, thread: ThreadId) {
    let turn = send_prompt(rig, 10, thread);
    until_idle(rig, thread);
    dispatch_then_hold(
        rig,
        11,
        send(thread, Some(OTHER)),
        "the switch's log",
        switch_log,
    );
    turn_helpers::start(&mut rig.actor, thread, turn, open_start()).unwrap();
    rig.settle_commits();
    rig.release_held();
    until_idle(rig, thread);
}

/// A helper of the old session still open when the switch commits ends `Stopped` in that
/// commit, before the old session's detach, as at any detach (`helpers::on_detach`): its card
/// does not show running on. The thread forgets the old session's helpers once it is durable.
#[test]
fn a_helper_open_at_the_switch_commit_ends_stopped_with_the_detach() {
    let mut rig = two_agents("switch-helper-commit", Some(PROMPTS_END));
    let thread = rig.thread();
    switch_with_a_helper_open(&mut rig, thread);
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
    let log = rig.log(thread);
    assert_eq!(
        helper_story(&log),
        ["started", "ended Stopped", "detached Switched", "switched"],
        "{log:?}"
    );
    assert!(no_helper_open(&mut rig, thread), "the card is closed");
    assert!(rig.actor.threads[&thread].turn.helpers.is_none());
    rig.finish();
}

/// The same switch whose commit the store refuses: the helper's end went with it, so the
/// `Closed` detach logged for the old session once the stream drains ends it `Stopped`, once.
#[test]
fn a_helper_open_at_a_refused_switch_commit_ends_with_the_closed_detach() {
    let mut rig = two_agents("switch-helper-refused", Some(PROMPTS_END));
    let thread = rig.thread();
    rig.fail(1, |e| matches!(e, DomainEvent::AgentSwitched(_)));
    switch_with_a_helper_open(&mut rig, thread);
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), "fake-agent");
    let log = rig.log(thread);
    assert_eq!(
        helper_story(&log),
        ["started", "ended Stopped", "detached Closed"],
        "{log:?}"
    );
    // The refused commit's drain unloaded the stream: the log above is the card's end.
    assert!(rig.actor.threads[&thread].turn.helpers.is_none());
    rig.finish();
}
