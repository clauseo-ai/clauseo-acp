//! The set-up commands' edge cases (decision record 0051, R17 PR-B1, stage C's review): a
//! set-up thread that panics or never starts, a retry joining a running check, a removal
//! waiting behind a check, and a second removal joining the first.

use std::sync::atomic::Ordering;

use clauseo_protocol::{CommandOutcome, ErrorCode, HostCommand, ProtectionWork};
use clauseo_sandbox::{NoSandbox, SetupState};

use crate::protection::{dir, start};
use crate::protection_rig::{Gated, Plan, b, c, rig, send, view};
use crate::support::wait;

/// A set-up that needs running, then tier B.
fn needs_set_up() -> Plan {
    Plan::new(
        vec![c(NoSandbox::NeedsSetUp), b()],
        vec![SetupState::Needed, SetupState::Ready],
    )
}

/// The thread panics in `run()` (test builds unwind): the waiting command still gets an
/// answer, `sandbox.setup_failed`, and nothing stays in progress.
#[test]
fn a_set_up_that_panics_answers_setup_failed() {
    let dir = dir("protection_panic");
    let mut plan = needs_set_up();
    plan.panics = true;
    let (host, rig) = start(&dir, 0, plan);
    host.refused(
        HostCommand::RetryProtection {},
        ErrorCode::SandboxSetupFailed,
    );
    assert_eq!(rig.setup.run_calls.load(Ordering::SeqCst), 1);
    let after = view(&host);
    assert_eq!(after.in_progress, None);
    // The cache stays as it was: no detection ran after the panic.
    assert_eq!(rig.detections.load(Ordering::SeqCst), 1);
    host.shutdown();
}

/// The set-up thread does not start: `sandbox.setup_failed` at once, nothing ran.
#[test]
fn a_set_up_thread_that_does_not_start_answers_setup_failed() {
    let dir = dir("protection_no_thread");
    let rig = rig(needs_set_up());
    let sandbox = rig.sandbox.clone().failing_spawn();
    let host = dir.start_with(0, move |config| config.with_sandbox(sandbox));
    for command in [
        HostCommand::RetryProtection {},
        HostCommand::RemoveSandboxSetup {},
    ] {
        host.refused(command, ErrorCode::SandboxSetupFailed);
    }
    assert_eq!(rig.setup.run_calls.load(Ordering::SeqCst), 0);
    assert_eq!(rig.setup.remove_calls.load(Ordering::SeqCst), 0);
    assert_eq!(view(&host).in_progress, None);
    host.shutdown();
}

/// A retry while a check runs joins it: one detection more, both answered `Done`, no run.
#[test]
fn a_retry_during_a_check_joins_it() {
    let dir = dir("protection_check_join");
    let mut plan = Plan::new(
        vec![c(NoSandbox::SetUpFailed("another user".to_owned())), b()],
        vec![SetupState::SetUpByOtherUser],
    );
    plan.gated = Gated::Detect;
    let (host, rig) = start(&dir, 0, plan);
    let gate = rig.gate.as_ref().unwrap();
    let first = send(&host.handle, HostCommand::RetryProtection {});
    gate.started.recv().unwrap();
    assert_eq!(view(&host).in_progress, Some(ProtectionWork::Check));
    let second = send(&host.handle, HostCommand::RetryProtection {});
    // A query sent after the second is answered after it has joined.
    host.settle();
    gate.release.send(()).unwrap();
    assert_eq!(wait("the first retry", first), Ok(CommandOutcome::Done));
    assert_eq!(wait("the second retry", second), Ok(CommandOutcome::Done));
    assert_eq!(rig.detections.load(Ordering::SeqCst), 2);
    assert_eq!(rig.setup.run_calls.load(Ordering::SeqCst), 0);
    host.shutdown();
}

/// R17 PR-E follow-up: a removal asked while a check runs (a "Try again", or a refused launch's
/// re-probe) is not refused: it waits behind the check, launches are refused with
/// `sandbox.removing` meanwhile, and it runs once the check ends, then answers.
#[test]
fn a_removal_during_a_check_waits_for_it_then_runs() {
    let dir = dir("protection_check_then_remove");
    let other = || c(NoSandbox::SetUpFailed("another user".to_owned()));
    let mut plan = Plan::new(vec![other(), other()], vec![SetupState::SetUpByOtherUser]);
    plan.gated = Gated::Detect;
    let (host, rig) = start(&dir, 0, plan);
    let gate = rig.gate.as_ref().unwrap();
    // Nothing a set-up can fix: the retry is a check.
    let retry = send(&host.handle, HostCommand::RetryProtection {});
    gate.started.recv().unwrap();
    assert_eq!(view(&host).in_progress, Some(ProtectionWork::Check));
    let removal = send(&host.handle, HostCommand::RemoveSandboxSetup {});
    let matter = host.matters()[0].matter;
    let command = HostCommand::CreateThread {
        matter,
        agent: clauseo_protocol::AgentId::new(crate::support::FAKE).unwrap(),
    };
    drop(host.refused(command, ErrorCode::SandboxRemoving));
    assert_eq!(rig.setup.remove_calls.load(Ordering::SeqCst), 0);
    gate.release.send(()).unwrap();
    assert_eq!(wait("the retry", retry), Ok(CommandOutcome::Done));
    // The removal's own detection after `remove()`.
    gate.started.recv().unwrap();
    gate.release.send(()).unwrap();
    assert_eq!(wait("the removal", removal), Ok(CommandOutcome::Done));
    assert_eq!(rig.setup.remove_calls.load(Ordering::SeqCst), 1);
    assert_eq!(rig.detections.load(Ordering::SeqCst), 3);
    host.shutdown();
}

/// A second removal while one runs joins it: `remove()` once, both answered alike.
#[test]
fn a_second_removal_joins_the_first() {
    let dir = dir("protection_remove_join");
    let mut plan = Plan::new(
        vec![b(), c(NoSandbox::NeedsSetUp)],
        vec![SetupState::Ready, SetupState::Needed],
    );
    plan.gated = Gated::Remove;
    let (host, rig) = start(&dir, 0, plan);
    let gate = rig.gate.as_ref().unwrap();
    let first = send(&host.handle, HostCommand::RemoveSandboxSetup {});
    gate.started.recv().unwrap();
    assert_eq!(view(&host).in_progress, Some(ProtectionWork::Remove));
    let second = send(&host.handle, HostCommand::RemoveSandboxSetup {});
    // Sent after the second, so answered after it has joined.
    host.refused(HostCommand::RetryProtection {}, ErrorCode::CommandInvalid);
    gate.release.send(()).unwrap();
    assert_eq!(wait("the first removal", first), Ok(CommandOutcome::Done));
    assert_eq!(wait("the second removal", second), Ok(CommandOutcome::Done));
    assert_eq!(rig.setup.remove_calls.load(Ordering::SeqCst), 1);
    host.shutdown();
}
