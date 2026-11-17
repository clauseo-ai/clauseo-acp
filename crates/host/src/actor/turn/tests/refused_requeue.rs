//! A turn refused before it started never reopens by itself (`own_turn/refused.rs`): with agent
//! output held between turns (a background command's update, a helper's permission card) and a
//! refusal that lasts (here the matter's folder renamed, `matter.folder_missing`; a failed
//! checkpoint, `matter.checkpoint_failed`, takes the same path), the first agent-started turn is
//! refused once, the held output dropped and its cards answered, the agent stopped, and no
//! other turn opens: exactly one `turn_failed`. And the belt: however they come to fail, no
//! more than `refused::CAP` agent-started turns fail in a row before the agent is stopped.

#[cfg(unix)]
use std::time::Duration;

use clauseo_acp::{AcpEvent, AgentUpdate, Origin, PendingKey};
use clauseo_protocol::{
    AgentSessionId, DomainEvent, ErrorCode, PermissionKind, PermissionOption, ThreadId,
};

use super::agents::send_prompt;
#[cfg(unix)]
use super::idle::advance;
use super::idle::{TWO_ANSWERS, live, rig, until_idle};
use super::rig::Rig;
#[cfg(unix)]
use crate::actor::Actor;
use crate::actor::turn::own_turn::{self, refused};
#[cfg(unix)]
use crate::agent::OwnerStep;
#[cfg(unix)]
use crate::agent::leftovers::{self, STOP_LIST_WAIT};

fn session() -> AgentSessionId {
    AgentSessionId::new("sess-1").unwrap()
}

/// A background command's update, as Claude sends one with no prompt in flight.
fn background_update() -> AcpEvent {
    AcpEvent::Update {
        session: session(),
        origin: Origin::Idle,
        update: AgentUpdate::Plan(Vec::new()),
    }
}

/// A helper's permission card, with no prompt in flight.
fn helper_card() -> AcpEvent {
    let option = |id: &str, kind| PermissionOption {
        option_id: id.into(),
        kind,
        agent_label: id.to_owned(),
    };
    AcpEvent::PermissionRequested {
        session: session(),
        origin: Origin::Idle,
        key: PendingKey(77),
        acp_tool_call_id: "call-patch".into(),
        options: vec![
            option("allow", PermissionKind::AllowOnce),
            option("reject", PermissionKind::RejectOnce),
        ],
        paths: Vec::new(),
        raw_input: None,
    }
}

/// The codes of `thread`'s failed turns.
fn failures(rig: &mut Rig, thread: ThreadId) -> Vec<ErrorCode> {
    let log = rig.log(thread);
    log.iter()
        .filter_map(|event| {
            let DomainEvent::TurnFailed(failed) = event else {
                return None;
            };
            Some(failed.code)
        })
        .collect()
}

/// What makes the next before-turn checkpoint refuse, given the matter's folder; what it returns
/// is held until the test ends.
type Refuse = fn(&std::path::Path) -> Option<std::fs::File>;

/// Renames the matter's folder: `matter.folder_missing`.
fn rename_folder(folder: &std::path::Path) -> Option<std::fs::File> {
    crate::write::rename(folder, &folder.with_file_name("Scratch (renamed)")).unwrap();
    None
}

/// A first turn ends, the folder is renamed, then `held` comes between turns.
fn refused_once_with(name: &str, held: fn() -> AcpEvent) {
    refused_once_by(name, held, rename_folder, ErrorCode::MatterFolderMissing);
}

/// A first turn ends, `refuse` runs, then `held` comes between turns, up to ten times over while
/// the agent is live: one refused agent-started turn failing with `code`, the agent stopped,
/// nothing held, and the log stops growing.
fn refused_once_by(name: &str, held: fn() -> AcpEvent, refuse: Refuse, code: ErrorCode) {
    let mut rig = rig(name, TWO_ANSWERS, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    let folder = std::path::PathBuf::from(&rig.actor.matters[&rig.matter()].row.folder_path);
    let _holding = refuse(&folder);
    // Only a live agent sends: once the refusal has stopped it, nothing more comes.
    for _ in 0..10 {
        if !live(&rig.actor, thread) {
            break;
        }
        own_turn::hold_for_test(&mut rig.actor, thread, held());
        own_turn::open_if_idle(&mut rig.actor, thread);
        rig.run_until("nothing in flight", |actor| {
            let state = &actor.threads[&thread].turn;
            state.current.is_none()
                && state.closing.is_none()
                && actor.work.commits == 0
                && actor.work.matter_jobs == 0
        });
    }
    assert_eq!(failures(&mut rig, thread), vec![code]);
    assert_eq!(own_turn::waiting(&mut rig.actor, thread), 0, "nothing held");
    let events = rig.log(thread).len();
    rig.run_until("the agent stopped", |actor| !live(actor, thread));
    assert_eq!(rig.log(thread).len(), events, "the log stopped growing");
    rig.finish();
}

#[test]
fn a_background_update_between_turns_with_the_folder_gone_fails_once() {
    refused_once_with("refused-once-update", background_update);
}

#[test]
fn a_helper_card_between_turns_with_the_folder_gone_fails_once_and_is_answered() {
    refused_once_with("refused-once-card", helper_card);
}

/// A new file in the matter that another program holds open with no sharing at all (share mode
/// 0): the before-turn checkpoint can't read it and fails, `matter.checkpoint_failed`.
#[cfg(windows)]
fn hold_a_file_with_no_sharing(folder: &std::path::Path) -> Option<std::fs::File> {
    let path = folder.join("Draft – Reply (v3).docx");
    crate::write::write_file(&path, b"draft").unwrap();
    let held = crate::write::open_no_share(&path).unwrap();
    Some(held)
}

#[cfg(windows)]
#[test]
fn a_file_held_with_no_sharing_between_turns_fails_once() {
    refused_once_by(
        "refused-once-no-share",
        background_update,
        hold_a_file_with_no_sharing,
        ErrorCode::MatterCheckpointFailed,
    );
}

/// The belt: agent-started turns refused in a row with the same code stop the agent at the
/// `CAP`th, not before; a turn of the lawyer's between ends the run.
#[test]
fn the_cap_on_refused_agent_started_turns_stops_the_agent() {
    let mut rig = rig("refused-cap", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    // An agent-started turn is current (its open is synchronous) while each refusal is counted.
    own_turn::hold_for_test(&mut rig.actor, thread, background_update());
    own_turn::open_if_idle(&mut rig.actor, thread);
    assert!(
        own_turn::is_own(&mut rig.actor, thread),
        "an agent-started turn opened"
    );
    let code = ErrorCode::MatterFileLocked;
    for n in 1..refused::CAP {
        refused::unstarted_failed(&mut rig.actor, thread, code);
        assert!(live(&rig.actor, thread), "still live after {n}");
    }
    refused::unstarted_failed(&mut rig.actor, thread, code);
    assert!(!live(&rig.actor, thread), "stopped at the cap");
    rig.run_until("the agent stopped", |actor| {
        actor.threads[&thread].turn.agent.is_none() && actor.work.commits == 0
    });
    rig.finish();
}

/// The first prompt starts a command in a session of its own (as Codex runs a helper's shell),
/// then ends the turn.
#[cfg(unix)]
const OWN_SESSION_COMMAND: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"spawn_child":{"pid_file":"cmd.pid","own_group":true}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
"#;

/// The stop a refused turn asks for reaches a command that left the agent's process group,
/// even when the agent ignores its stdin closing (`on_stdin_close: hang`): the host lists the
/// agent's descendants by parent before the ladder (`agent.rs`, `agent/leftovers.rs`), and the
/// tree kill ends the listed ones after the group.
/// Handles what the tasks post until `done` holds, looking every 5 ms for up to 10 s: for a
/// step that posts no input (a listing thread's answer, the owner task's step).
#[cfg(unix)]
pub(super) fn poll_until(rig: &mut Rig, what: &str, mut done: impl FnMut(&mut Actor) -> bool) {
    for _ in 0..2000 {
        rig.drain();
        if done(&mut rig.actor) {
            return;
        }
        futures::executor::block_on(async_io::Timer::after(Duration::from_millis(5)));
    }
    panic!("no {what} within 10 s");
}

/// Kills the test's own command on drop, so a failed assert leaves no process behind.
#[cfg(unix)]
pub(super) struct Reap(pub(super) u32);

#[cfg(unix)]
impl Drop for Reap {
    fn drop(&mut self) {
        if clauseo_process::running(self.0) {
            clauseo_process::kill(self.0);
        }
    }
}

#[cfg(unix)]
#[test]
fn a_hung_agents_command_in_a_session_of_its_own_dies_with_the_stop() {
    own_session_command_dies_with_the_stop("refused-hung", false);
}

/// The stop's listing answers only after its wait (a loaded machine): the ladder runs without
/// it, and the command is ended once the listing comes.
#[cfg(unix)]
#[test]
fn a_late_listing_still_ends_the_command_after_the_stop() {
    own_session_command_dies_with_the_stop("refused-late-listing", true);
}

/// The first prompt leaves a command in a session of its own; the folder goes, a background
/// update opens a turn, its refusal stops the agent (which ignores its stdin closing), and the
/// command dies with the stop. `late`: the listing answers only after [`STOP_LIST_WAIT`].
#[cfg(unix)]
fn own_session_command_dies_with_the_stop(name: &str, late: bool) {
    let mut rig = rig(name, OWN_SESSION_COMMAND, "hang");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    let stored = std::path::PathBuf::from(&rig.actor.matters[&rig.matter()].row.folder_path);
    let folder = dunce::canonicalize(&stored).unwrap();
    let pid: u32 = std::fs::read_to_string(folder.join("cmd.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let _reap = Reap(pid);
    assert!(clauseo_process::running(pid), "the command runs");
    crate::write::rename(&folder, &folder.with_file_name("Scratch (renamed)")).unwrap();
    let held = late.then(leftovers::hold_next_listing);
    own_turn::hold_for_test(&mut rig.actor, thread, background_update());
    own_turn::open_if_idle(&mut rig.actor, thread);
    rig.run_until("the stop asked", |actor| !live(actor, thread));
    let step = |actor: &mut Actor| {
        actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .map(|agent| agent.link.owner_step())
    };
    if let Some(held) = &held {
        poll_until(&mut rig, "the listing taken", |_| !held.taken.is_empty());
        advance(&mut rig, STOP_LIST_WAIT + Duration::from_secs(1));
    }
    // The listing waits for its thread on the host's clock: the clock moves on only once the
    // listing is taken (or, `late`, once its wait has run out).
    poll_until(&mut rig, "the descendants listed", |actor| {
        !matches!(step(actor), Some(OwnerStep::Running | OwnerStep::Listing))
    });
    // The ladder's waits are on the host's clock: moved on until the agent has ended.
    for _ in 0..20 {
        if rig.actor.threads[&thread].turn.agent.is_none() {
            break;
        }
        advance(&mut rig, Duration::from_secs(3));
    }
    assert!(
        rig.actor.threads[&thread].turn.agent.is_none(),
        "the agent ended"
    );
    assert!(
        !late || clauseo_process::running(pid),
        "the command waits for the late listing"
    );
    drop(held);
    let gone = futures::executor::block_on(async {
        for _ in 0..500 {
            if !clauseo_process::running(pid) {
                return true;
            }
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
        !clauseo_process::running(pid)
    });
    assert!(
        gone,
        "the command in a session of its own outlived the stop"
    );
    rig.finish();
}
