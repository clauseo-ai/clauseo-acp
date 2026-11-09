//! The launch gate (`launch_checks::gate`, R17 PR-D2c, the orchestrator review's H1): no launch
//! of a thread runs beside an ended session's processes, whatever ended it. Each test leaves a
//! helper of the jailed Codex's first session in its process group (a zombie the test reaps,
//! `egress_wait::group`), then launches again: nothing launches, and no `ModeChanged` comes,
//! until the helper is gone. Linux only, like the group's straggler.
#![cfg(target_os = "linux")]

use std::time::Duration;

use clauseo_protocol::{AgentId, CommandOutcome, DetachReason, DomainEvent, ErrorCode, StopReason};

use crate::codex::{CODEX, codex_dir};
use crate::codex_jailed::{Linger, lingering_with, modes, send, set_mode};
use crate::egress_wait::{group, pause};
use crate::observe::Events;
use crate::support::{Dir, Running, wait};

/// The cap these tests wait with when the helper is to go away: well past the test's pause.
const CAP: Duration = Duration::from_secs(20);

/// A jailed host whose first session left a helper, with the gate's cap [`CAP`], and its
/// folder, which must outlive it.
fn linger(test: &str, plays: [&'static str; 2]) -> (Dir, Linger) {
    let dir = codex_dir(test, "codex-handshake");
    let linger = lingering_with(&dir, plays, |config| config.with_tree_wait(CAP));
    (dir, linger)
}

/// Asserts that nothing launched again and no mode was logged while the helper lingers.
fn held(host: &Running, fake: &crate::sandbox_rig::Fake, events: &mut Events) {
    pause(Duration::from_secs(1));
    host.settle();
    events.drain_ready();
    assert_eq!(fake.calls().len(), 1, "launched beside the old tree");
    assert_eq!(modes(events), ["agent-full-access"], "{}", events.dump());
}

/// A prompt sent while the helper lingers waits for it, then runs on a new launch.
fn prompt_waits(linger: Linger) {
    let (host, fake, thread, mut events, helper) = linger;
    group::until_zombie(helper);
    let (turn, _) = host.prompt(thread, "Go on.");
    held(&host, &fake, &mut events);
    group::reap(helper);
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        events.turn_text(turn),
        "Still on its own.",
        "{}",
        events.dump()
    );
    assert_eq!(fake.calls().len(), 2);
    host.shutdown();
}

/// Whether `event` is a detach for `reason`'s kind.
fn detached(event: &DomainEvent, exited: bool) -> bool {
    matches!(event, DomainEvent::AgentSessionDetached(d)
        if matches!(d.reason, DetachReason::Exited { .. }) == exited)
}

/// A switch that failed at the cap, then Read-only picked again while the helper still lives:
/// it fails the same way and launches nothing; once the helper is gone it launches read-only.
#[test]
fn read_only_picked_again_after_a_cap_failure_waits_for_the_old_tree() {
    let dir = codex_dir("gate_cap_again", "codex-handshake");
    let plays = ["codex-jailed-linger", "codex-jailed-switched"];
    let cap = Duration::from_millis(300);
    let (host, fake, thread, mut events, helper) =
        lingering_with(&dir, plays, |config| config.with_tree_wait(cap));
    for _ in 0..2 {
        let error = host.dispatch(set_mode(thread, "read-only")).unwrap_err();
        assert_eq!(error.code(), ErrorCode::AgentLaunchFailed, "{error}");
        host.settle();
        events.drain_ready();
        assert_eq!(fake.calls().len(), 1, "launched beside the old tree");
        assert_eq!(modes(&events), ["agent-full-access"], "{}", events.dump());
    }
    group::reap(helper);
    assert_eq!(host.ok(set_mode(thread, "read-only")), CommandOutcome::Done);
    events.until("ModeChanged read-only", |e| modes(e).len() == 2);
    let calls = fake.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].policy.matter_read_only());
    host.shutdown();
}

/// An idle stop, then a prompt while the helper lives: the launch waits for it.
#[test]
fn a_launch_after_an_idle_stop_waits_for_the_old_tree() {
    use crate::idle_stop::{LIMIT, idle_stopped};
    let dir = codex_dir("gate_idle", "codex-handshake");
    let plays = ["codex-jailed-linger", "codex-jailed-saved-full"];
    let linger = lingering_with(&dir, plays, |config| {
        config
            .with_tree_wait(CAP)
            .with_idle_limit(LIMIT)
            .with_live_agents(2)
    });
    let (host, fake, thread, mut events, helper) = linger;
    host.wait_armed(LIMIT);
    host.clock.advance(LIMIT);
    events.until("the idle stop", |e| e.position(idle_stopped).is_some());
    prompt_waits((host, fake, thread, events, helper));
}

/// The agent exits by itself after its turn (a crash), then a prompt while the helper lives:
/// the launch waits for it.
#[test]
fn a_launch_after_a_crash_waits_for_the_old_tree() {
    let plays = ["codex-jailed-linger-exit", "codex-jailed-saved-full"];
    let (_dir, (host, fake, thread, mut events, helper)) = linger("gate_crash", plays);
    events.until("the exit's detach", |e| {
        e.position(|e| detached(e, true)).is_some()
    });
    prompt_waits((host, fake, thread, events, helper));
}

/// A sign-in stop (`stop_agents`, then `restart_agents`), then a prompt while the helper lives:
/// the launch waits for it.
#[test]
fn a_launch_after_a_sign_in_stop_waits_for_the_old_tree() {
    let plays = ["codex-jailed-linger", "codex-jailed-saved-full"];
    let (_dir, linger) = linger("gate_sign_in", plays);
    let agent = AgentId::new(CODEX).unwrap();
    wait("stop_agents", linger.0.handle.stop_agents(agent.clone())).unwrap();
    wait("restart_agents", linger.0.handle.restart_agents(agent)).unwrap();
    prompt_waits(linger);
}

/// `SetMode read-only` with no agent attached (it crashed), while the helper lives: the launch
/// under the read-only policy, and its `ModeChanged`, wait until the helper is gone.
#[test]
fn an_unattached_switch_to_read_only_waits_for_the_old_tree() {
    let plays = ["codex-jailed-linger-exit", "codex-jailed-switched"];
    let (_dir, (host, fake, thread, mut events, helper)) = linger("gate_unattached", plays);
    events.until("the exit's detach", |e| {
        e.position(|e| detached(e, true)).is_some()
    });
    group::until_zombie(helper);
    let picked = send(&host, set_mode(thread, "read-only"));
    held(&host, &fake, &mut events);
    assert!(!picked.is_finished(), "answered beside the old tree");
    group::reap(helper);
    assert_eq!(picked.join().unwrap(), Ok(CommandOutcome::Done));
    events.until("ModeChanged read-only", |e| modes(e).len() == 2);
    let calls = fake.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].policy.matter_read_only());
    host.shutdown();
}
