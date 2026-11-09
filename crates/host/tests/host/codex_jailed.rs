//! A jailed Codex's modes through the host and the launcher seam (R17 PR-D2c, the
//! orchestrator's ruling B): it launches in the jailed preset and offers only read-only and the
//! preset; `SetMode` or `SetConfigOption` naming another mode is refused with
//! `sandbox.ask_mode_unavailable` and never reaches the agent; a thread whose saved mode asks
//! starts read-only, logged once, and a saved preset stays after a restart; a move to an asking
//! mode by itself is switched to read-only.

use std::sync::Arc;

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, ConfigOption, ConfigOptionKind, ConfigValue, DomainEvent,
    ErrorCode, HostCommand, Seq, StopReason, StreamId, ThreadId,
};

use crate::codex::{CODEX, codex_dir, run_turn};
use crate::observe::Events;
use crate::sandbox_rig::{Answer, Env, Fake, start_run};
use crate::support::{Running, Scenario, matter_with_thread};

fn set_mode(thread: ThreadId, mode: &str) -> HostCommand {
    HostCommand::SetMode {
        thread,
        mode_id: AgentChoiceId::new(mode).unwrap(),
    }
}

fn set_config_mode(thread: ThreadId, mode: &str) -> HostCommand {
    HostCommand::SetConfigOption {
        thread,
        option_id: AgentChoiceId::new("mode").unwrap(),
        value: ConfigValue::Select(AgentChoiceId::new(mode).unwrap()),
    }
}

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

/// The last logged mode option's current value and values.
fn mode_option(events: &Events) -> Option<(String, Vec<String>)> {
    let options: Vec<&ConfigOption> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ConfigOptionsReplaced(replaced) => Some(&replaced.options),
            _ => None,
        })
        .flatten()
        .filter(|option| option.id == "mode")
        .collect();
    match &options.last()?.kind {
        ConfigOptionKind::Select {
            current_value,
            options,
        } => Some((
            current_value.to_string(),
            options
                .iter()
                .map(|choice| choice.value.to_string())
                .collect(),
        )),
        _ => None,
    }
}

fn mode_var(env: &Env) -> Option<String> {
    env.get(&std::ffi::OsString::from("INITIAL_AGENT_MODE"))
        .and_then(|value| value.to_str())
        .map(str::to_owned)
}

/// A jailed host (fake launchers), as run `run`.
fn jailed(dir: &crate::support::Dir, run: u16) -> (Running, Arc<Fake>) {
    let (fake, _) = Fake::new(dir, Answer::Launch(Vec::new()));
    let (host, _, _) = start_run(dir, run, Some(Arc::clone(&fake)), |config| config);
    (host, fake)
}

#[test]
fn a_jailed_codex_starts_in_the_preset_offers_two_modes_and_refuses_the_rest() {
    let dir = codex_dir("codex_jailed_new", "codex-jailed-new");
    dir.set_agents(&[(CODEX, Scenario::Local("codex-jailed-new"))]);
    let (host, fake) = jailed(&dir, 0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_ended(turn).map(|ended| ended.stop),
        Some(StopReason::EndTurn),
        "{}",
        events.dump()
    );
    assert_eq!(events.turn_text(turn), "Working on its own.");
    // Launched through the seam in the preset; the session reported it, so no switch.
    assert_eq!((fake.reserved(), fake.calls().len()), (1, 1));
    let envs = fake.envs();
    assert_eq!(mode_var(&envs[0]).as_deref(), Some("agent-full-access"));
    assert_eq!(modes(&events), ["agent-full-access"]);
    assert_eq!(
        mode_option(&events),
        Some((
            "agent-full-access".to_owned(),
            vec!["read-only".to_owned(), "agent-full-access".to_owned()]
        )),
        "{}",
        events.dump()
    );

    // The asking modes are refused here; the scenario fails on any call but read-only's.
    for mode in ["workspace-write", "agent"] {
        host.refused(set_mode(thread, mode), ErrorCode::SandboxAskModeUnavailable);
        host.refused(
            set_config_mode(thread, mode),
            ErrorCode::SandboxAskModeUnavailable,
        );
    }
    assert_eq!(host.ok(set_mode(thread, "read-only")), CommandOutcome::Done);
    // Back to the preset through the option: its answer's four modes are logged as two.
    let back = set_config_mode(thread, "agent-full-access");
    assert_eq!(host.ok(back), CommandOutcome::Done);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(modes(&events), ["agent-full-access", "read-only"]);
    assert_eq!(
        mode_option(&events),
        Some((
            "agent-full-access".to_owned(),
            vec!["read-only".to_owned(), "agent-full-access".to_owned()]
        ))
    );
    host.shutdown();
}

/// A jailed Codex that moves itself to an asking mode is switched to read-only.
#[test]
fn a_jailed_codex_moving_itself_to_an_asking_mode_is_switched_to_read_only() {
    let dir = codex_dir("codex_jailed_moves", "codex-handshake");
    dir.set_agents(&[(CODEX, Scenario::Local("codex-jailed-moves"))]);
    let (host, _fake) = jailed(&dir, 0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    run_turn(&host, &mut events, thread);
    events.until("ModeChanged read-only", |e| modes(e).len() == 3);
    events.assert_no_mismatch();
    assert_eq!(
        modes(&events),
        ["agent-full-access", "workspace-write", "read-only"],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// Run 0 at tier C saves `workspace-write`; run 1 is jailed and starts read-only.
#[test]
fn a_saved_asking_mode_starts_read_only_when_codex_becomes_jailed() {
    let dir = codex_dir("codex_jailed_saved_ask", "codex-handshake");
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    run_turn(&host, &mut events, thread);
    assert_eq!(
        modes(&events).last().map(String::as_str),
        Some("workspace-write")
    );
    let head = events.head();
    host.shutdown();

    dir.set_agents(&[(CODEX, Scenario::Local("codex-jailed-saved-ask"))]);
    let (host, fake) = jailed(&dir, 1);
    let mut events = host.events_after(StreamId::Thread(thread), Seq::new(head).ok());
    events.until_caught_up();
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_text(turn), "Read-only.", "{}", events.dump());
    assert_eq!(
        mode_var(&fake.envs()[0]).as_deref(),
        Some("agent-full-access")
    );
    // The effective mode, logged once, and the options cut to the two at it.
    assert_eq!(modes(&events), ["read-only"]);
    assert_eq!(
        mode_option(&events),
        Some((
            "read-only".to_owned(),
            vec!["read-only".to_owned(), "agent-full-access".to_owned()]
        ))
    );
    host.shutdown();
}

/// Run 0 jailed saves the preset; run 1 jailed resumes in it, with no switch.
#[test]
fn a_saved_preset_stays_after_a_restart() {
    let dir = codex_dir("codex_jailed_saved_full", "codex-handshake");
    dir.set_agents(&[(CODEX, Scenario::Local("codex-jailed-new"))]);
    let (host, _fake) = jailed(&dir, 0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    run_turn(&host, &mut events, thread);
    assert_eq!(modes(&events), ["agent-full-access"]);
    let head = events.head();
    host.shutdown();

    dir.set_agents(&[(CODEX, Scenario::Local("codex-jailed-saved-full"))]);
    let (host, _fake) = jailed(&dir, 1);
    let mut events = host.events_after(StreamId::Thread(thread), Seq::new(head).ok());
    events.until_caught_up();
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(turn),
        "Still on its own.",
        "{}",
        events.dump()
    );
    assert_eq!(modes(&events), ["agent-full-access"]);
    host.shutdown();
}
