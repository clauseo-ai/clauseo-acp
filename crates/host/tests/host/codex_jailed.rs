//! A jailed Codex's modes through the host and the launcher seam (R17 PR-D2c, the
//! orchestrator's rulings B and 1): it always runs the agent-full-access preset, and its two
//! modes are the jail's policies. `SetMode` or `SetConfigOption` naming another mode is refused
//! with `sandbox.ask_mode_unavailable` and never reaches the agent; a switch relaunches it at the
//! turn boundary under the new policy, its `ModeChanged` logged only once the new agent runs; a
//! thread whose saved mode asks launches in the fallback; the agent's own mode changes are ignored.

use std::sync::Arc;
use std::thread::JoinHandle;
#[cfg(target_os = "linux")]
use std::time::Duration;

use clauseo_protocol::{
    AgentChoiceId, CommandEnvelope, CommandOutcome, ConfigOption, ConfigOptionKind, ConfigValue,
    DetachReason, DomainEvent, ErrorCode, HostCommand, HostError, Seq, StopReason, StreamId,
    ThreadId,
};

use crate::codex::{CODEX, codex_dir, run_turn};
use crate::observe::Events;
use crate::sandbox_rig::{Answer, Env, Fake, start_run};
use crate::support::{Running, Scenario, command_id, matter_with_thread, wait};

pub(crate) fn set_mode(thread: ThreadId, mode: &str) -> HostCommand {
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
pub(crate) fn modes(events: &Events) -> Vec<String> {
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
    // The mode it runs in is answered by the host: no call, no event, no relaunch.
    assert_eq!(
        host.ok(set_mode(thread, "agent-full-access")),
        CommandOutcome::Done
    );
    let same = set_config_mode(thread, "agent-full-access");
    assert_eq!(host.ok(same), CommandOutcome::Done);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(modes(&events), ["agent-full-access"]);
    assert_eq!(fake.calls().len(), 1);
    assert!(!fake.calls()[0].policy.matter_read_only());
    host.shutdown();
}

/// A jailed Codex that moves itself to an asking mode, and says so in its mode option: neither
/// is logged nor applied, and the session is put back in the preset, not read-only's.
#[test]
fn a_jailed_codex_moving_itself_is_ignored_and_put_back_in_the_preset() {
    let dir = codex_dir("codex_jailed_moves", "codex-handshake");
    dir.set_agents(&[(CODEX, Scenario::Local("codex-jailed-moves"))]);
    let (host, fake) = jailed(&dir, 0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_text(turn), "Moved.", "{}", events.dump());
    assert_eq!(modes(&events), ["agent-full-access"], "{}", events.dump());
    let option = mode_option(&events).unwrap();
    assert_eq!(option.0, "agent-full-access");
    assert_eq!(fake.calls().len(), 1);
    host.shutdown();
}

/// Run 0 at tier C saves `workspace-write`; run 1 is jailed and launches in
/// `jailed::SAVED_ASK_FALLBACK`, today the normal jailed policy (Rohan's ruling, 2026-11-09: no
/// Codex mode switch at launch). The agent's own mode updates change nothing.
#[test]
fn a_saved_asking_mode_starts_in_the_fallback_and_the_agent_cannot_leave_it() {
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
    assert_eq!(events.turn_text(turn), "On its own.", "{}", events.dump());
    // The fallback is the normal jailed policy; Codex runs the preset.
    assert_eq!(
        mode_var(&fake.envs()[0]).as_deref(),
        Some("agent-full-access")
    );
    assert!(!fake.calls()[0].policy.matter_read_only());
    // The effective mode, logged once, and the options cut to the two at it. The agent's own
    // mode updates mid-turn changed nothing: no event, no relaunch.
    assert_eq!(modes(&events), ["agent-full-access"]);
    assert_eq!(fake.calls().len(), 1);
    assert_eq!(
        mode_option(&events),
        Some((
            "agent-full-access".to_owned(),
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
    let (host, fake) = jailed(&dir, 1);
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
    assert!(!fake.calls()[0].policy.matter_read_only());
    host.shutdown();
}

/// Sends `command` from a thread of its own, so the test goes on while it waits.
pub(crate) fn send(
    host: &Running,
    command: HostCommand,
) -> JoinHandle<Result<CommandOutcome, HostError>> {
    let handle = host.handle.clone();
    std::thread::spawn(move || {
        let envelope = CommandEnvelope {
            command_id: command_id(),
            command,
        };
        wait("dispatch", handle.dispatch(envelope))
    })
}

/// The switch's order: the lawyer picks Read-only mid-turn and nothing changes while the turn
/// runs; the turn ends, the agent stops (`Restarting`) and is launched again under the read-only
/// policy with its session resumed, and only then is `ModeChanged` logged; the next prompt runs
/// there.
#[test]
fn a_switch_to_read_only_relaunches_at_the_turn_boundary_then_logs_the_mode() {
    let dir = codex_dir("codex_jailed_switch", "codex-handshake");
    let (host, fake) = jailed(&dir, 0);
    let plays = ["codex-jailed-switch", "codex-jailed-switched"];
    fake.play(&plays.map(Scenario::Local));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Edit clause 1.");
    events.until("the permission request", |e| {
        !e.permission_requests().is_empty()
    });
    let picked = send(&host, set_mode(thread, "read-only"));
    host.settle();
    events.drain_ready();
    // Held to the boundary: no event and no relaunch while the turn runs.
    assert!(!picked.is_finished());
    assert_eq!(modes(&events), ["agent-full-access"]);
    assert_eq!(fake.calls().len(), 1);
    let request = events.permission_requests()[0];
    let option_id = AgentChoiceId::new("allow-once").unwrap();
    let answer = HostCommand::AnswerPermission {
        thread,
        request,
        option_id,
    };
    assert_eq!(host.ok(answer), CommandOutcome::Done);
    assert_eq!(picked.join().unwrap(), Ok(CommandOutcome::Done));
    events.until("ModeChanged read-only", |e| modes(e).len() == 2);
    let ended = events.position(|e| matches!(e, DomainEvent::TurnEnded(end) if end.turn == first));
    let detached = events.position(|e| {
        matches!(e, DomainEvent::AgentSessionDetached(d) if d.reason == DetachReason::Restarting)
    });
    let attached = (events.events().enumerate())
        .filter(|(_, e)| matches!(e, DomainEvent::AgentSessionAttached(_)))
        .nth(1)
        .map(|(at, _)| at);
    let changed =
        events.position(|e| matches!(e, DomainEvent::ModeChanged(c) if c.mode_id == "read-only"));
    let order = [ended, detached, attached, changed];
    assert!(order.iter().all(Option::is_some), "{}", events.dump());
    assert!(order.is_sorted(), "{order:?}: {}", events.dump());
    // Launched again under the read-only policy; Codex runs the preset in both.
    let calls = fake.calls();
    assert_eq!(calls.len(), 2);
    assert!(!calls[0].policy.matter_read_only());
    assert!(calls[1].policy.matter_read_only());
    let envs = fake.envs();
    assert!(
        envs.iter()
            .all(|env| mode_var(env).as_deref() == Some("agent-full-access"))
    );
    let second = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(second),
        "Read-only now.",
        "{}",
        events.dump()
    );
    let started = events.position(|e| crate::observe::started(e, second));
    assert!(started > changed, "{}", events.dump());
    assert_eq!(modes(&events), ["agent-full-access", "read-only"]);
    assert_eq!(fake.calls().len(), 2);
    host.shutdown();
}

/// A relaunch for a switch that fails before its session attaches: the `SetMode` fails with
/// the launch's error, no `ModeChanged` is logged, and the mode stays as it was, so the next
/// prompt launches under the normal policy.
#[test]
fn a_failed_relaunch_logs_no_mode_and_keeps_the_mode() {
    let dir = codex_dir("codex_jailed_switch_fails", "codex-handshake");
    let (host, fake) = jailed(&dir, 0);
    let plays = [
        "codex-jailed-new",
        "codex-jailed-switch-fails",
        "codex-jailed-saved-full",
    ];
    fake.play(&plays.map(Scenario::Local));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    run_turn(&host, &mut events, thread);
    let error = host.dispatch(set_mode(thread, "read-only")).unwrap_err();
    assert!(error.code().as_str().starts_with("agent."), "{error}");
    host.settle();
    events.drain_ready();
    assert_eq!(modes(&events), ["agent-full-access"], "{}", events.dump());
    assert!(fake.calls()[1].policy.matter_read_only());
    let turn = run_turn(&host, &mut events, thread);
    assert_eq!(
        events.turn_text(turn),
        "Still on its own.",
        "{}",
        events.dump()
    );
    assert!(!fake.calls()[2].policy.matter_read_only());
    assert!(!modes(&events).iter().any(|mode| mode == "read-only"));
    host.shutdown();
}

/// A jailed host whose seam plays `plays`, with the tree wait's cap `cap`.
#[cfg(target_os = "linux")]
fn lingering(dir: &crate::support::Dir, plays: [&'static str; 2], cap: Duration) -> Linger {
    lingering_with(dir, plays, |config| config.with_tree_wait(cap))
}

/// A jailed host whose seam plays `plays`, its config adjusted by `adjust`, after one turn
/// that left a helper in the agent's group (`plays[0]`): with the helper's pid.
#[cfg(target_os = "linux")]
pub(crate) fn lingering_with(
    dir: &crate::support::Dir,
    plays: [&'static str; 2],
    adjust: impl FnOnce(clauseo_host::HostConfig) -> clauseo_host::HostConfig,
) -> Linger {
    use crate::egress_wait::group;
    group::become_reaper();
    let (fake, _) = Fake::new(dir, Answer::Launch(Vec::new()));
    let (host, _, _) = start_run(dir, 0, Some(Arc::clone(&fake)), adjust);
    fake.play(&plays.map(Scenario::Local));
    let (_, thread, folder) = matter_with_thread(dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    run_turn(&host, &mut events, thread);
    let helper = group::pid(&folder);
    (host, fake, thread, events, helper)
}

#[cfg(target_os = "linux")]
pub(crate) type Linger = (Running, Arc<Fake>, ThreadId, Events, nix::unistd::Pid);

/// A switch to Read-only while a helper of the read-write session's tree lingers: neither the
/// relaunch nor `ModeChanged` comes until the helper is gone (`launch_checks::relaunch`).
#[cfg(target_os = "linux")]
#[test]
fn a_switch_relaunches_and_logs_the_mode_only_once_the_old_tree_is_gone() {
    use crate::egress_wait::{group, pause};
    let dir = codex_dir("codex_jailed_linger", "codex-handshake");
    let plays = ["codex-jailed-linger", "codex-jailed-switched"];
    let (host, fake, thread, mut events, helper) = lingering(&dir, plays, Duration::from_secs(20));
    let picked = send(&host, set_mode(thread, "read-only"));
    group::until_zombie(helper);
    pause(Duration::from_secs(1));
    host.settle();
    events.drain_ready();
    assert!(!picked.is_finished(), "answered beside the old tree");
    assert_eq!(fake.calls().len(), 1, "relaunched beside the old tree");
    assert_eq!(modes(&events), ["agent-full-access"]);
    group::reap(helper);
    assert_eq!(picked.join().unwrap(), Ok(CommandOutcome::Done));
    events.until("ModeChanged read-only", |e| modes(e).len() == 2);
    let calls = fake.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].policy.matter_read_only());
    host.shutdown();
}

/// A helper of the old tree that outlives the wait's cap: the switch fails, nothing relaunches,
/// no `ModeChanged` is logged and the mode stays, so the next prompt runs read-write.
#[cfg(target_os = "linux")]
#[test]
fn a_tree_that_outlives_the_wait_fails_the_switch_with_no_mode() {
    use crate::egress_wait::group;
    let dir = codex_dir("codex_jailed_linger_cap", "codex-handshake");
    let plays = ["codex-jailed-linger", "codex-jailed-saved-full"];
    let (host, fake, thread, mut events, helper) =
        lingering(&dir, plays, Duration::from_millis(300));
    let error = host.dispatch(set_mode(thread, "read-only")).unwrap_err();
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed, "{error}");
    host.settle();
    events.drain_ready();
    assert_eq!(fake.calls().len(), 1);
    assert_eq!(modes(&events), ["agent-full-access"], "{}", events.dump());
    group::reap(helper);
    let turn = run_turn(&host, &mut events, thread);
    assert_eq!(
        events.turn_text(turn),
        "Still on its own.",
        "{}",
        events.dump()
    );
    assert!(!fake.calls()[1].policy.matter_read_only());
    assert!(!modes(&events).iter().any(|mode| mode == "read-only"));
    host.shutdown();
}

/// Two mode commands queued while the relaunch waits for the old tree: the last picks the
/// launch's policy, the first is answered as replaced at the attach, and nothing relaunches
/// again (replaying the first would relaunch for it, with the same queue, for ever).
#[cfg(target_os = "linux")]
#[test]
fn two_queued_switches_launch_once_in_the_last_mode() {
    use crate::egress_wait::group;
    let dir = codex_dir("codex_jailed_two_queued", "codex-handshake");
    let plays = ["codex-jailed-linger", "codex-jailed-switched"];
    let (host, fake, thread, mut events, helper) = lingering(&dir, plays, Duration::from_secs(20));
    let first = send(&host, set_mode(thread, "read-only"));
    group::until_zombie(helper);
    host.settle();
    let second = send(&host, set_mode(thread, "agent-full-access"));
    host.settle();
    assert!(!first.is_finished() && !second.is_finished());
    group::reap(helper);
    assert_eq!(first.join().unwrap(), Ok(CommandOutcome::Done));
    assert_eq!(second.join().unwrap(), Ok(CommandOutcome::Done));
    host.settle();
    events.drain_ready();
    let calls = fake.calls();
    assert_eq!(calls.len(), 2, "{}", events.dump());
    assert!(!calls[1].policy.matter_read_only());
    assert!(!modes(&events).iter().any(|mode| mode == "read-only"));
    let detaches = (events.events())
        .filter(|e| matches!(e, DomainEvent::AgentSessionDetached(_)))
        .count();
    assert_eq!(detaches, 1, "{}", events.dump());
    host.shutdown();
}
