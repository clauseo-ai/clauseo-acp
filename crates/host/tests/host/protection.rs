//! This computer's protection through the host (decision record 0051, R17 PR-B1), with the
//! recording fakes of `protection_rig.rs`: the map from a detection to the query's answer, each
//! matter's row, `RetryProtection` and `RemoveSandboxSetup` with their held answers, the join,
//! the mutual exclusion and a shutdown mid-set-up, and `sandbox.required`'s refusals.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use clauseo_protocol::{
    CommandOutcome, ErrorCode, HostCommand, HostEvent, NotProtectedCause, Protection,
    ProtectionTier, ProtectionWork, StopReason, StreamId, UnsupportedWhy,
};
use clauseo_sandbox::{Detection, NoSandbox, SetupError, SetupState, Tier};

use crate::codex::{Recording, turn_failed};
use crate::protection_rig::{
    Gated, Plan, b, c, protection_events, protection_events_to_end, rig, send, subscribe, through,
    view,
};
use crate::support::{Dir, FAKE, Running, Scenario, wait};

fn not(cause: NotProtectedCause) -> Protection {
    Protection::NotProtected { cause }
}

pub(crate) fn dir(test: &str) -> Dir {
    Dir::new(test, &[(FAKE, Scenario::Bundled("prompt-stream"))])
}

/// A host of run `run` on `dir` with the seam of `plan`.
pub(crate) fn start(dir: &Dir, run: u16, plan: Plan) -> (Running, crate::protection_rig::Rig) {
    let rig = rig(plan);
    let sandbox = rig.sandbox.clone();
    let host = dir.start_with(run, move |config| config.with_sandbox(sandbox));
    (host, rig)
}

#[test]
fn each_detection_reaches_the_query_and_every_row() {
    let dir = dir("protection_map");
    let os = if cfg!(all(windows, target_arch = "aarch64")) {
        UnsupportedWhy::WindowsArm
    } else {
        UnsupportedWhy::Os
    };
    let rows = [
        (
            Detection {
                tier: Tier::A,
                reason: None,
            },
            Protection::Protected {
                tier: ProtectionTier::A,
            },
        ),
        (
            b(),
            Protection::Protected {
                tier: ProtectionTier::B,
            },
        ),
        (
            c(NoSandbox::UnsupportedOs),
            not(NotProtectedCause::Unsupported { why: os }),
        ),
        (
            c(NoSandbox::NotBuilt),
            not(NotProtectedCause::Unsupported {
                why: UnsupportedWhy::NotBuilt,
            }),
        ),
        (
            c(NoSandbox::ProbeFailed("/usr/bin/sandbox-exec".to_owned())),
            not(NotProtectedCause::SetUpFailed),
        ),
        (
            c(NoSandbox::SetUpFailed("C:\\Users\\lawyer".to_owned())),
            not(NotProtectedCause::SetUpFailed),
        ),
        (c(NoSandbox::NeedsSetUp), not(NotProtectedCause::NeedsSetUp)),
        (
            c(NoSandbox::SetUpRefused),
            not(NotProtectedCause::SetUpRefused),
        ),
    ];
    for (run, (detection, expected)) in (0u16..).zip(rows) {
        let (host, rig) = start(
            &dir,
            run,
            Plan::new(vec![detection], vec![SetupState::Ready]),
        );
        let answer = view(&host);
        assert_eq!(answer.computer, expected, "run {run}");
        let tier = match &expected {
            Protection::Protected { tier } => *tier,
            Protection::NotProtected { .. } => ProtectionTier::C,
        };
        assert_eq!(answer.tier, tier);
        assert_eq!(answer.in_progress, None);
        // A failure's text never reaches the wire (0047).
        let json = serde_json::to_string(&answer).unwrap();
        assert!(
            !json.contains("sandbox-exec") && !json.contains("lawyer"),
            "{json}"
        );
        let rows = host.matters();
        assert_ne!(rows.len(), 0);
        for row in rows {
            assert_eq!(row.protection.as_ref(), Some(&expected), "run {run}");
        }
        // Detected once, at start, on the starting thread: the query read the cache.
        assert_eq!(rig.detections.load(Ordering::SeqCst), 1);
        host.shutdown();
    }
}

#[test]
fn no_seam_is_not_built_and_both_commands_are_invalid() {
    let dir = dir("protection_no_seam");
    let host = dir.start(0);
    let not_built = not(NotProtectedCause::Unsupported {
        why: UnsupportedWhy::NotBuilt,
    });
    let answer = view(&host);
    assert_eq!(answer.computer, not_built);
    assert_eq!(answer.tier, ProtectionTier::C);
    assert!(!answer.retry_asks_admin);
    for row in host.matters() {
        assert_eq!(row.protection.as_ref(), Some(&not_built));
    }
    for command in [
        HostCommand::RetryProtection {},
        HostCommand::RemoveSandboxSetup {},
    ] {
        host.refused(command, ErrorCode::CommandInvalid);
    }
    host.shutdown();
}

#[test]
fn a_set_up_that_works_sends_its_progress_the_protected_view_and_each_matter_then_answers() {
    let dir = dir("protection_set_up");
    let plan = Plan::new(
        vec![c(NoSandbox::NeedsSetUp), b()],
        vec![SetupState::Needed, SetupState::Ready],
    );
    let (host, rig) = start(&dir, 0, plan);
    let matter = host.create_matter("Smith", &dir.folder("smith", &[]));
    let mut sub = subscribe(&host);
    assert!(view(&host).retry_asks_admin);

    assert_eq!(
        host.ok(HostCommand::RetryProtection {}),
        CommandOutcome::Done
    );
    // The set-up's start, the protected view, then each matter: all before the answer.
    let events = protection_events(&mut sub, 4);
    let views: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            HostEvent::ProtectionViewChanged { view } => Some(view.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(views.len(), 2, "{events:?}");
    assert_eq!(views[0].in_progress, Some(ProtectionWork::SetUp));
    assert_eq!(views[0].computer, not(NotProtectedCause::NeedsSetUp));
    let protected = Protection::Protected {
        tier: ProtectionTier::B,
    };
    assert_eq!(views[1].computer, protected);
    assert_eq!(views[1].in_progress, None);
    assert!(!views[1].retry_asks_admin);
    let mut changed: Vec<_> = events[2..]
        .iter()
        .map(|event| match event {
            HostEvent::ProtectionChanged { matter, protection } => {
                assert_eq!(protection, &protected);
                *matter
            }
            other => panic!("not a ProtectionChanged: {other:?}"),
        })
        .collect();
    changed.sort();
    let mut listed: Vec<_> = host.matters().iter().map(|row| row.matter).collect();
    listed.sort();
    assert_eq!(changed, listed);
    assert!(listed.contains(&matter));
    assert!(
        host.matters()
            .iter()
            .all(|row| row.protection == Some(protected.clone()))
    );

    assert_eq!(rig.setup.run_calls.load(Ordering::SeqCst), 1);
    let threads = rig.setup.threads.lock().unwrap().clone();
    assert!(
        threads.iter().all(|name| name != "clauseo-host"),
        "the actor called the set-up: {threads:?}"
    );
    assert!(
        threads.iter().any(|name| name == "clauseo-sandbox-setup"),
        "{threads:?}"
    );
    host.shutdown();
}

#[test]
fn a_refused_failed_or_ineffective_set_up_answers_its_code() {
    let dir = dir("protection_set_up_fails");
    let cases = [
        (
            Err(SetupError::Refused),
            c(NoSandbox::SetUpRefused),
            SetupState::Refused,
            ErrorCode::SandboxSetupRefused,
            NotProtectedCause::SetUpRefused,
        ),
        (
            Err(SetupError::Failed(
                "C:\\Users\\lawyer: access denied".to_owned(),
            )),
            c(NoSandbox::SetUpFailed("x".to_owned())),
            SetupState::Failed("x".to_owned()),
            ErrorCode::SandboxSetupFailed,
            NotProtectedCause::SetUpFailed,
        ),
        // It ran, and the sandbox still is not there.
        (
            Ok(()),
            c(NoSandbox::NeedsSetUp),
            SetupState::Needed,
            ErrorCode::SandboxSetupFailed,
            NotProtectedCause::NeedsSetUp,
        ),
    ];
    for (run, (outcome, after, state, code, cause)) in (0u16..).zip(cases) {
        let mut plan = Plan::new(
            vec![c(NoSandbox::NeedsSetUp), after],
            vec![SetupState::Needed, state],
        );
        plan.runs = vec![outcome];
        let (host, rig) = start(&dir, run, plan);
        let error = host.refused(HostCommand::RetryProtection {}, code);
        assert!(!error.message().contains("lawyer"), "{error}");
        let answer = view(&host);
        assert_eq!(answer.computer, not(cause), "run {run}");
        assert_eq!(answer.in_progress, None);
        assert!(answer.retry_asks_admin);
        assert_eq!(rig.setup.run_calls.load(Ordering::SeqCst), 1);
        host.shutdown();
    }
}

#[test]
fn a_retry_with_nothing_a_set_up_can_fix_only_checks_again() {
    let dir = dir("protection_check");
    let plan = Plan::new(
        vec![c(NoSandbox::SetUpFailed("another user".to_owned())), b()],
        vec![SetupState::SetUpByOtherUser],
    );
    let (host, rig) = start(&dir, 0, plan);
    assert!(!view(&host).retry_asks_admin);
    assert_eq!(
        host.ok(HostCommand::RetryProtection {}),
        CommandOutcome::Done
    );
    assert_eq!(rig.setup.run_calls.load(Ordering::SeqCst), 0);
    assert_eq!(rig.detections.load(Ordering::SeqCst), 2);
    assert_eq!(
        view(&host).computer,
        Protection::Protected {
            tier: ProtectionTier::B
        }
    );
    host.shutdown();
}

/// While `run()` waits (at its administrator prompt): the query answers at once with the work
/// in progress, a second retry joins (one run, the same answer), and a removal is refused.
#[test]
fn while_a_set_up_waits_the_host_answers_a_retry_joins_and_a_removal_is_refused() {
    let dir = dir("protection_join");
    let mut plan = Plan::new(
        vec![c(NoSandbox::NeedsSetUp), b()],
        vec![SetupState::Needed, SetupState::Ready],
    );
    plan.gated = Gated::Run;
    let (host, rig) = start(&dir, 0, plan);
    let gate = rig.gate.as_ref().unwrap();
    let first = send(&host.handle, HostCommand::RetryProtection {});
    gate.started.recv().unwrap();
    assert_eq!(view(&host).in_progress, Some(ProtectionWork::SetUp));
    let second = send(&host.handle, HostCommand::RetryProtection {});
    // Sent after the second, so answered after it has joined.
    let removal = host.refused(
        HostCommand::RemoveSandboxSetup {},
        ErrorCode::CommandInvalid,
    );
    assert!(removal.message().contains("running"), "{removal}");
    gate.release.send(()).unwrap();
    assert_eq!(wait("the first retry", first), Ok(CommandOutcome::Done));
    assert_eq!(wait("the second retry", second), Ok(CommandOutcome::Done));
    assert_eq!(rig.setup.run_calls.load(Ordering::SeqCst), 1);
    assert_eq!(rig.setup.remove_calls.load(Ordering::SeqCst), 0);
    assert_eq!(view(&host).in_progress, None);
    host.shutdown();
}

/// A removal answers `Done`, `sandbox.setup_refused` or `sandbox.setup_failed` by its outcome.
#[test]
fn a_removal_answers_by_its_outcome() {
    let dir = dir("protection_remove");
    let cases = [
        (Ok(()), None),
        (
            Err(SetupError::Refused),
            Some(ErrorCode::SandboxSetupRefused),
        ),
        (
            Err(SetupError::Failed("x".to_owned())),
            Some(ErrorCode::SandboxSetupFailed),
        ),
    ];
    for (run, (outcome, code)) in (0u16..).zip(cases) {
        let mut plan = Plan::new(
            vec![b(), c(NoSandbox::NeedsSetUp)],
            vec![SetupState::Ready, SetupState::Needed],
        );
        plan.removes = vec![outcome];
        let (host, rig) = start(&dir, run, plan);
        let mut sub = subscribe(&host);
        match code {
            None => assert_eq!(
                host.ok(HostCommand::RemoveSandboxSetup {}),
                CommandOutcome::Done
            ),
            Some(code) => drop(host.refused(HostCommand::RemoveSandboxSetup {}, code)),
        }
        // The removal's start, then the view after it.
        let events = protection_events(&mut sub, 2);
        let HostEvent::ProtectionViewChanged { view: during } = &events[0] else {
            panic!("{events:?}");
        };
        assert_eq!(during.in_progress, Some(ProtectionWork::Remove));
        let after = view(&host);
        assert_eq!(after.computer, not(NotProtectedCause::NeedsSetUp));
        assert!(after.retry_asks_admin);
        assert_eq!(rig.setup.remove_calls.load(Ordering::SeqCst), 1);
        host.shutdown();
    }
}

/// Shutdown does not wait for the set-up's thread: it ends at once, the held answer is
/// `Closed` (`internal` on the wire), and nothing more is sent.
#[test]
fn a_shutdown_while_a_set_up_waits_answers_closed_and_sends_nothing_after() {
    let dir = dir("protection_shutdown");
    let mut plan = Plan::new(
        vec![c(NoSandbox::NeedsSetUp), b()],
        vec![SetupState::Needed, SetupState::Ready],
    );
    plan.gated = Gated::Run;
    let (host, rig) = start(&dir, 0, plan);
    let mut sub = subscribe(&host);
    let held = send(&host.handle, HostCommand::RetryProtection {});
    rig.gate.as_ref().unwrap().started.recv().unwrap();
    host.shutdown();
    let answer = wait("the held retry", held).unwrap_err();
    assert_eq!(answer.code(), ErrorCode::Internal, "{answer}");
    let events = protection_events_to_end(&mut sub);
    let in_progress: Vec<_> = events
        .iter()
        .map(|event| match event {
            HostEvent::ProtectionViewChanged { view } => view.in_progress,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(in_progress, [Some(ProtectionWork::SetUp)]);
    // The thread ends after the host; its result goes nowhere.
    rig.gate.as_ref().unwrap().release.send(()).unwrap();
}

/// `sandbox.required` refuses `CreateThread` by the computer's cause; protected, or with the
/// setting off, nothing is refused.
#[test]
fn sandbox_required_refuses_create_thread_by_cause() {
    let dir = dir("protection_required");
    let required = serde_json::json!({"sandbox": {"required": true}});
    let off = serde_json::json!({});
    let cases = [
        (
            c(NoSandbox::NeedsSetUp),
            Some(ErrorCode::SandboxSetupRequired),
        ),
        (
            c(NoSandbox::SetUpRefused),
            Some(ErrorCode::SandboxSetupRequired),
        ),
        (
            c(NoSandbox::UnsupportedOs),
            Some(ErrorCode::SandboxRequired),
        ),
        (b(), None),
    ];
    let mut run = 0u16;
    for (detection, code) in cases {
        for (settings, expected) in [(&required, code), (&off, None)] {
            dir.set_settings(
                &[(FAKE, Scenario::Bundled("prompt-stream"))],
                settings.clone(),
            );
            let (host, _rig) = start(
                &dir,
                run,
                Plan::new(vec![detection.clone()], vec![SetupState::Ready]),
            );
            run += 1;
            let matter = host.matters()[0].matter;
            let command = HostCommand::CreateThread {
                matter,
                agent: clauseo_protocol::AgentId::new(FAKE).unwrap(),
            };
            match expected {
                Some(code) => drop(host.refused(command, code)),
                None => drop(host.ok(command)),
            }
            host.shutdown();
        }
    }
}

/// A queued turn's launch meets the same refusal: the turn fails with the code and no agent
/// starts.
#[test]
fn sandbox_required_fails_a_turn_before_its_agent_starts() {
    let dir = dir("protection_required_turn");
    let host = dir.start(0);
    let matter = host.create_matter("Smith", &dir.folder("smith", &[]));
    let thread = host.create_thread(matter, FAKE);
    host.shutdown();
    dir.set_settings(
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
        serde_json::json!({"sandbox": {"required": true}}),
    );
    for (run, detection, code) in [
        (1, c(NoSandbox::NeedsSetUp), ErrorCode::SandboxSetupRequired),
        (2, c(NoSandbox::UnsupportedOs), ErrorCode::SandboxRequired),
    ] {
        let recording = Arc::new(Recording::new(&dir));
        let launches = Arc::clone(&recording.envs);
        let sandbox = rig(Plan::new(vec![detection], vec![SetupState::Ready])).sandbox;
        let sandbox = through(sandbox, Arc::clone(&recording) as _);
        let host = dir.start_launching(run, recording, move |config| config.with_sandbox(sandbox));
        let mut events = host.events(StreamId::Thread(thread));
        let (turn, _) = host.prompt(thread, "Look at the matter.");
        assert_eq!(events.until_turn_ended(turn).stop, StopReason::Failed);
        let failed = turn_failed(&events, turn).unwrap_or_else(|| panic!("{}", events.dump()));
        assert_eq!(failed.code, code);
        assert!(launches.lock().unwrap().is_empty(), "an agent started");
        host.shutdown();
    }
}

/// The view carries both settings as they are at start.
#[test]
fn the_view_carries_both_settings() {
    let dir = dir("protection_settings");
    dir.set_settings(
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
        serde_json::json!({"sandbox": {"required": true, "allow_unprotected_auto": true}}),
    );
    let (host, _rig) = start(
        &dir,
        0,
        Plan::new(vec![c(NoSandbox::NeedsSetUp)], vec![SetupState::Needed]),
    );
    let answer = view(&host);
    assert!(answer.required && answer.unprotected_auto_allowed);
    host.shutdown();
}
