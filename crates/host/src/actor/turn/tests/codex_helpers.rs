//! Codex's helpers through the turn path (`turn/helpers/codex.rs`): the recorded frames of
//! `fake_agent`'s `codex-helpers` scenario (codex-acp 2.10.0, X1) through the ACP reader on a
//! Codex connection, and which connections ask for helper sessions; then `AcpEvent`s put in by
//! hand, shaped as the recordings have them (fixtures X2, X4, X5): two at once, a helper's
//! helper, the model a helper's first update names, a helper's own options, usage and title.
//! Replays, late frames, a clashing step id, a helper open at the turn's end and the order of a
//! helper's report and end are in `codex_answers.rs`.

use clauseo_acp::{AcpEvent, AgentUpdate, HelperState, MessageRole, Origin, ToolCall};
use clauseo_protocol::{
    AgentKind, CompactString, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind,
    ContentBlock, Count, DomainEvent, HelperId, PlanEntry, PlanPriority, PlanStatus, StreamId,
    TextOrBlob, ThreadId, ToolCallPatch, ToolCallStatus, ToolKind, TurnId,
};
use std::collections::HashMap;

use super::agents::send_prompt;
use super::failures::until_ended;
use super::helpers::{feed, live_turn, one_turn, session};
use super::rig::{Rig, kind};
use crate::actor::turn::agents::launch::connect_options;

/// X2's two helpers, X4's helper and its helper.
pub(super) const LEASE: &str = "01a20c2b-62d3-7ba1-b160-5033e3954e3f";
pub(super) const ORDER: &str = "01a20c2b-6e7c-7c92-97b3-c4ec0f7a4be2";
pub(super) const RELAY: &str = "01a20c2d-4f42-7fc2-a1b6-dc9ec203c47b";
const READER: &str = "01a20c2d-621a-7f52-877b-04548942bbd2";

/// `fake_agent`'s `codex-helpers` scenario after its header and handshake (the rig plays its
/// own), without its permission request and answer.
pub(super) fn recorded_steps(with_ask: bool) -> String {
    let scenario = include_str!("../../../../../fake_agent/scenarios/codex-helpers.jsonl");
    scenario
        .lines()
        .skip(2)
        .filter(|line| with_ask || !line.contains("\"perm-1\""))
        .fold(String::new(), |mut steps, line| {
            steps.push_str(line);
            steps.push('\n');
            steps
        })
}

/// The log's helper events and steps, one line each: helpers by name, with their model and
/// parent; steps by ACP id and helper.
pub(super) fn story(log: &[DomainEvent]) -> Vec<String> {
    let mut names: HashMap<HelperId, String> = HashMap::new();
    let name = |names: &HashMap<HelperId, String>, id: Option<HelperId>| match id {
        Some(id) => names.get(&id).cloned().unwrap_or_else(|| "?".to_owned()),
        None => "-".to_owned(),
    };
    let mut lines = Vec::new();
    for event in log {
        if let DomainEvent::HelperStarted(started) = event {
            names.insert(started.helper, started.name.to_string());
            let model = started.model.as_deref().unwrap_or("-");
            let parent = name(&names, started.parent);
            let shape = (started.call.is_none(), started.task.is_none());
            assert_eq!(
                shape,
                (true, true),
                "a Codex helper has no call and no task"
            );
            lines.push(format!(
                "start {} model={model} parent={parent}",
                started.name
            ));
        } else if let DomainEvent::ToolCallStarted(started) = event {
            let helper = name(&names, started.helper);
            lines.push(format!("step {} {helper}", started.acp_id));
        } else if let DomainEvent::HelperText(text) = event {
            let helper = name(&names, Some(text.helper));
            lines.push(format!("words {helper}: {}", text.text));
        } else if let DomainEvent::HelperEnded(ended) = event {
            let helper = name(&names, Some(ended.helper));
            let summary = match &ended.summary {
                Some(TextOrBlob::Inline(text)) => text.clone(),
                Some(TextOrBlob::Blob(_)) => "<blob>".to_owned(),
                None => "-".to_owned(),
            };
            lines.push(format!("end {helper} {:?} {summary}", ended.outcome));
        } else if let DomainEvent::TurnEnded(ended) = event {
            lines.push(format!("turn_ended {:?}", ended.stop));
        }
    }
    lines
}

/// Plays the recorded helper on an agent of `kind`; the thread's log.
fn played(test: &str, kind: Option<AgentKind>) -> Vec<DomainEvent> {
    let mut rig = Rig::start(test, &recorded_steps(false));
    rig.actor.turn_path.kind_for_tests = kind;
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    rig.finish();
    log
}

/// On a Codex connection the recorded helper shows: its card names the model its first update
/// gave, its words come before its read, its report is its summary and not words, and the lead's
/// `wait` stays the lead's step. Its options, usage and status touch nothing of the
/// conversation's.
#[test]
fn the_recorded_codex_helper_shows_its_model_steps_words_and_report() {
    let log = played("codex-helper-recorded", Some(AgentKind::Codex));
    assert_eq!(
        story(&log),
        [
            "start Read documents model=gpt-6.2-sol parent=-",
            "step call_3a599374996943828a280bc01e0e8cb4 -",
            "words Read documents: I",
            "step exec-7fd37d45-aacb-4501-8bc2-96ebd303db13 Read documents",
            "end Read documents Done Lease start",
            "turn_ended EndTurn",
        ]
    );
    let kinds: Vec<String> = log.iter().map(kind).collect();
    for own in ["config_options_replaced", "usage_updated"] {
        let after_prompt = kinds.iter().skip_while(|kind| *kind != "turn_started");
        assert!(after_prompt.clone().all(|kind| kind != own), "{kinds:?}");
    }
}

/// Only a Codex connection asks for helper sessions: Claude's and `fake_agent`'s (no kind) never
/// do, so codex-acp's helper frames can only come on a Codex connection.
#[test]
fn other_connections_get_no_codex_helpers() {
    let asks = |kind| connect_options(kind).helper_sessions;
    assert!(asks(Some(AgentKind::Codex)));
    assert!(!asks(Some(AgentKind::Claude)));
    assert!(!asks(None));
}

/// Puts Codex's helper events into `thread`, as the ACP reader hands them over: by default in
/// its live `turn`, else with `origin`.
pub(super) struct Codex<'a> {
    pub(super) rig: &'a mut Rig,
    pub(super) thread: ThreadId,
    pub(super) turn: TurnId,
    /// The origin of the events whose origin the call does not name.
    pub(super) origin: Origin,
}

impl<'a> Codex<'a> {
    /// Events in `thread`'s live `turn`.
    pub(super) fn new(rig: &'a mut Rig, thread: ThreadId, turn: TurnId) -> Self {
        Self {
            rig,
            thread,
            turn,
            origin: Origin::Turn(turn),
        }
    }

    pub(super) fn at(&self) -> Origin {
        self.origin
    }

    pub(super) fn spawned(
        &mut self,
        origin: Origin,
        helper: &str,
        parent: Option<&str>,
        name: &str,
    ) {
        let event = AcpEvent::HelperSpawned {
            session: session(),
            origin,
            helper: helper.into(),
            parent: parent.map(CompactString::from),
            name: name.into(),
            task: None,
        };
        feed(self.rig, self.thread, event);
    }

    pub(super) fn update(&mut self, origin: Origin, helper: &str, update: AgentUpdate) {
        let event = AcpEvent::HelperUpdate {
            session: session(),
            origin,
            helper: helper.into(),
            update,
        };
        feed(self.rig, self.thread, event);
    }

    /// The helper's first update: its one-entry model list (X5.jsonl:27).
    pub(super) fn model(&mut self, helper: &str, model: &str) {
        let at = self.at();
        self.update(at, helper, options(model));
    }

    pub(super) fn step(&mut self, helper: &str, acp_id: &str) {
        let at = self.at();
        self.update(at, helper, step(acp_id));
        let done = AgentUpdate::ToolCallUpdate {
            acp_id: acp_id.into(),
            patch: ToolCallPatch {
                status: Some(ToolCallStatus::Completed),
                ..ToolCallPatch::default()
            },
        };
        self.update(at, helper, done);
    }

    pub(super) fn words(&mut self, helper: &str, text: &str) {
        let at = self.at();
        let chunk = AgentUpdate::MessageChunk {
            role: MessageRole::Answer,
            acp_message_id: None,
            content: ContentBlock::Text {
                text: text.to_owned(),
            },
        };
        self.update(at, helper, chunk);
    }

    pub(super) fn answer(&mut self, helper: &str, text: &str) {
        let event = AcpEvent::HelperAnswer {
            session: session(),
            origin: self.at(),
            helper: helper.into(),
            content: ContentBlock::Text {
                text: text.to_owned(),
            },
        };
        feed(self.rig, self.thread, event);
    }

    pub(super) fn ended(&mut self, origin: Origin, helper: &str, state: HelperState) {
        let event = AcpEvent::HelperEnded {
            session: session(),
            origin,
            helper: helper.into(),
            state,
        };
        feed(self.rig, self.thread, event);
    }
}

/// A Codex helper's options: its model, the only choice.
pub(super) fn options(model: &str) -> AgentUpdate {
    AgentUpdate::ConfigOptions(vec![ConfigOption {
        id: "model".into(),
        name: "Model".into(),
        description: Some("Model Codex uses for the subagent".to_owned()),
        category: ConfigCategory::Model,
        kind: ConfigOptionKind::Select {
            current_value: model.into(),
            options: vec![ConfigChoice {
                value: model.into(),
                name: model.into(),
                description: None,
                group: None,
                takes_effort: false,
            }],
        },
    }])
}

/// A Codex helper's command step, as codex-acp names them (`exec-<uuid>`).
pub(super) fn step(acp_id: &str) -> AgentUpdate {
    AgentUpdate::ToolCall(ToolCall {
        acp_id: acp_id.into(),
        title: CompactString::from("Read file"),
        kind: ToolKind::Read,
        tool_name: Some("exec_command".into()),
        status: ToolCallStatus::InProgress,
        locations: Vec::new(),
        raw_input: None,
        raw_output: None,
        content: Vec::new(),
    })
}

/// Runs `play` during a live turn of a prompt answered `end_turn`; the story after the turn.
pub(super) fn story_of(test: &str, play: impl FnOnce(&mut Codex<'_>)) -> Vec<String> {
    story_after(test, play, |_| {}).0
}

/// Runs `play` during a live turn of a prompt answered `end_turn`, then `after` once the turn
/// has ended, with events `Idle` by default; the story then, and how many helpers the view
/// leaves open.
pub(super) fn story_after(
    test: &str,
    play: impl FnOnce(&mut Codex<'_>),
    after: impl FnOnce(&mut Codex<'_>),
) -> (Vec<String>, u32) {
    let mut rig = Rig::start(test, &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    play(&mut Codex::new(&mut rig, thread, turn));
    rig.release_held();
    until_ended(&mut rig, thread);
    {
        let mut codex = Codex::new(&mut rig, thread, turn);
        codex.origin = Origin::Idle;
        after(&mut codex);
    }
    until_ended(&mut rig, thread);
    let lines = story(&rig.log(thread));
    let open = rig
        .actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .map_or(0, clauseo_reducer::ThreadState::open_helpers);
    rig.finish();
    (lines, open)
}

/// Two helpers at once (X2): each card names its own model; one whose first update is a step
/// starts without a model, before its step; their steps, words and reports stay apart; a failed
/// one keeps the report it had.
#[test]
fn two_codex_helpers_at_once_keep_their_own_steps_words_and_reports() {
    let lines = story_of("codex-helpers-two", |codex| {
        let at = codex.at();
        codex.spawned(at, LEASE, None, "Lease");
        codex.spawned(at, ORDER, None, "Order");
        codex.model(LEASE, "gpt-6-luna");
        codex.step(ORDER, "exec-b1");
        codex.step(LEASE, "exec-a1");
        codex.words(LEASE, "Reading the lease.");
        codex.answer(ORDER, "Order of 9 May");
        codex.answer(LEASE, "Term 1 April 2024");
        codex.answer(LEASE, " to 31 March 2026");
        codex.ended(at, LEASE, HelperState::Completed);
        codex.ended(at, ORDER, HelperState::Failed);
    });
    assert_eq!(
        lines,
        [
            "step tc-0 -",
            "start Lease model=gpt-6-luna parent=-",
            "start Order model=- parent=-",
            "step exec-b1 Order",
            "step exec-a1 Lease",
            "words Lease: Reading the lease.",
            "end Lease Done Term 1 April 2024 to 31 March 2026",
            "end Order Failed Order of 9 May",
            "turn_ended EndTurn",
        ]
    );
}

/// A helper's helper (X4) names its parent; a helper still waiting for its model when its own
/// helper is spawned starts first, without one. Its end on the parent's session ends it alone.
#[test]
fn a_codex_helpers_helper_names_its_parent() {
    let lines = story_of("codex-helpers-nested", |codex| {
        let at = codex.at();
        codex.spawned(at, RELAY, None, "Relay");
        codex.spawned(at, READER, Some(RELAY), "Reader");
        codex.model(READER, "gpt-6-luna");
        codex.step(READER, "exec-r1");
        codex.answer(READER, "3 matches");
        codex.ended(at, READER, HelperState::Completed);
        codex.words(RELAY, "The worker found 3 matches.");
        codex.ended(at, RELAY, HelperState::Completed);
    });
    assert_eq!(
        lines,
        [
            "step tc-0 -",
            "start Relay model=- parent=-",
            "start Reader model=gpt-6-luna parent=Relay",
            "step exec-r1 Reader",
            "end Reader Done 3 matches",
            "words Relay: The worker found 3 matches.",
            "end Relay Done -",
            "turn_ended EndTurn",
        ]
    );
}

/// A helper's own options after its first, its usage, its title, its plan and its mode change
/// nothing of the conversation's: the log is the log of the same helper without them, and the
/// lead's plan, sent before, is the only plan in it.
#[test]
fn a_codex_helpers_options_usage_title_plan_and_mode_leave_the_conversations_alone() {
    let run = |test: &str, extra: bool| {
        let mut rig = Rig::start(test, &one_turn("end_turn"));
        let thread = rig.thread();
        let turn = live_turn(&mut rig, thread);
        let mut codex = Codex::new(&mut rig, thread, turn);
        let at = codex.at();
        let lead_plan = AgentUpdate::Plan(vec![plan_entry("Read the lease")]);
        let lead = AcpEvent::Update {
            session: session(),
            origin: at,
            update: lead_plan,
        };
        feed(codex.rig, thread, lead);
        codex.spawned(at, LEASE, None, "Lease");
        codex.model(LEASE, "gpt-6-luna");
        if extra {
            let plan = AgentUpdate::Plan(vec![plan_entry("Find the rent")]);
            codex.update(at, LEASE, plan);
            let mode_id = "read-only".into();
            codex.update(at, LEASE, AgentUpdate::CurrentMode { mode_id });
            codex.update(at, LEASE, options("gpt-6.2-sol"));
            let usage = AgentUpdate::Usage {
                used: Count::new(12869).unwrap(),
                size: Count::new(475_000).unwrap(),
                cost: None,
            };
            codex.update(at, LEASE, usage);
            let title = Some("Read the lease".to_owned());
            codex.update(at, LEASE, AgentUpdate::SessionTitle { title });
        }
        codex.ended(at, LEASE, HelperState::Completed);
        rig.release_held();
        until_ended(&mut rig, thread);
        let log = rig.log(thread);
        rig.finish();
        let plans: Vec<Vec<String>> = log
            .iter()
            .filter_map(|event| {
                let DomainEvent::PlanReplaced(plan) = event else {
                    return None;
                };
                Some(
                    plan.entries
                        .iter()
                        .map(|entry| entry.content.clone())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(plans, [["Read the lease"]], "{test}");
        log.iter().map(kind).collect::<Vec<String>>()
    };
    let with = run("codex-helper-own-updates", true);
    assert_eq!(with, run("codex-helper-no-own-updates", false));
    assert!(with.iter().any(|kind| kind == "helper_ended"), "{with:?}");
}

/// A plan entry saying `content`, pending.
fn plan_entry(content: &str) -> PlanEntry {
    PlanEntry {
        content: content.to_owned(),
        priority: PlanPriority::Medium,
        status: PlanStatus::Pending,
    }
}
