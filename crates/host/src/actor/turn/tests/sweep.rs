//! A cancelled turn's sweep (`cancel::sweep_leftovers`) takes the prompt's listing while the host
//! serves, and leaves it while the host stops: the stop's own kill covers the agent's tree then.
//! A prompt's listing that comes after its wait is kept: the cancel still ends what the turn
//! started.

use clauseo_protocol::{StopReason, ThreadId};

use super::rig::Rig;
use crate::actor::HostPhase;
use crate::actor::turn::{BeforePrompt, cancel};

/// Gives `thread`'s current turn an (empty) prompt listing; whether it is still there after.
fn listing_left(rig: &mut Rig, thread: ThreadId, stop: StopReason) -> bool {
    let current = rig
        .actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .current
        .as_mut();
    current.unwrap().before_prompt = Some(BeforePrompt::default());
    cancel::sweep_leftovers(&mut rig.actor, thread, stop);
    let current = rig
        .actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .current
        .as_ref();
    current.unwrap().before_prompt.is_some()
}

#[test]
fn a_cancelled_turn_takes_its_listing_while_the_host_serves() {
    let mut rig = Rig::start("sweep-serving", "");
    let thread = rig.thread();
    rig.running_turn(thread);
    assert!(!listing_left(&mut rig, thread, StopReason::Cancelled));
    // A turn that ends any other way, not cancelled, sweeps nothing.
    assert!(listing_left(&mut rig, thread, StopReason::EndTurn));
    rig.finish();
}

#[test]
fn no_sweep_while_the_host_stops() {
    let mut rig = Rig::start("sweep-stopping", "");
    let thread = rig.thread();
    rig.running_turn(thread);
    let serving = std::mem::replace(&mut rig.actor.phase, HostPhase::Stopping(Vec::new()));
    let left = listing_left(&mut rig, thread, StopReason::Cancelled);
    rig.actor.phase = serving;
    assert!(left, "the sweep took the listing while the host stopped");
    rig.finish();
}

/// The prompt starts a command in a group of its own and waits; the cancel answers cancelled.
#[cfg(unix)]
const COMMAND_THEN_CANCEL: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"spawn_child":{"pid_file":"cmd.pid","own_group":true}}
{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}
"#;

/// The prompt's listing answers only after its wait (a loaded machine): the prompt goes out
/// without it, and the cancel's sweep waits for it and still ends the turn's command.
#[cfg(unix)]
#[test]
fn a_late_prompt_listing_still_lets_the_cancel_end_the_turns_command() {
    use std::time::Duration;

    use clauseo_protocol::HostCommand;

    use super::agents::send_prompt;
    use super::idle::{advance, rig, until_idle};
    use crate::agent::leftovers::{self, SNAPSHOT_WAIT};

    let mut rig = rig("sweep-late-listing", COMMAND_THEN_CANCEL, "exit");
    let thread = rig.thread();
    let held = leftovers::hold_next_listing();
    send_prompt(&mut rig, 10, thread);
    super::refused_requeue::poll_until(&mut rig, "the listing taken", |_| !held.taken.is_empty());
    advance(&mut rig, SNAPSHOT_WAIT + Duration::from_secs(1));
    let stored = std::path::PathBuf::from(&rig.actor.matters[&rig.matter()].row.folder_path);
    let pid_file = dunce::canonicalize(&stored).unwrap().join("cmd.pid");
    let mut pid = None;
    super::refused_requeue::poll_until(&mut rig, "the command started", |_| {
        pid = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok());
        pid.is_some()
    });
    let pid = pid.unwrap();
    let _reap = super::refused_requeue::Reap(pid);
    // A sweep spares what started in its cutoff's clock unit (10 ms ticks on Linux).
    super::refused_requeue::poll_until(&mut rig, "a cutoff to cover the command", |_| {
        clauseo_process::started_before_now(pid)
    });
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
    until_idle(&mut rig, thread);
    assert!(
        clauseo_process::running(pid),
        "the sweep waits for the listing"
    );
    drop(held);
    let gone = futures::executor::block_on(async {
        for _ in 0..500 {
            if !clauseo_process::running(pid) {
                return true;
            }
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
        false
    });
    assert!(gone, "the turn's command outlived the cancel");
    rig.finish();
}

/// A listing that misses its wait: kept for later on Unix (`Late`), where a cancel's sweep
/// waits for it; dropped on Windows (`Failed`), whose stop ends the whole job.
#[test]
fn a_late_listing_is_kept_on_unix_and_dropped_on_windows() {
    use std::sync::Arc;

    use super::agents::send_prompt;
    use super::idle::{TWO_ANSWERS, rig, until_idle};
    use crate::agent::leftovers::{self, Listed, SNAPSHOT_WAIT};
    use crate::clock::Clock;

    let mut rig = rig("sweep-late-listed", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    let descendants = rig.actor.threads[&thread]
        .turn
        .agent
        .as_deref()
        .unwrap()
        .link
        .descendants()
        .clone();
    let clock: Arc<dyn Clock> = Arc::new(rig.clock.clone());
    let held = leftovers::hold_next_listing();
    let manual = rig.clock.clone();
    let listed = futures::executor::block_on(async {
        let listing = leftovers::listing(descendants, clock, SNAPSHOT_WAIT);
        let late = async {
            held.taken.recv().await.unwrap();
            manual.advance(SNAPSHOT_WAIT);
        };
        futures::join!(listing, late).0
    });
    if cfg!(windows) {
        assert!(matches!(listed, Listed::Failed), "{listed:?}");
    } else {
        assert!(matches!(listed, Listed::Late(_)), "{listed:?}");
    }
    drop((held, listed));
    rig.finish();
}
