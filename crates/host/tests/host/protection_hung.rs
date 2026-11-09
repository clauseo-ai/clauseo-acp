//! A sandbox read that hangs (#813's reviews): one bounded snapshot holds the detection, the
//! set-up's state and its time, so a hung `state()` holds neither the host's start nor its
//! quit; at most one snapshot thread is out, so retries during a hang join it; with launchers a
//! first snapshot with no answer is checked once more before its failed probe stands, launches
//! refused throughout; without launchers a timeout only shows in `set_up_state`; a removal
//! never takes the answer of a detection started before it.

use std::sync::atomic::Ordering;
use std::time::Duration;

use clauseo_protocol::{
    CommandOutcome, ErrorCode, HostCommand, NotProtectedCause, Protection, ProtectionTier,
    ProtectionView, ProtectionWork, UnsupportedWhy,
};
use clauseo_sandbox::{NoSandbox, SetupState};

use crate::protection::{dir, start};
use crate::protection_rig::{Gated, Plan, b, c, protection_events, subscribe, view};
use crate::support::Running;

/// A snapshot gives up after this in these tests.
const SHORT: Duration = Duration::from_millis(100);

fn failed() -> Protection {
    Protection::NotProtected {
        cause: NotProtectedCause::SetUpFailed,
    }
}

fn create_thread(host: &Running) -> HostCommand {
    HostCommand::CreateThread {
        matter: host.matters()[0].matter,
        agent: crate::keys_loading::fake(),
    }
}

/// A plan gated as `gated`, the snapshot waiting [`SHORT`] and the re-check `retry`.
fn hung(gated: Gated, retry: Duration) -> Plan {
    let mut plan = Plan::new(vec![b()], vec![SetupState::Ready]);
    plan.gated = gated;
    plan.waits = Some((SHORT, retry));
    plan
}

/// The view once `done` holds for it: read now, then again after each protection event.
fn until_view(host: &Running, done: impl Fn(&ProtectionView) -> bool) -> ProtectionView {
    let mut sub = subscribe(host);
    loop {
        let now = view(host);
        if done(&now) {
            return now;
        }
        let _next = protection_events(&mut sub, 1);
    }
}

/// Item 1: a `state()` that hangs keeps neither the start nor the quit: the host serves with a
/// failed probe (launches refused, launchers present) and stops while the read still hangs.
#[test]
fn a_hung_set_up_state_holds_neither_start_nor_quit() {
    let dir = dir("protection_hung_state");
    let (host, mut rig) = start(&dir, 0, hung(Gated::State, SHORT));
    let gate = rig.gate.take().unwrap();
    gate.started.recv().unwrap();
    let now = until_view(&host, |view| view.in_progress.is_none());
    assert_eq!(now.computer, failed());
    assert!(!now.retry_asks_admin);
    drop(host.refused(create_thread(&host), ErrorCode::SandboxBlocked));
    host.shutdown();
    drop(gate.release);
}

/// Item 2: retries while a detection hangs join its thread: three checks, one thread beside
/// the start's (each thread calls the detection once).
#[test]
fn retries_during_a_hang_start_one_detection() {
    let dir = dir("protection_hung_retries");
    let (host, mut rig) = start(&dir, 0, hung(Gated::Detect, SHORT));
    let gate = rig.gate.take().unwrap();
    for _ in 0..3 {
        assert_eq!(
            host.ok(HostCommand::RetryProtection {}),
            CommandOutcome::Done
        );
        assert_eq!(view(&host).computer, failed());
    }
    gate.started.recv().unwrap();
    assert_eq!(rig.detections.load(Ordering::SeqCst), 2);
    host.shutdown();
    drop(gate.release);
}

/// Item 3: with launchers, a first snapshot with no answer is checked once more: launches are
/// refused while the check waits, and the hung detection's late answer, joined, is taken.
#[test]
fn with_launchers_a_first_snapshot_with_no_answer_is_checked_once_more() {
    let dir = dir("protection_hung_first");
    let (host, mut rig) = start(&dir, 0, hung(Gated::Hang, Duration::from_secs(20)));
    let gate = rig.gate.take().unwrap();
    gate.started.recv().unwrap();
    let checking = view(&host);
    assert_eq!(checking.in_progress, Some(ProtectionWork::Check));
    assert_eq!(checking.computer, failed());
    drop(host.refused(create_thread(&host), ErrorCode::SandboxBlocked));
    gate.release.send(()).unwrap();
    let now = until_view(&host, |view| view.in_progress.is_none());
    assert_eq!(
        now.computer,
        Protection::Protected {
            tier: ProtectionTier::B
        }
    );
    assert_eq!(rig.detections.load(Ordering::SeqCst), 1, "the check joined");
    let _made = host.ok(create_thread(&host));
    host.shutdown();
}

/// Item 3: a re-check that gets no answer either leaves the failed probe, refusing, with no
/// second automatic check.
#[test]
fn with_launchers_a_re_check_with_no_answer_leaves_the_failed_probe() {
    let dir = dir("protection_hung_first_again");
    let (host, mut rig) = start(&dir, 0, hung(Gated::Hang, SHORT));
    let gate = rig.gate.take().unwrap();
    let now = until_view(&host, |view| view.in_progress.is_none());
    assert_eq!(now.computer, failed());
    drop(host.refused(create_thread(&host), ErrorCode::SandboxBlocked));
    // The one snapshot thread has called the detection (a loaded machine may start it late).
    gate.started.recv().unwrap();
    assert_eq!(rig.detections.load(Ordering::SeqCst), 1);
    host.shutdown();
    drop(gate.release);
}

/// Item 4: without launchers, on a seam with a set-up, a detection with no answer leaves the
/// computer not-built (nothing refused, no automatic check) and shows as a failed set-up.
#[test]
fn without_launchers_a_detection_with_no_answer_shows_only_in_set_up_state() {
    let dir = dir("protection_hung_set_up_only");
    let mut plan = hung(Gated::Hang, SHORT);
    plan.launchers = false;
    let (host, mut rig) = start(&dir, 0, plan);
    let gate = rig.gate.take().unwrap();
    let now = view(&host);
    assert_eq!(
        now.computer,
        Protection::NotProtected {
            cause: NotProtectedCause::Unsupported {
                why: UnsupportedWhy::NotBuilt
            }
        }
    );
    assert_eq!(now.set_up_state, Some(failed()));
    assert_eq!(now.in_progress, None);
    assert!(!now.retry_asks_admin);
    let _made = host.ok(create_thread(&host));
    host.shutdown();
    drop(gate.release);
}

/// The gate review's LOW: a removal never joins a detection started before it. The start's
/// detection hangs; the removal's snapshot starts a fresh one (the set-up now needed), which
/// decides, where joining the old one would have waited it out. Released after, the old one
/// answers "set up, tier B": that answer predates the removal and is never used, and the next
/// check is fresh again.
#[test]
fn a_removal_never_takes_an_answer_from_before_it() {
    let dir = dir("protection_hung_removal");
    let mut plan = Plan::new(vec![b(), c(NoSandbox::NeedsSetUp)], vec![SetupState::Ready]);
    plan.gated = Gated::First;
    plan.waits = Some((Duration::from_secs(1), SHORT));
    let (host, mut rig) = start(&dir, 0, plan);
    let gate = rig.gate.take().unwrap();
    gate.started.recv().unwrap();
    let before = until_view(&host, |view| view.in_progress.is_none());
    assert_eq!(before.computer, failed());
    let needed = Protection::NotProtected {
        cause: NotProtectedCause::NeedsSetUp,
    };
    assert_eq!(
        host.ok(HostCommand::RemoveSandboxSetup {}),
        CommandOutcome::Done
    );
    assert_eq!(view(&host).computer, needed, "the fresh detection decides");
    assert_eq!(rig.detections.load(Ordering::SeqCst), 2);
    assert_eq!(rig.setup.remove_calls.load(Ordering::SeqCst), 1);
    // The old detection answers tier B now; nobody takes it.
    drop(gate.release);
    assert_eq!(
        host.ok(HostCommand::RetryProtection {}),
        CommandOutcome::Done
    );
    assert_eq!(view(&host).computer, needed);
    host.shutdown();
}
