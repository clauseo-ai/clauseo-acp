//! A thread whose agent changed (the host's coming agent switch: `ThreadRow::agent` changes)
//! takes nothing of the first agent's for the new one (`picks.rs`, `picks/own.rs`): not its
//! `CreateThread` config, not the options its session logged, even when nothing else was
//! cleared at the switch (the session id and the config kept in memory).

use clauseo_protocol::{
    AgentChoiceId, AgentId, CompactString, ConfigCategory, ConfigChoice, ConfigOption,
    ConfigOptionKind, ConfigOptionsReplaced, ConfigPicks,
};
use clauseo_store::OwnPicks;

use super::rig::Rig;
use crate::actor::turn::{TurnPhase, picks};

/// A select option `id` of `category` on `values[0]`, offering `values`.
fn select(id: &str, category: ConfigCategory, values: &[&str]) -> ConfigOption {
    ConfigOption {
        id: CompactString::from(id),
        name: CompactString::from(id),
        description: None,
        category,
        kind: ConfigOptionKind::Select {
            current_value: CompactString::from(values[0]),
            options: (values.iter())
                .map(|value| ConfigChoice {
                    value: CompactString::from(*value),
                    name: CompactString::from(*value),
                    description: None,
                    group: None,
                    takes_effort: true,
                })
                .collect(),
        },
    }
}

fn id(text: &str) -> AgentChoiceId {
    AgentChoiceId::new(text).unwrap()
}

/// A thread made on Claude with model opus, whose session logged opus and high and kept them,
/// then switched to Codex with its session id and `CreateThread` config left in place: Codex's
/// start takes no Claude value, the thread keeps nothing for Codex from Claude's log, and so
/// nothing is set or told.
#[test]
fn a_thread_switched_to_another_agent_takes_none_of_the_first_agents_values() {
    let mut rig = Rig::start("kept-switch", "");
    let thread = rig.thread();
    rig.current_turn(thread, TurnPhase::Queued);
    let claude_options = vec![
        select("model", ConfigCategory::Model, &["opus", "haiku"]),
        select("effort", ConfigCategory::ThoughtLevel, &["high", "low"]),
    ];
    rig.commit(
        thread,
        vec![
            ConfigOptionsReplaced {
                options: claude_options,
            }
            .into(),
        ],
    );
    rig.settle_commits();
    let (claude, codex) = (
        AgentId::new("claude-acp").unwrap(),
        AgentId::new("codex-acp").unwrap(),
    );
    let opus = ConfigPicks::new([(id("model"), id("opus"))].into()).unwrap();
    let entry = rig.actor.threads.get_mut(&thread).unwrap();
    entry.row.acp_session_id = Some("sess-1".parse().unwrap());
    entry.row.own_picks = Some(OwnPicks([(claude.clone(), opus.clone())].into()));
    entry.picks = Some((claude.clone(), opus));
    entry.logged_by = Some(claude);
    // The switch, with nothing else cleared.
    entry.row.agent = codex.clone();

    let codex_options = [
        select("model", ConfigCategory::Model, &["gpt-6", "gpt-6-luna"]),
        select(
            "reasoning_effort",
            ConfigCategory::ThoughtLevel,
            &["medium", "low", "high"],
        ),
    ];
    let wanted = picks::wanted(&mut rig.actor, thread, &codex_options);
    assert_eq!(wanted, Vec::new(), "Codex's start takes a Claude value");
    assert_eq!(picks::own::own(&rig.actor, thread), Vec::new());
    picks::own::seed(&mut rig.actor, thread);
    let row = &rig.actor.threads[&thread].row;
    assert_eq!(row.own_picks.as_ref().and_then(|own| own.of(&codex)), None);
    rig.finish();
}
