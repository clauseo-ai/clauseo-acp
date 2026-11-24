//! A switch's message picks (`SendPrompt.config`, `switch.rs` with `prompt_picks.rs`): the
//! new agent's first session starts with them and runs the turn with them, before its
//! `TurnStarted`, and the handover's `to` names the model and effort they come to on that agent.

use clauseo_protocol::{
    AgentChoiceId, AgentId, AgentModels, CommandOutcome, ConfigPicks, DeliveryRequest, DomainEvent,
    HostCommand, ModelEntry, NamedChoice, PromptBlock,
};

use super::switch::{OTHER, one_turn, two_agents_new, until_idle};

/// The new agent's options: Opus at high, Sonnet offered.
const OPTIONS_OPUS_HIGH: &str = r#"[{"id":"model","name":"Model","category":"model","type":"select","currentValue":"opus","options":[{"value":"opus","name":"Opus 5.5"},{"value":"sonnet","name":"Sonnet 5.5"}]},{"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"high","options":[{"value":"low","name":"Low"},{"value":"medium","name":"Medium"},{"value":"high","name":"High"}]}]"#;

/// The new agent's steps after `session/new`: the start sets the message's model, then its
/// effort, and only then comes the prompt.
const PICKS_THEN_PROMPT: &str = concat!(
    r#"{"expect":{"method":"session/set_config_option","as":"m","params":{"sessionId":"sess-2","configId":"model","value":"sonnet"}}}"#,
    "\n",
    r#"{"respond":{"to":"m","result":{"configOptions":[{"id":"model","name":"Model","category":"model","type":"select","currentValue":"sonnet","options":[{"value":"opus","name":"Opus 5.5"},{"value":"sonnet","name":"Sonnet 5.5"}]},{"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"medium","options":[{"value":"low","name":"Low"},{"value":"medium","name":"Medium"},{"value":"high","name":"High"}]}]}}}"#,
    "\n",
    r#"{"expect":{"method":"session/set_config_option","as":"e","params":{"sessionId":"sess-2","configId":"effort","value":"low"}}}"#,
    "\n",
    r#"{"respond":{"to":"e","result":{"configOptions":[{"id":"model","name":"Model","category":"model","type":"select","currentValue":"sonnet","options":[{"value":"opus","name":"Opus 5.5"},{"value":"sonnet","name":"Sonnet 5.5"}]},{"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"low","options":[{"value":"low","name":"Low"},{"value":"medium","name":"Medium"},{"value":"high","name":"High"}]}]}}}"#,
    "\n",
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

fn named(id: &str, name: &str) -> NamedChoice {
    NamedChoice {
        id: id.into(),
        name: name.into(),
    }
}

fn choice(id: &str) -> AgentChoiceId {
    AgentChoiceId::new(id).unwrap()
}

/// What the host learnt of [`OTHER`]'s models before the switch, as a probe gives them.
fn probed() -> AgentModels {
    let efforts = vec![
        named("low", "Low"),
        named("medium", "Medium"),
        named("high", "High"),
    ];
    let model = |id: &str, name: &str| ModelEntry {
        id: id.into(),
        name: name.into(),
        description: None,
        efforts: efforts.clone(),
        default_effort: Some("medium".into()),
    };
    AgentModels {
        model_option: "model".into(),
        effort_option: Some("effort".into()),
        models: vec![model("opus", "Opus 5.5"), model("sonnet", "Sonnet 5.5")],
        current_model: "opus".into(),
        current_effort: Some("high".into()),
    }
}

#[test]
fn a_switch_hands_the_message_picks_to_the_new_session_before_its_turn_starts() {
    let new = format!(r#"{{"sessionId":"sess-2","configOptions":{OPTIONS_OPUS_HIGH}}}"#);
    let mut rig = two_agents_new("switch-picks", Some((&new, PICKS_THEN_PROMPT)));
    let thread = rig.thread();
    let kept = rig.actor.agent_options.keep_probed(OTHER, probed());
    assert!(kept.is_ok(), "{kept:?}");
    one_turn(&mut rig, 10, thread);
    let picks = ConfigPicks::new(
        [
            (choice("model"), choice("sonnet")),
            (choice("effort"), choice("low")),
        ]
        .into(),
    )
    .unwrap();
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "ChatGPT, review".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
        config: Some(picks),
        agent: Some(AgentId::new(OTHER).unwrap()),
    };
    let sent = rig.dispatch(11, command);
    let Ok(CommandOutcome::PromptAccepted { turn, .. }) = sent else {
        panic!("not accepted: {sent:?}");
    };
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    let at = |pick: &dyn Fn(&DomainEvent) -> bool| {
        log.iter()
            .position(pick)
            .unwrap_or_else(|| panic!("not logged: {log:?}"))
    };
    let switched = at(&|e| matches!(e, DomainEvent::AgentSwitched(_)));
    let started = at(&|e| matches!(e, DomainEvent::TurnStarted(s) if s.turn == turn));
    assert!(switched < started, "{log:?}");
    // The new session runs the message's picks when the turn starts: the scenario answered the
    // two sets before the prompt, and `TurnStarted` names what the session holds.
    let DomainEvent::TurnStarted(start) = &log[started] else {
        panic!("not a TurnStarted: {:?}", log[started]);
    };
    assert_eq!(start.model, Some(named("sonnet", "Sonnet 5.5")), "{log:?}");
    assert_eq!(start.effort, Some(named("low", "Low")), "{log:?}");
    // The handover names the new agent at what the picks come to on it.
    let handoff = log
        .iter()
        .find_map(|event| {
            if let DomainEvent::ContextHandoff(handoff) = event {
                Some(handoff.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("no handover: {log:?}"));
    assert_eq!(handoff.to.agent.as_str(), OTHER);
    assert_eq!(handoff.to.model, Some(named("sonnet", "Sonnet 5.5")));
    assert_eq!(handoff.to.effort, Some(named("low", "Low")));
    // The message logs its own picks, and the thread is on the new agent.
    let config = log.iter().find_map(|event| {
        if let DomainEvent::UserMessage(message) = event
            && message.turn == turn
        {
            Some(message.config.clone())
        } else {
            None
        }
    });
    let config = config.unwrap_or_default();
    assert_eq!(
        config
            .get("model")
            .map(clauseo_protocol::CompactString::as_str),
        Some("sonnet"),
        "{log:?}"
    );
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
    rig.finish();
}
