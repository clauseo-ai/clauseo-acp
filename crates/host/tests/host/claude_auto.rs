//! Claude's mode per launch through the host (R17 PR-D2d; Rohan, 2026-11-09): a jailed launch
//! (the fake launchers) that is not forced ask switches to `auto` before its first prompt and
//! leaves Claude's own moves alone; every other launch kind asks (`default`), with a forced-ask
//! drift set back; a thread loaded once jailed and once unjailed gets each launch's mode, never a
//! stored one. A refused auto falls back by its cause, never failing the launch: `default` for a
//! `disableAutoMode` setting, `acceptEdits` for the plan, the model or an adapter offering no
//! auto, each told to the thread once.

use std::path::PathBuf;

use clauseo_host::HostConfig;
use clauseo_protocol::{
    DomainEvent, FolderSupportKind, NoticeKind, StopReason, StreamId, ThreadId,
};

use crate::observe::Events;
use crate::sandbox_rig::{Answer, Fake, start_run};
use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// The guard program a forced-ask launch needs; the fake agent never runs it.
const GUARD: &str = "/opt/clauseo/clauseo-desktop";

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

/// Every host notice's kind, in order (an agent's own notice has none).
fn notice_kinds(events: &Events) -> Vec<NoticeKind> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => notice.kind.clone(),
            _ => None,
        })
        .collect()
}

/// Runs one prompt on `thread` to its end, with no scenario mismatch.
fn turn(host: &Running, events: &mut Events, thread: ThreadId) {
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, "Summarise clause 4.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
}

/// How a launch is made: through the fake launchers (jailed) or not, and in forced ask or not.
#[derive(Clone, Copy)]
struct Launch {
    jailed: bool,
    forced_ask: bool,
}

const JAILED: Launch = Launch {
    jailed: true,
    forced_ask: false,
};
const TIER_C: Launch = Launch {
    jailed: false,
    forced_ask: false,
};

/// A host as run `run` launching as `launch`, each launch playing the next of `plays` (the last
/// one again).
fn launch_host(dir: &Dir, run: u16, launch: Launch, plays: &[Scenario]) -> Running {
    let adjust = move |config: HostConfig| {
        if launch.forced_ask {
            config
                .with_folder_support(Some(FolderSupportKind::Network))
                .with_pre_write_guard(PathBuf::from(GUARD))
        } else {
            config
        }
    };
    if launch.jailed {
        let (fake, _) = Fake::new(dir, Answer::Launch(Vec::new()));
        fake.play(plays);
        start_run(dir, run, Some(fake), adjust).0
    } else {
        let (host, _, counting) = start_run(dir, run, None, adjust);
        counting.play(plays);
        host
    }
}

fn dir(test: &str, first: &'static str) -> Dir {
    Dir::new(test, &[(CLAUDE, Scenario::Local(first))])
}

#[test]
fn a_jailed_launch_switches_to_auto_and_leaves_claude_s_own_moves_alone() {
    let dir = dir("claude_auto_new", "claude-auto-new");
    let host = launch_host(&dir, 0, JAILED, &[Scenario::Local("claude-auto-new")]);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread);
    // Its start, then Claude's own plan and back, logged as they came, never set back.
    assert_eq!(
        modes(&events),
        ["auto", "plan", "auto"],
        "{}",
        events.dump()
    );
    assert!(notice_kinds(&events).is_empty(), "{}", events.dump());
    host.shutdown();
}

/// No unjailed launch runs in auto or in its fallback, whatever the launch kind: a new session at
/// tier C, forced ask (unjailed, and jailed too), a `session/load` after a host restart, and a
/// relaunch after the agent exited, the last two loading a session that ran in auto. Each
/// scenario expects exactly the `session/set_mode` calls its row allows, so a switch to auto is a
/// mismatch. R17 PR-D2f's "protection switched off" (auto without the jail) adds its row here.
#[test]
fn no_unjailed_launch_runs_in_auto() {
    struct Row {
        kind: &'static str,
        launch: Launch,
        /// A jailed run that ran the thread in auto first (a host restart then loads it).
        auto_first: bool,
        plays: &'static [Scenario],
        modes: &'static [&'static str],
    }
    let rows = [
        Row {
            kind: "tier C, new session",
            launch: TIER_C,
            auto_first: false,
            plays: &[Scenario::Local("claude-default-new")],
            modes: &["default"],
        },
        Row {
            kind: "tier C, forced ask (drift set back)",
            launch: Launch {
                jailed: false,
                forced_ask: true,
            },
            auto_first: false,
            plays: &[Scenario::Local("claude-auto-forced")],
            // Claude's own move, logged as it came, then the set-back.
            modes: &["default", "acceptEdits", "default"],
        },
        Row {
            kind: "jailed, forced ask (drift set back)",
            launch: Launch {
                jailed: true,
                forced_ask: true,
            },
            auto_first: false,
            plays: &[Scenario::Local("claude-auto-forced")],
            // Claude's own move, logged as it came, then the set-back.
            modes: &["default", "acceptEdits", "default"],
        },
        Row {
            kind: "tier C, session/load after a restart of a jailed run",
            launch: TIER_C,
            auto_first: true,
            plays: &[Scenario::Local("claude-default-load")],
            modes: &["auto", "plan", "auto", "default"],
        },
        Row {
            kind: "tier C, relaunch after the agent exited",
            launch: TIER_C,
            auto_first: false,
            plays: &[
                Scenario::Local("claude-default-exit"),
                Scenario::Local("claude-default-load"),
            ],
            modes: &["default", "default"],
        },
    ];
    for (index, row) in rows.iter().enumerate() {
        let dir = dir(
            &format!("claude_auto_unjailed_{index}"),
            "claude-default-new",
        );
        let mut run = 0;
        let thread = if row.auto_first {
            let host = launch_host(&dir, run, JAILED, &[Scenario::Local("claude-auto-new")]);
            let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
            let mut events = host.events(StreamId::Thread(thread));
            turn(&host, &mut events, thread);
            host.shutdown();
            run += 1;
            Some(thread)
        } else {
            None
        };
        let host = launch_host(&dir, run, row.launch, row.plays);
        let thread = thread
            .unwrap_or_else(|| matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE).1);
        let mut events = host.events(StreamId::Thread(thread));
        for _ in 0..row.plays.len() {
            turn(&host, &mut events, thread);
        }
        assert_eq!(modes(&events), row.modes, "{}: {}", row.kind, events.dump());
        // This launch's own start (the last attach's mode) asks.
        let started = modes(&events)
            .into_iter()
            .nth(usize::from(row.auto_first) * 3);
        assert_eq!(started.as_deref(), Some("default"), "{}", row.kind);
        assert!(notice_kinds(&events).is_empty(), "{}", row.kind);
        host.shutdown();
    }
}

/// The same thread loaded jailed, then unjailed, then jailed again (as when its matter moves to
/// a network folder and back): each launch's mode, whatever the session ran in before.
#[test]
fn the_same_thread_gets_each_launch_s_mode_on_load() {
    let dir = dir("claude_auto_reload", "claude-auto-new");
    let host = launch_host(&dir, 0, JAILED, &[Scenario::Local("claude-auto-new")]);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread);
    host.shutdown();

    let host = launch_host(&dir, 1, TIER_C, &[Scenario::Local("claude-default-load")]);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread);
    assert_eq!(modes(&events), ["auto", "plan", "auto", "default"]);
    host.shutdown();

    let host = launch_host(&dir, 2, JAILED, &[Scenario::Local("claude-auto-load")]);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread);
    assert_eq!(
        modes(&events),
        ["auto", "plan", "auto", "default", "auto"],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// Runs a jailed thread through `plays` (one turn each) and returns its events.
fn jailed_turns(test: &str, plays: &'static [Scenario]) -> Events {
    let dir = dir(test, "claude-auto-new");
    let host = launch_host(&dir, 0, JAILED, plays);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    for _ in 0..plays.len() {
        turn(&host, &mut events, thread);
    }
    host.shutdown();
    events
}

/// A `disableAutoMode` setting turned auto off: the session asks (`default`), the launch goes on,
/// and the thread is told once, though the relaunch is refused auto again. The firm's managed
/// policy and the lawyer's own settings give the same refusal on the wire (claude-agent-acp
/// 0.89.1 with CLI 2.1.293: "Cannot set permission mode to auto: auto mode disabled by
/// settings", measured both ways with the pinned packages), so one scenario plays both rows.
#[test]
fn auto_turned_off_by_a_setting_asks_and_tells_the_thread_once() {
    for (index, source) in ["managed policy", "user settings"].into_iter().enumerate() {
        let events = jailed_turns(
            &format!("claude_auto_turned_off_{index}"),
            &[
                Scenario::Local("claude-auto-turned-off"),
                Scenario::Local("claude-auto-turned-off-load"),
            ],
        );
        assert_eq!(modes(&events), ["default", "default"], "{source}");
        assert_eq!(
            notice_kinds(&events),
            [NoticeKind::ClaudeAutoTurnedOff],
            "{source}: {}",
            events.dump()
        );
    }
}

/// Auto unavailable for a reason nobody at the firm chose: the session runs in `acceptEdits`
/// (edits in the matter without asking, commands ask), and the thread is told.
#[test]
fn auto_unavailable_falls_back_to_accept_edits() {
    let rows: [(&str, &'static [Scenario], &[&str]); 3] = [
        // The plan (the CLI's circuit breaker): the host sets acceptEdits itself.
        (
            "plan",
            &[Scenario::Local("claude-auto-unavailable")],
            &["acceptEdits"],
        ),
        // The model: the adapter's own fallback, seen and logged as acceptEdits.
        (
            "model",
            &[Scenario::Local("claude-auto-model")],
            &["auto", "acceptEdits"],
        ),
        // An adapter offering no auto at all.
        (
            "not_offered",
            &[Scenario::Local("claude-auto-not-offered")],
            &["acceptEdits"],
        ),
    ];
    for (cause, plays, modes_seen) in rows {
        let events = jailed_turns(&format!("claude_auto_unavailable_{cause}"), plays);
        assert_eq!(modes(&events), modes_seen, "{cause}: {}", events.dump());
        assert_eq!(
            notice_kinds(&events),
            [NoticeKind::ClaudeAutoUnavailable],
            "{cause}: {}",
            events.dump()
        );
    }
}
