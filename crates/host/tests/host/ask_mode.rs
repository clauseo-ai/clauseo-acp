//! A Claude conversation whose auto-mode checks cannot work runs in ask mode (`default`), with
//! every deny rule kept (`actor/turn/session/ask_mode.rs`):
//!
//! - a launch whose flag settings would give Claude Code an auto-mode context over budget is
//!   switched to `default`, never `auto`, with one UC-491 notice; one under budget goes to `auto`;
//! - a tool result carrying Claude Code's refusal because the request carried no auto-mode
//!   context (the withheld kind) gives one UC-492 notice and marks the agent: the next prompt
//!   relaunches it in `default`, and neither the replayed refusal nor a later answer quoting it
//!   does anything more (no loop);
//! - a service blip ("gave no verdict (error)") with a small rule list never changes the mode: a
//!   turn that ends on one gets one UC-800 line, one whose retry runs gets none; at an estimate
//!   of 80% of the limit or more, a turn that ends on a blip relaunches in `default`;
//! - each notice shows once per conversation: a host started again that runs into the refusal
//!   once more switches without a second UC-492, and none follows a UC-491.

use std::collections::BTreeMap;
use std::ffi::OsString;

use clauseo_host::ProfileHook;
use clauseo_protocol::{AgentId, DomainEvent, NoticeKind, StopReason, StreamId, ThreadId};
use serde_json::{Map, Value, json};

use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// A profile hook whose Claude `_meta` holds `rules` deny rules in its flag settings, each a
/// `Read` of another matter's folder (about 200 B of auto-mode context apiece).
struct DenyRules {
    rules: usize,
}

impl ProfileHook for DenyRules {
    fn session_meta(
        &self,
        agent: &AgentId,
        _packs: Option<&clauseo_profile::PacksWritten>,
    ) -> Option<Map<String, Value>> {
        let deny: Vec<String> = (0..self.rules)
            .map(|n| {
                format!("Read(//c/Users/clauseo-lawyer/Documents/Matters/Seeded Matter {n:05}/**)")
            })
            .collect();
        let meta =
            json!({"claudeCode": {"options": {"settings": {"permissions": {"deny": deny}}}}});
        (agent.as_str() == CLAUDE)
            .then(|| meta.as_object().cloned())
            .flatten()
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }
}

/// Enough rules for about 1 MB of auto-mode context, over the 900,000 B budget.
const OVER: usize = 5_000;

/// Rules for 836,040 B of auto-mode context: past 80% of the limit (832,000 B) alone, and under
/// the budget with the data folder's own rules (about 40-60 KB here).
const LARGE: usize = 4_272;

/// Every `ModeChanged`'s mode, in order.
fn modes(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ModeChanged(changed) => Some(changed.mode_id.to_string()),
            _ => None,
        })
        .collect()
}

/// How many notices of `kind` the thread has.
fn notices(events: &Events, kind: &NoticeKind) -> usize {
    events.count(
        |event| matches!(event, DomainEvent::Notice(notice) if notice.kind.as_ref() == Some(kind)),
    )
}

/// Runs one prompt on `thread` to its end and returns the thread's events so far.
fn turn(host: &Running, thread: ThreadId, text: &str) -> Events {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, text);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    events
}

#[test]
fn an_over_budget_launch_runs_in_default_with_one_notice() {
    let dir = Dir::new(
        "ask_mode_over_budget",
        &[(CLAUDE, Scenario::Local("ask-mode-over-budget"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_profile_hook(Box::new(DenyRules { rules: OVER }))
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    turn(&host, thread, "Summarise clause 4.");
    let events = turn(&host, thread, "And clause 5?");
    // The scenario takes `set_mode default` and no `set_mode auto`.
    assert_eq!(modes(&events), ["default"], "{}", events.dump());
    assert_eq!(
        notices(&events, &NoticeKind::AskModeTooManyMatters),
        1,
        "{}",
        events.dump()
    );
    assert_eq!(notices(&events, &NoticeKind::AskModeChecksStopped), 0);
    host.shutdown();

    // Started again under budget, the conversation goes to auto; Claude Code refuses a call for
    // want of its checks anyway: no UC-492 after the UC-491.
    dir.set_agents(&[(CLAUDE, Scenario::Local("ask-mode-again"))]);
    let host = dir.start(1);
    let events = turn(&host, thread, "And clause 6?");
    assert_eq!(modes(&events), ["default", "auto"], "{}", events.dump());
    assert_eq!(
        notices(&events, &NoticeKind::AskModeTooManyMatters),
        1,
        "{}",
        events.dump()
    );
    assert_eq!(
        notices(&events, &NoticeKind::AskModeChecksStopped),
        0,
        "{}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn an_under_budget_launch_stays_in_auto() {
    let dir = Dir::new(
        "ask_mode_under_budget",
        &[(CLAUDE, Scenario::Local("ask-mode-under-budget"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_profile_hook(Box::new(DenyRules { rules: 300 }))
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let events = turn(&host, thread, "Summarise clause 4.");
    assert_eq!(modes(&events), ["auto"], "{}", events.dump());
    assert_eq!(notices(&events, &NoticeKind::AskModeTooManyMatters), 0);
    host.shutdown();
}

#[test]
fn a_refusal_for_want_of_the_checks_relaunches_in_default_once() {
    let dir = Dir::new(
        "ask_mode_detected",
        &[(CLAUDE, Scenario::Local("ask-mode-detected"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    // Two refused calls in the turn: one notice.
    let events = turn(&host, thread, "Convert the memo.");
    assert_eq!(modes(&events), ["auto"], "{}", events.dump());
    assert_eq!(
        notices(&events, &NoticeKind::AskModeChecksStopped),
        1,
        "{}",
        events.dump()
    );
    // The next prompt relaunches the agent: `session/load`, then `set_mode default`.
    turn(&host, thread, "Try again.");
    // An answer quoting the refusal in `default`: no notice, no relaunch (the scenario's third
    // prompt goes to the same agent).
    let events = turn(&host, thread, "What happened?");
    assert_eq!(modes(&events), ["auto", "default"], "{}", events.dump());
    assert_eq!(
        notices(&events, &NoticeKind::AskModeChecksStopped),
        1,
        "{}",
        events.dump()
    );
    assert_eq!(notices(&events, &NoticeKind::AskModeTooManyMatters), 0);
    host.shutdown();

    // A host started again decides from the estimate: auto; refused once more, the
    // conversation switches again with no second UC-492.
    dir.set_agents(&[(CLAUDE, Scenario::Local("ask-mode-again"))]);
    let host = dir.start(1);
    let events = turn(&host, thread, "Once more.");
    assert_eq!(
        modes(&events),
        ["auto", "default", "auto"],
        "{}",
        events.dump()
    );
    assert_eq!(
        notices(&events, &NoticeKind::AskModeChecksStopped),
        1,
        "{}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn a_blip_with_a_small_rule_list_gets_a_plain_line_once_a_turn_and_no_mode_change() {
    let dir = Dir::new(
        "ask_mode_blip",
        &[(CLAUDE, Scenario::Local("ask-mode-blip"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    // Two blips, the turn ends on them: one line.
    let events = turn(&host, thread, "Convert the memo.");
    assert_eq!(
        notices(&events, &NoticeKind::SafetyCheckNoAnswer),
        1,
        "{}",
        events.dump()
    );
    // A blip, then Claude's retry runs: none.
    let events = turn(&host, thread, "Edit the memo.");
    assert_eq!(
        notices(&events, &NoticeKind::SafetyCheckNoAnswer),
        1,
        "{}",
        events.dump()
    );
    // The hard-failure kind, which Claude does not retry: one more, at the turn's end.
    let events = turn(&host, thread, "Edit it again.");
    assert_eq!(
        notices(&events, &NoticeKind::SafetyCheckNoAnswer),
        2,
        "{}",
        events.dump()
    );
    // Never a mode change, and no relaunch (the scenario takes no second launch).
    assert_eq!(modes(&events), ["auto"], "{}", events.dump());
    assert_eq!(notices(&events, &NoticeKind::AskModeChecksStopped), 0);
    assert_eq!(notices(&events, &NoticeKind::AskModeTooManyMatters), 0);
    host.shutdown();
}

#[test]
fn a_turn_ending_on_blips_at_a_large_estimate_relaunches_in_default() {
    let dir = Dir::new(
        "ask_mode_blip_large",
        &[(CLAUDE, Scenario::Local("ask-mode-blip-large"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_profile_hook(Box::new(DenyRules { rules: LARGE }))
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let events = turn(&host, thread, "Edit the memo.");
    assert_eq!(modes(&events), ["auto"], "{}", events.dump());
    assert_eq!(
        notices(&events, &NoticeKind::AskModeChecksStopped),
        1,
        "{}",
        events.dump()
    );
    assert_eq!(
        notices(&events, &NoticeKind::SafetyCheckNoAnswer),
        0,
        "{}",
        events.dump()
    );
    let events = turn(&host, thread, "Try again.");
    assert_eq!(modes(&events), ["auto", "default"], "{}", events.dump());
    assert_eq!(notices(&events, &NoticeKind::AskModeChecksStopped), 1);
    host.shutdown();
}
