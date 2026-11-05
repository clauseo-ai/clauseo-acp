//! Shutdown waits for a session's run folder removal: the host answers only once the run folder
//! (and the browser claims riding on it) is gone, never while the removal thread still runs.

use super::agents::send_prompt;
use super::rig::Rig;
use crate::actor::HostPhase;
use crate::actor::input::Input;
use crate::actor::shutdown;
use crate::actor::turn::TurnInput;

/// The agent answers the prompt and stays until its stdin closes.
const ANSWER: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
"#;

fn run_removed(_: &crate::actor::Actor, input: &Input) -> bool {
    matches!(input, Input::Turn(TurnInput::RunRemoved))
}

/// Whether shutdown's work other than run folder removals is done.
fn all_else_done(actor: &mut crate::actor::Actor) -> bool {
    let work = &actor.work;
    work.commits == 0
        && work.loads == 0
        && work.receipts == 0
        && work.matter_jobs == 0
        && work.snapshots == 0
        && !crate::actor::turn::busy(actor)
        && !crate::actor::browser_install::busy(actor)
}

#[test]
fn shutdown_waits_for_the_run_folder_removal() {
    let mut rig = Rig::start("turn-run-removal", ANSWER);
    let thread = rig.thread();
    let _turn = send_prompt(&mut rig, 10, thread);
    rig.run_until("the turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.agent.is_some() && actor.work.commits == 0
    });
    let root = rig.actor.threads[&thread]
        .turn
        .run
        .as_ref()
        .map(|run| run.root.clone())
        .expect("the live session has its run folder");
    assert!(
        root.is_dir(),
        "the run folder is there while the agent runs"
    );

    // Shutdown stops the agent; its run folder's removal answers, and that answer is held
    // while the rest of shutdown's work ends.
    shutdown::begin_shutdown(&mut rig.actor, None);
    rig.hold_next("the run folder's removal", run_removed);
    rig.run_holding("the rest of shutdown's work", run_removed, all_else_done);
    assert!(!root.exists(), "the removal ran before it answered");
    assert_eq!(rig.actor.work.run_removals, 1);
    // `settle` ran on every input above; had it let the actor exit, the phase would be back
    // to serving.
    assert!(
        matches!(rig.actor.phase, HostPhase::Stopping(_)) && rig.actor.settle().is_none(),
        "shutdown answered with a run folder removal in flight"
    );

    rig.release_held();
    assert_eq!(rig.actor.work.run_removals, 0);
    assert!(
        rig.actor.settle().is_some(),
        "shutdown ends once the removal answered"
    );
    rig.finish();
}
