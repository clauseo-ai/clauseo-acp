//! R17 PR-E: while the sandbox's set-up is being removed, every launch and `CreateThread` is
//! refused with `sandbox.removing` (the cache still says tier B meanwhile), and a forgotten
//! matter is forgotten by the sandbox's set-up too, off the actor.

use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{
    AgentId, CommandOutcome, ErrorCode, HostCommand, ProtectionWork, StopReason, StreamId,
};
use clauseo_sandbox::{NoSandbox, SetupState};

use crate::codex::{Recording, turn_failed};
use crate::protection::dir;
use crate::protection_rig::{Gated, Plan, b, c, rig, send, through, view};
use crate::support::{FAKE, Scenario, wait};

/// A removal that waits at its gate refuses `CreateThread` and a queued turn's launch (no agent
/// starts); once it ends, `CreateThread` goes through again.
#[test]
fn a_removal_refuses_create_thread_and_a_turn_s_launch_until_it_ends() {
    let dir = dir("protection_removing");
    let host = dir.start(0);
    let matter = host.create_matter("Smith", &dir.folder("smith", &[]));
    let thread = host.create_thread(matter, FAKE);
    host.shutdown();
    dir.set_settings(
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
        serde_json::json!({"sandbox": {"required": false, "allow_unprotected_auto": true}}),
    );
    let mut plan = Plan::new(
        vec![b(), c(NoSandbox::NeedsSetUp)],
        vec![SetupState::Ready, SetupState::Needed],
    );
    plan.gated = Gated::Remove;
    let mut rig = rig(plan);
    let gate = rig.gate.take().unwrap();
    let recording = Arc::new(Recording::new(&dir));
    let launches = Arc::clone(&recording.envs);
    let sandbox = through(rig.sandbox.clone(), Arc::clone(&recording) as _);
    let host = dir.start_launching(1, recording, move |config| config.with_sandbox(sandbox));
    let removal = send(&host.handle, HostCommand::RemoveSandboxSetup {});
    gate.started.recv().unwrap();
    assert_eq!(view(&host).in_progress, Some(ProtectionWork::Remove));
    let create = || HostCommand::CreateThread {
        matter,
        agent: AgentId::new(FAKE).unwrap(),
    };
    drop(host.refused(create(), ErrorCode::SandboxRemoving));
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    let failed = turn_failed(&events, turn).unwrap_or_else(|| panic!("{}", events.dump()));
    assert_eq!(failed.code, ErrorCode::SandboxRemoving);
    assert!(launches.lock().unwrap().is_empty(), "an agent started");
    gate.release.send(()).unwrap();
    assert_eq!(wait("the removal", removal), Ok(CommandOutcome::Done));
    assert_eq!(view(&host).in_progress, None);
    let _thread = host.create_thread(matter, FAKE);
    host.shutdown();
}

/// `ForgetMatter` asks the set-up to forget the matter once its commit is durable, on the
/// sandbox's own thread, never the actor's.
#[test]
fn a_forgotten_matter_is_forgotten_by_the_sandbox_off_the_actor() {
    let dir = dir("protection_forget");
    let rig = rig(Plan::new(vec![b()], vec![SetupState::Ready]));
    let sandbox = rig.sandbox.clone();
    let host = dir.start_with(0, move |config| config.with_sandbox(sandbox));
    let matter = host.create_matter("Smith", &dir.folder("smith", &[]));
    assert_eq!(
        host.dispatch(HostCommand::ForgetMatter { matter }),
        Ok(CommandOutcome::Done)
    );
    let forgotten = wait("the sandbox's forget", async {
        loop {
            let forgotten = rig.setup.forgotten.lock().unwrap().clone();
            if !forgotten.is_empty() {
                break forgotten;
            }
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
    assert_eq!(
        forgotten,
        vec![(matter, "clauseo-sandbox-forget".to_owned())]
    );
    host.shutdown();
}
