//! The second review of the agent switch (`switch.rs`): a handover owed across a restart; a
//! message to another agent that the old agent's sign-in or limit hold does not hold; the old
//! agent's picks and session gone at the commit; a picker change during a switch checked
//! against the new agent; a switch's picks kept out of the old agent's start config; a
//! handover delivered only once the new agent took its prompt; and no copy of the header keeping
//! the old agent's options, mode, commands or tools past the switch.

use clauseo_protocol::{
    AgentChoiceId, AgentId, CommandOutcome, ConfigCategory, ConfigOption, ConfigOptionKind,
    ConfigPicks, ConfigValue, DeliveryRequest, DomainEvent, ErrorCode, HandoffStatus, HeaderEntry,
    HostCommand, PromptBlock, StopReason, StreamId, ThreadId,
};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::rig::Rig;
use super::switch::{
    OTHER, count, one_turn, send, send_text, two_agents, two_agents_both, until_idle,
};
use crate::actor::turn::{AgentReply, LogFor, TurnInput, picks, queue, switch};

/// The new agent's steps: two prompts, each answered `end_turn`.
const TWO_PROMPTS: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
    r#"{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// The new agent's steps: a prompt it fails.
const PROMPT_FAILS: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","error":{"code":-32603,"message":"Internal error"}}}"#,
    "\n",
);

const FAKE: &str = "fake-agent";

fn agent(name: &str) -> AgentId {
    AgentId::new(name).unwrap()
}

fn choice(id: &str) -> AgentChoiceId {
    AgentChoiceId::new(id).unwrap()
}

/// The result of the commit logging `AgentSwitched`.
fn switch_commit(actor: &Actor, input: &Input) -> bool {
    matches!(input, Input::Committed { ticket, .. }
        if actor.pipeline.events(*ticket).iter()
            .any(|e| matches!(e.event, DomainEvent::AgentSwitched(_))))
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

/// A prompt's answer.
fn prompt_reply(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Turn(TurnInput::Agent {
            reply: AgentReply::Prompt { .. },
            ..
        })
    )
}

/// Dispatches `command` as command `n` and holds back the first input `hold` picks.
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

fn switches(log: &[DomainEvent]) -> usize {
    count(log, |e| matches!(e, DomainEvent::AgentSwitched(_)))
}

/// Waits out start-up, resumes a queue it restored held, and runs the thread idle.
fn after_restart(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("start-up's repairs", |actor| {
        actor.work.loads == 0 && actor.work.commits == 0
    });
    if !queue::queued_turns(&rig.actor, thread).is_empty() {
        let resumed = rig.dispatch(30, HostCommand::ResumeQueue { thread });
        assert!(resumed.is_ok(), "{resumed:?}");
    }
    until_idle(rig, thread);
}

/// The next turn after a restart carried the owed handover: a second `ContextHandoff`,
/// delivered, on the new agent.
fn carried_after_restart(rig: &mut Rig, thread: ThreadId) {
    one_turn(rig, 31, thread);
    let log = rig.log(thread);
    let handoffs = count(&log, |e| matches!(e, DomainEvent::ContextHandoff(_)));
    assert_eq!(handoffs, 2, "the next turn carries the handover: {log:?}");
    assert_eq!(
        settled(&log).last(),
        Some(&HandoffStatus::Delivered),
        "{log:?}"
    );
    assert_eq!(switches(&log), 1, "{log:?}");
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
    assert_eq!(switch::seen(&rig.actor, thread).1, None, "nothing owed now");
}

/// A crash after the switch's commit is durable and before its prompt went out: start-up
/// settles the handover `Failed` and owes it, so the next turn carries it.
#[test]
fn a_restart_after_the_switch_commit_owes_the_handover() {
    let mut rig = two_agents("review-restart-commit", Some(TWO_PROMPTS));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the switch's commit",
        switch_commit,
    );
    rig.held.clear();
    let mut rig = rig.restart_unstopped(1);
    rig.run_until("start-up's repairs", |actor| {
        actor.work.loads == 0 && actor.work.commits == 0
    });
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
    let (_, owed) = switch::seen(&rig.actor, thread);
    assert_eq!(owed.as_ref().map(AgentId::as_str), Some(FAKE));
    after_restart(&mut rig, thread);
    carried_after_restart(&mut rig, thread);
    rig.finish();
}

/// The new agent may not start once the switch is committed: the turn fails, the handover
/// settles `Failed` and is owed, across a restart too, and the retry carries it.
#[test]
fn a_new_agent_that_fails_to_start_owes_the_handover_to_the_retry() {
    let mut rig = two_agents("review-launch-fails", Some(TWO_PROMPTS));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the switch's commit",
        switch_commit,
    );
    let code = ErrorCode::SigninPolicyUnreadable;
    let refused = &mut rig.actor.wiring.sign_in.refused;
    refused.set(agent(OTHER), Some(code)).unwrap();
    rig.release_held();
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    let failed = log.iter().find_map(|event| {
        if let DomainEvent::TurnFailed(failed) = event {
            Some(failed.code)
        } else {
            None
        }
    });
    assert_eq!(failed, Some(code), "{log:?}");
    assert_eq!(settled(&log), [HandoffStatus::Failed], "{log:?}");
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
    let (_, owed) = switch::seen(&rig.actor, thread);
    assert_eq!(owed.as_ref().map(AgentId::as_str), Some(FAKE));
    let refused = &mut rig.actor.wiring.sign_in.refused;
    refused.set(agent(OTHER), None).unwrap();
    let mut rig = rig.restart(1);
    rig.run_until("start-up's repairs", |actor| {
        actor.work.loads == 0 && actor.work.commits == 0
    });
    let (_, owed) = switch::seen(&rig.actor, thread);
    assert_eq!(
        owed.as_ref().map(AgentId::as_str),
        Some(FAKE),
        "read back from the failed handover"
    );
    after_restart(&mut rig, thread);
    carried_after_restart(&mut rig, thread);
    rig.finish();
}

/// `SendPrompt` of `text` to `thread`, sent now, naming `agent`.
fn send_now(thread: ThreadId, agent: Option<&str>) -> HostCommand {
    HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "ChatGPT, review".to_owned(),
        }],
        delivery: DeliveryRequest::SendNow,
        config: None,
        agent: agent.map(|agent| AgentId::new(agent).unwrap()),
    }
}

/// The thread's agent is held for its sign-in: a message sent now to another agent is not held
/// behind it, and switches at once.
#[test]
fn a_switch_away_from_an_agent_held_for_its_sign_in_goes_through() {
    let mut rig = two_agents("review-sign-in-away", Some(TWO_PROMPTS));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    let hold = crate::agent::login_link::StopFor::SignInAgain;
    rig.actor.wiring.sign_in.held.insert(agent(FAKE), hold);
    let sent = rig.dispatch(11, send_now(thread, Some(OTHER)));
    assert!(
        matches!(sent, Ok(CommandOutcome::PromptAccepted { .. })),
        "{sent:?}"
    );
    assert_eq!(
        queue::queued_turns(&rig.actor, thread),
        [],
        "not queued behind the sign-in"
    );
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(switches(&log), 1, "{log:?}");
    let ends = count(
        &log,
        |e| matches!(e, DomainEvent::TurnEnded(t) if t.stop == StopReason::EndTurn),
    );
    assert_eq!(ends, 2, "{log:?}");
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
    rig.actor.wiring.sign_in.held.remove(&agent(FAKE));
    rig.finish();
}

/// The queue is held because the thread's last turn stopped at its agent's limit: a message
/// queued to another agent starts at once and switches, and the queue moves on after it, on
/// the new agent.
#[test]
fn a_switch_away_from_an_agent_at_its_limit_goes_through() {
    let mut rig = two_agents("review-limit-away", Some(TWO_PROMPTS));
    let thread = rig.thread();
    let sent = rig.dispatch_holding(10, send(thread, None), prompt_reply);
    assert!(sent.is_ok(), "{sent:?}");
    if rig.held.is_empty() {
        rig.hold_next("the first answer", prompt_reply);
    }
    let sent = rig.dispatch(11, send_text(thread, None, "Then clause 6."));
    assert!(sent.is_ok(), "{sent:?}");
    queue::hold_for_limit(&mut rig.actor, thread);
    rig.release_held();
    rig.run_until("the first turn ended", |actor| {
        actor.threads[&thread].turn.current.is_none()
            && actor.threads[&thread].turn.closing.is_none()
    });
    rig.settle_commits();
    assert_eq!(queue::queued_turns(&rig.actor, thread).len(), 1, "held");
    let sent = rig.dispatch(12, send_text(thread, Some(OTHER), "ChatGPT, review"));
    assert!(sent.is_ok(), "{sent:?}");
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(switches(&log), 1, "{log:?}");
    let ends = count(
        &log,
        |e| matches!(e, DomainEvent::TurnEnded(t) if t.stop == StopReason::EndTurn),
    );
    assert_eq!(ends, 3, "the switch, then the held turn: {log:?}");
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
    rig.finish();
}

/// The switch's commit lets go of the old agent's `CreateThread` picks and its session: with
/// the commit in flight the thread has neither, and the message's own picks alone are the new
/// session's start config (none here).
#[test]
fn the_switch_commit_drops_the_old_agents_picks_and_session() {
    let mut rig = two_agents("review-picks-dropped", Some(TWO_PROMPTS));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    let old = ConfigPicks::new([(choice("model"), choice("opus"))].into()).unwrap();
    let entry = rig.actor.threads.get_mut(&thread).unwrap();
    entry.picks = Some((entry.row.agent.clone(), old));
    assert!(entry.row.acp_session_id.is_some());
    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the switch's commit",
        switch_commit,
    );
    let entry = &rig.actor.threads[&thread];
    assert_eq!(entry.picks, None, "the old agent's picks went");
    assert_eq!(entry.row.acp_session_id, None, "the old session went");
    assert_eq!(entry.row.agent.as_str(), OTHER);
    rig.release_held();
    until_idle(&mut rig, thread);
    rig.finish();
}

/// A message naming another agent keeps its picks out of the old agent's start config, even
/// while the thread has no session: they are the new agent's.
#[test]
fn a_switch_messages_picks_stay_out_of_the_old_agents_start() {
    let mut rig = two_agents("review-picks-claim", Some(TWO_PROMPTS));
    let thread = rig.thread();
    let picks = ConfigPicks::new([(choice("effort"), choice("low"))].into()).unwrap();
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "ChatGPT, review".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
        config: Some(picks.clone()),
        agent: Some(agent(OTHER)),
    };
    dispatch_then_hold(&mut rig, 10, command, "the switch's log", switch_log);
    let entry = &rig.actor.threads[&thread];
    assert_eq!(entry.row.acp_session_id, None);
    assert_eq!(entry.picks, None, "not the old agent's");
    rig.release_held();
    rig.run_until("the switch committed", |actor| {
        actor.threads[&thread].row.agent.as_str() == OTHER
    });
    assert_eq!(
        rig.actor.threads[&thread].picks,
        Some((agent(OTHER), picks)),
        "the new agent's start config"
    );
    until_idle(&mut rig, thread);
    rig.finish();
}

/// An agent's `Ended`.
fn ended(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Agent {
            input: crate::agent::AgentInput::Ended(_),
            ..
        }
    )
}

/// An effort option `effort` offering low and high.
fn effort_option() -> ConfigOption {
    let choice = |value: &str, name: &str| clauseo_protocol::ConfigChoice {
        value: value.into(),
        name: name.into(),
        description: None,
        group: None,
        takes_effort: true,
    };
    ConfigOption {
        id: "effort".into(),
        name: "Effort".into(),
        description: None,
        category: ConfigCategory::ThoughtLevel,
        kind: ConfigOptionKind::Select {
            current_value: "high".into(),
            options: vec![choice("low", "Low"), choice("high", "High")],
        },
    }
}

/// A picker change while the old agent stops for a switch waits for the new agent, so the new
/// agent's kept options judge it, not the old agent's.
#[test]
fn a_pick_during_a_switch_is_checked_against_the_new_agent() {
    let mut rig = two_agents("review-pick-stopping", Some(TWO_PROMPTS));
    let thread = rig.thread();
    let kept = rig.actor.agent_options.keep(OTHER, &[effort_option()]);
    assert!(kept.is_ok(), "{kept:?}");
    one_turn(&mut rig, 10, thread);
    let low = ConfigValue::Select(choice("low"));
    // The old agent offers no effort: on it the pick is refused.
    let refused = picks::settable(&mut rig.actor, thread, &choice("effort"), &low);
    assert!(refused.is_err(), "{refused:?}");
    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the old agent's end",
        ended,
    );
    let during = picks::settable(&mut rig.actor, thread, &choice("effort"), &low);
    assert!(during.is_ok(), "{during:?}");
    let unknown = picks::settable(&mut rig.actor, thread, &choice("speed"), &low);
    assert!(unknown.is_err(), "still checked: {unknown:?}");
    rig.release_held();
    until_idle(&mut rig, thread);
    rig.finish();
}

/// The new agent fails the prompt carrying the handover: it was not delivered, and stays owed.
#[test]
fn a_prompt_the_new_agent_fails_leaves_the_handover_owed() {
    let mut rig = two_agents("review-prompt-fails", Some(PROMPT_FAILS));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    let sent = rig.dispatch(11, send(thread, Some(OTHER)));
    assert!(sent.is_ok(), "{sent:?}");
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(switches(&log), 1, "{log:?}");
    assert_eq!(settled(&log), [HandoffStatus::Failed], "{log:?}");
    let (_, owed) = switch::seen(&rig.actor, thread);
    assert_eq!(owed.as_ref().map(AgentId::as_str), Some(FAKE));
    rig.finish();
}

/// The old agent's session: a model and an effort option, and a mode.
const OLD_NEW: &str = r#"{"sessionId":"sess-1","modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"Ask"},{"id":"code","name":"Code"}]},"configOptions":[{"id":"model","name":"Model","category":"model","type":"select","currentValue":"opus","options":[{"value":"opus","name":"Opus 5.5"},{"value":"sonnet","name":"Sonnet 5.5"}]},{"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"high","options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]}]}"#;

/// The old agent's steps: a prompt during which it lists a command, answered `end_turn`; then
/// it exits.
const OLD_STEPS: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#,
    "\n",
    r#"{"update":{"update":{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"review","description":"Review the draft"}]}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
    r#"{"exit":{"code":0}}"#,
    "\n",
);

/// The thread's header as the host reads it: the loaded stream's, else the one it kept.
fn read_header(actor: &Actor, thread: ThreadId) -> Option<HeaderEntry> {
    actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(actor.threads[&thread].header.as_ref())
        .cloned()
}

/// Whether `header` holds none of a session's options, mode or commands.
fn bare(header: &HeaderEntry) -> bool {
    header.config_options.is_empty() && header.mode_id.is_none() && header.commands.is_empty()
}

/// The model and effort the last turn of `log` started with.
fn last_started_with(log: &[DomainEvent]) -> String {
    log.iter()
        .rev()
        .find_map(|event| {
            if let DomainEvent::TurnStarted(start) = event {
                Some(format!("{:?} {:?}", start.model, start.effort))
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("no TurnStarted: {log:?}"))
}

/// A switch leaves no copy of the header with the old agent's option lists, mode, commands or
/// tools: the header kept for the unloaded stream loses them at the commit, before the fold;
/// the new agent, whose session reports none, starts with none; and a restart folds none.
#[test]
fn a_switch_keeps_none_of_the_old_agents_options_in_any_header() {
    let mut rig = two_agents_both(
        "review-header-options",
        Some((OLD_NEW, OLD_STEPS)),
        Some((r#"{"sessionId":"sess-2"}"#, TWO_PROMPTS)),
    );
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    let old = read_header(&rig.actor, thread).expect("a header");
    assert!(
        old.config_options.len() == 2 && old.mode_id.is_some() && !old.commands.is_empty(),
        "the old agent's session reported its options, mode and commands: {old:?}"
    );
    // The old agent exited, so nothing needs the stream between the turns: it unloads, and the
    // actor keeps its header.
    rig.run_until("the old agent gone", |actor| {
        actor.threads[&thread].turn.agent.is_none() && actor.work.commits == 0
    });
    rig.actor.release(StreamId::Thread(thread));
    let kept = rig.actor.threads[&thread].header.clone();
    assert!(
        kept.as_ref().is_some_and(|kept| !bare(kept)),
        "the unloaded stream's kept header holds them: {kept:?}"
    );

    dispatch_then_hold(
        &mut rig,
        11,
        send(thread, Some(OTHER)),
        "the switch's commit",
        switch_commit,
    );
    // Sent, not folded: the kept header already holds none of them.
    let kept = rig.actor.threads[&thread].header.clone().expect("kept");
    assert!(bare(&kept) && kept.tools.is_empty(), "{kept:?}");
    assert_eq!(kept.agent.as_str(), OTHER);
    assert_eq!((kept.model, kept.effort), (None, None));
    rig.release_held();
    until_idle(&mut rig, thread);

    // The new session reported no options: the host reads none, and its turn ran with none.
    let log = rig.log(thread);
    assert_eq!(last_started_with(&log), "None None", "{log:?}");
    assert_eq!(picks::thread_options(&rig.actor, thread), []);
    let after = read_header(&rig.actor, thread).expect("a header");
    assert!(bare(&after), "{after:?}");
    assert_eq!(after.agent.as_str(), OTHER);

    // A restart folds the log: still none.
    let mut rig = rig.restart(1);
    after_restart(&mut rig, thread);
    one_turn(&mut rig, 40, thread);
    let log = rig.log(thread);
    assert_eq!(last_started_with(&log), "None None", "{log:?}");
    let restarted = read_header(&rig.actor, thread).expect("a header");
    assert!(bare(&restarted), "{restarted:?}");
    assert_eq!(picks::thread_options(&rig.actor, thread), []);
    rig.finish();
}
