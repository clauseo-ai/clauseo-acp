//! R17 PR-D3a: a sandbox seam without launchers sets the sandbox up but never drives launch
//! gating (the orchestrator's four tests). Launches go as with no seam (`not_built`): the
//! host's own launcher, `sandbox.required` refusing as for not-built and nothing else refusing,
//! while the view's `set_up_state` carries the sandbox's own state for the set-up screen. With
//! launchers, R17 PR-E's refusals apply and `set_up_state` is `None`.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use clauseo_protocol::{
    CommandOutcome, ErrorCode, HostCommand, HostEvent, NotProtectedCause, Protection,
    ProtectionTier, ProtectionWork, StopReason, StreamId, UnsupportedWhy,
};
use clauseo_sandbox::{Blocked, NoSandbox, SetupState};

use crate::codex::{Recording, turn_failed};
use crate::protection::{dir, start};
use crate::protection_rig::{
    Gated, Plan, b, c, protection_events, protection_events_to_end, rig, send, subscribe, view,
};
use crate::support::{Dir, FAKE, Scenario};

fn not(cause: NotProtectedCause) -> Protection {
    Protection::NotProtected { cause }
}

fn not_built() -> Protection {
    not(NotProtectedCause::Unsupported {
        why: UnsupportedWhy::NotBuilt,
    })
}

fn tier_b() -> Protection {
    Protection::Protected {
        tier: ProtectionTier::B,
    }
}

fn blocked() -> Protection {
    not(NotProtectedCause::Blocked {
        reason: "admin_copy_missing".to_owned(),
    })
}

/// Both protection settings as given.
fn settings(dir: &Dir, required: bool, auto: bool) {
    dir.set_settings(
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
        serde_json::json!({"sandbox": {"required": required, "allow_unprotected_auto": auto}}),
    );
}

fn create_thread(matter: clauseo_protocol::MatterId) -> HostCommand {
    HostCommand::CreateThread {
        matter,
        agent: crate::keys_loading::fake(),
    }
}

/// Test 1: tier B set up, no launchers: a turn's agent starts through the host's own launcher,
/// as today, and the view says not-built for launches beside the set-up state tier B; every
/// matter's row says not-built too. With `sandbox.required` on, the launch is refused with
/// `sandbox.required`, as for not-built.
#[test]
fn a_set_up_sandbox_without_launchers_launches_as_today_and_says_set_up() {
    let dir = dir("set_up_only_launch");
    let host = dir.start(0);
    let matter = host.create_matter("Smith", &dir.folder("smith", &[]));
    let thread = host.create_thread(matter, FAKE);
    host.shutdown();
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let sandbox = rig(Plan::set_up_only(vec![b()], vec![SetupState::Ready])).sandbox;
    let host = dir.start_launching(1, Arc::new(recording), move |config| {
        config.with_sandbox(sandbox)
    });
    let answer = view(&host);
    assert_eq!(answer.computer, not_built());
    assert_eq!(answer.tier, ProtectionTier::C);
    assert_eq!(answer.set_up_state, Some(tier_b()));
    assert!(!answer.retry_asks_admin);
    assert!(
        host.matters()
            .iter()
            .all(|row| row.protection == Some(not_built()))
    );
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert_eq!(launches.lock().unwrap().len(), 1, "the host's launcher");
    host.shutdown();
    // Set up, and the firm requires the sandbox: the turn's launch is refused as not-built's,
    // `sandbox.required`, and no agent starts.
    settings(&dir, true, false);
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let sandbox = rig(Plan::set_up_only(vec![b()], vec![SetupState::Ready])).sandbox;
    let host = dir.start_launching(2, Arc::new(recording), move |config| {
        config.with_sandbox(sandbox)
    });
    assert_eq!(view(&host).set_up_state, Some(tier_b()));
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter again.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::Failed);
    let failed = turn_failed(&events, turn).unwrap_or_else(|| panic!("{}", events.dump()));
    assert_eq!(failed.code, ErrorCode::SandboxRequired);
    assert!(launches.lock().unwrap().is_empty(), "an agent started");
    host.shutdown();
}

/// Test 2: a blocked sandbox without launchers refuses nothing with `sandbox.required` off
/// (auto modes off too), and with it on refuses `CreateThread` as not-built does:
/// `sandbox.required`, never `sandbox.blocked`. A removal running refuses nothing either.
#[test]
fn a_blocked_sandbox_without_launchers_refuses_only_as_not_built() {
    let dir = dir("set_up_only_blocked");
    let detection = c(NoSandbox::SetUpBlocked(Blocked::AdminCopyMissing));
    let state = SetupState::Blocked(Blocked::AdminCopyMissing);
    for (run, required) in [(0u16, false), (1, true)] {
        settings(&dir, required, false);
        let plan = Plan::set_up_only(vec![detection.clone()], vec![state.clone()]);
        let (host, _rig) = start(&dir, run, plan);
        let answer = view(&host);
        assert_eq!(answer.computer, not_built());
        assert_eq!(answer.set_up_state, Some(blocked()));
        assert!(!answer.retry_asks_admin, "a set-up can't lift a block");
        let matter = host.matters()[0].matter;
        if required {
            drop(host.refused(create_thread(matter), ErrorCode::SandboxRequired));
        } else {
            let _made = host.ok(create_thread(matter));
        }
        host.shutdown();
    }
    settings(&dir, false, false);
    let mut plan = Plan::set_up_only(vec![b()], vec![SetupState::Ready]);
    plan.gated = Gated::Remove;
    let (host, mut rig) = start(&dir, 2, plan);
    let gate = rig.gate.take().unwrap();
    let removal = send(&host.handle, HostCommand::RemoveSandboxSetup {});
    gate.started.recv().unwrap();
    assert_eq!(view(&host).in_progress, Some(ProtectionWork::Remove));
    let matter = host.matters()[0].matter;
    let _made = host.ok(create_thread(matter));
    gate.release.send(()).unwrap();
    assert_eq!(
        crate::support::wait("the removal", removal).unwrap(),
        CommandOutcome::Done
    );
    host.shutdown();
}

/// Test 3: the app's set-up action (`RetryProtection`) runs the fake set-up off the actor and
/// moves `set_up_state` from needs-set-up to tier B, answering `done`; the computer stays
/// not-built all along, so no matter's row changes.
#[test]
fn the_apps_set_up_action_moves_set_up_state_to_tier_b_off_the_actor() {
    let dir = dir("set_up_only_set_up");
    let plan = Plan::set_up_only(
        vec![c(NoSandbox::NeedsSetUp), b()],
        vec![SetupState::Needed, SetupState::Ready],
    );
    let (host, rig) = start(&dir, 0, plan);
    let mut sub = subscribe(&host);
    let before = view(&host);
    assert_eq!(before.computer, not_built());
    assert_eq!(
        before.set_up_state,
        Some(not(NotProtectedCause::NeedsSetUp))
    );
    assert!(before.retry_asks_admin);

    assert_eq!(
        host.ok(HostCommand::RetryProtection {}),
        CommandOutcome::Done
    );
    let views: Vec<_> = protection_events(&mut sub, 2)
        .into_iter()
        .map(|event| match event {
            HostEvent::ProtectionViewChanged { view } => view,
            other => panic!("not a view: {other:?}"),
        })
        .collect();
    assert_eq!(views[0].in_progress, Some(ProtectionWork::SetUp));
    assert_eq!(
        views[0].set_up_state,
        Some(not(NotProtectedCause::NeedsSetUp))
    );
    assert_eq!(views[1].in_progress, None);
    assert_eq!(views[1].set_up_state, Some(tier_b()));
    assert!(!views[1].retry_asks_admin);
    assert!(views.iter().all(|view| view.computer == not_built()));

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
    let rest = protection_events_to_end(&mut sub);
    assert!(
        rest.iter()
            .all(|event| !matches!(event, HostEvent::ProtectionChanged { .. })),
        "a matter's row changed: {rest:?}"
    );
}

/// Test 4: with launchers, R17 PR-E's refusal applies: a blocked sandbox refuses `CreateThread`
/// with `sandbox.blocked` though both settings would let an unprotected agent run, and the
/// view's computer is the block with no `set_up_state` beside it.
#[test]
fn with_launchers_a_blocked_sandbox_refuses_whatever_the_settings() {
    let dir = dir("set_up_only_launchers");
    settings(&dir, false, true);
    let plan = Plan::new(
        vec![c(NoSandbox::SetUpBlocked(Blocked::AdminCopyMissing))],
        vec![SetupState::Blocked(Blocked::AdminCopyMissing)],
    );
    let (host, _rig) = start(&dir, 0, plan);
    let answer = view(&host);
    assert_eq!(answer.computer, blocked());
    assert_eq!(answer.set_up_state, None);
    assert!(answer.unprotected_auto_allowed && !answer.required);
    let matter = host.matters()[0].matter;
    drop(host.refused(create_thread(matter), ErrorCode::SandboxBlocked));
    host.shutdown();
}
