//! A turn sent while a `RelocateMatter` holds the matter's writer: it queues behind the
//! relocate, a second relocate, an undo and a forget are refused, and once the relocate is
//! durable the turn's agent, started in the old folder, starts again in the new one before the
//! turn runs.

use std::path::PathBuf;

use clauseo_protocol::{
    CommandEnvelope, CommandOutcome, DetachReason, DomainEvent, ErrorCode, FileChoice,
    FileDecision, FolderRefId, HostCommand, MatterPath, MatterSource, PromptBlock, StopReason,
    ThreadId, Title, TurnId,
};
use futures::channel::oneshot;

use super::rig::{Rig, command_id, kind};
use crate::actor::input::{Input, MatterTag, Request};
use crate::actor::turn::{Holder, TurnPhase};

/// Every prompt ends at once; a new agent plays it from the start.
const ANSWER: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
"#;

fn send_prompt(rig: &mut Rig, n: u128, thread: ThreadId) -> TurnId {
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Read the brief.".to_owned(),
        }],
        delivery: clauseo_protocol::DeliveryRequest::Queue,
    };
    let accepted = rig.dispatch(n, command).unwrap();
    let CommandOutcome::PromptAccepted { turn, .. } = accepted else {
        panic!("not accepted: {accepted:?}");
    };
    turn
}

/// Handles inputs until `thread` has no turn and nothing is in flight.
fn until_idle(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the turn's end", |actor| {
        actor.threads[&thread].turn.current.is_none()
            && actor.work.commits == 0
            && actor.work.matter_jobs == 0
    });
}

#[test]
fn a_turn_sent_during_a_relocate_waits_and_runs_in_the_new_folder() {
    let mut rig = Rig::start("relocate-turn", ANSWER);
    let thread = rig.thread();
    let matter = rig.matter();
    // A first turn: the agent is attached, started in the old folder.
    let first = send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    assert!(rig.log(thread).iter().any(|event| matches!(event,
        DomainEvent::TurnEnded(ended) if ended.turn == first)));

    // A copy of the folder, marker included, is where the matter is located.
    let old = PathBuf::from(&rig.actor.matters[&matter].row.folder_path);
    let moved = old.with_file_name("Scratch moved");
    crate::write::create_dir_all(&moved.join(".clauseo")).unwrap();
    let marker = std::fs::read(old.join(".clauseo/matter.json")).unwrap();
    crate::write::write_file(&moved.join(".clauseo/matter.json"), &marker).unwrap();
    let now = rig.actor.now().wall;
    let folder: FolderRefId = rig.actor.ids.mint(now).unwrap();
    rig.actor.folders.insert(folder, moved.clone());

    let (reply, mut answer) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(20),
        command: HostCommand::RelocateMatter { matter, folder },
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.hold_next("the locate's answer", |_, input| {
        matches!(
            input,
            Input::Matter {
                tag: MatterTag::Relocate { .. },
                ..
            }
        )
    });
    assert_eq!(
        rig.actor.matters[&matter].turns.holder,
        Some(Holder::Relocate)
    );

    // `ForgetMatter` waits for the relocate too. The rig's matter is the Scratch matter, which
    // forget refuses before anything else, so it is a folder matter for this one command.
    let set_kind = |rig: &mut Rig, kind| {
        if let Some(entry) = rig.actor.matters.get_mut(&matter) {
            entry.row.kind = kind;
        }
    };
    set_kind(&mut rig, clauseo_protocol::MatterKind::Folder);
    let forget = rig.dispatch(19, HostCommand::ForgetMatter { matter });
    set_kind(&mut rig, clauseo_protocol::MatterKind::Scratch);
    assert_eq!(forget.unwrap_err().code(), ErrorCode::MatterTurnRunning);
    assert!(rig.actor.matters.contains_key(&matter), "still listed");

    // Meanwhile: a turn queues behind the relocate; another relocate and an undo are refused.
    let second = send_prompt(&mut rig, 21, thread);
    rig.run_until("the turn queued", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.phase == TurnPhase::Queued)
            && actor.work.commits == 0
    });
    let again = rig.dispatch(22, HostCommand::RelocateMatter { matter, folder });
    assert_eq!(again.unwrap_err().code(), ErrorCode::MatterTurnRunning);
    let undo = HostCommand::UndoStep {
        thread,
        turn: first,
        files: vec![FileDecision {
            path: MatterPath::new("brief.txt").unwrap(),
            choice: FileChoice::Undo,
        }],
    };
    assert!(
        rig.dispatch(23, undo).is_err(),
        "an undo ran during the relocate"
    );
    let log = rig.log(thread);
    assert!(
        log.iter().any(|event| matches!(event,
            DomainEvent::TurnQueued(queued) if queued.turn() == second)),
        "{:?}",
        log.iter().map(kind).collect::<Vec<_>>()
    );
    assert!(!log.iter().any(|event| matches!(event,
        DomainEvent::TurnStarted(started) if started.turn == second)));

    rig.release_held();
    until_idle(&mut rig, thread);
    let Ok(Some(Ok(outcome))) = answer.try_recv() else {
        panic!("the relocate was not answered");
    };
    assert_eq!(outcome, CommandOutcome::Done);
    assert_eq!(
        rig.actor.matters[&matter].row.folder_path,
        moved.to_string_lossy()
    );
    let log = rig.log(thread);
    let kinds = log.iter().map(kind).collect::<Vec<_>>();
    let restarted = log.iter().position(|event| {
        matches!(event,
        DomainEvent::AgentSessionDetached(detached) if detached.reason == DetachReason::Restarting)
    });
    let started = log.iter().position(|event| {
        matches!(event,
        DomainEvent::TurnStarted(started) if started.turn == second)
    });
    assert!(
        matches!((restarted, started), (Some(r), Some(s)) if r < s),
        "the agent was not started again before the turn: {kinds:?}"
    );
    let ended = log.iter().find_map(|event| {
        if let DomainEvent::TurnEnded(ended) = event
            && ended.turn == second
        {
            return Some(ended.stop);
        }
        None
    });
    assert_eq!(ended, Some(StopReason::EndTurn), "{kinds:?}");
    assert!(
        rig.matter_log()
            .iter()
            .any(|event| matches!(event, DomainEvent::MatterRelocated(_)))
    );
    rig.finish();
}

#[test]
fn a_folder_a_relocate_is_moving_a_matter_to_counts_against_new_matters() {
    let mut rig = Rig::start("relocate-target-claimed", ANSWER);
    let matter = rig.matter();
    let old = PathBuf::from(&rig.actor.matters[&matter].row.folder_path);
    let moved = old.with_file_name("Scratch target");
    crate::write::create_dir_all(&moved.join(".clauseo")).unwrap();
    let marker = std::fs::read(old.join(".clauseo/matter.json")).unwrap();
    crate::write::write_file(&moved.join(".clauseo/matter.json"), &marker).unwrap();
    let now = rig.actor.now().wall;
    let folder: FolderRefId = rig.actor.ids.mint(now).unwrap();
    rig.actor.folders.insert(folder, moved.clone());
    let target: FolderRefId = rig.actor.ids.mint(now).unwrap();
    rig.actor.folders.insert(target, moved.clone());

    // The relocate to `moved` is held in flight: `moved` is the folder a matter is moving to.
    let (reply, mut answer) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(20),
        command: HostCommand::RelocateMatter { matter, folder },
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.hold_next("the locate's answer", |_, input| {
        matches!(
            input,
            Input::Matter {
                tag: MatterTag::Relocate { .. },
                ..
            }
        )
    });

    // Importing that folder is refused, as is a new folder inside it.
    let import = rig.dispatch(
        21,
        HostCommand::CreateMatter {
            name: Title::new("Smith v Jones").unwrap(),
            source: MatterSource::ExistingFolder { folder: target },
            client: None,
            next_date: None,
        },
    );
    assert_eq!(
        import.unwrap_err().code(),
        ErrorCode::MatterFolderIsMatter,
        "the import of the relocate's target"
    );
    let inside = rig.dispatch(
        22,
        HostCommand::CreateMatter {
            name: Title::new("Appeal").unwrap(),
            source: MatterSource::NewFolder { folder: target },
            client: None,
            next_date: None,
        },
    );
    assert_eq!(
        inside.unwrap_err().code(),
        ErrorCode::MatterFolderIsMatter,
        "a new folder inside the relocate's target"
    );
    assert!(!moved.join("Appeal").exists(), "nothing was made");

    rig.release_held();
    let mut got = None;
    rig.run_until("the relocate's answer", |_| {
        if let Ok(Some(result)) = answer.try_recv() {
            got = Some(result);
        }
        got.is_some()
    });
    assert_eq!(got.unwrap().unwrap(), CommandOutcome::Done);
    assert_eq!(
        rig.actor.matters[&matter].row.folder_path,
        moved.to_string_lossy()
    );
    rig.finish();
}

#[test]
fn a_failed_relocate_commit_puts_the_matter_back_on_its_folder() {
    let mut rig = Rig::start("relocate-commit-fails", ANSWER);
    let thread = rig.thread();
    let matter = rig.matter();
    let old = PathBuf::from(&rig.actor.matters[&matter].row.folder_path);
    let moved = old.with_file_name("Scratch copy");
    crate::write::create_dir_all(&moved.join(".clauseo")).unwrap();
    let marker = std::fs::read(old.join(".clauseo/matter.json")).unwrap();
    crate::write::write_file(&moved.join(".clauseo/matter.json"), &marker).unwrap();
    let now = rig.actor.now().wall;
    let folder: FolderRefId = rig.actor.ids.mint(now).unwrap();
    rig.actor.folders.insert(folder, moved);

    rig.fail(1, |event| matches!(event, DomainEvent::MatterRelocated(_)));
    let refused = rig.dispatch(20, HostCommand::RelocateMatter { matter, folder });
    assert!(
        refused.is_err(),
        "the relocate's commit was refused: {refused:?}"
    );
    let entry = &rig.actor.matters[&matter];
    assert_eq!(PathBuf::from(&entry.row.folder_path), old);
    assert_eq!(entry.turns.holder, None, "the writer was not passed on");
    assert!(
        rig.actor.claims.is_empty(),
        "the claim outlived the relocate"
    );
    assert!(
        !rig.matter_log()
            .iter()
            .any(|event| matches!(event, DomainEvent::MatterRelocated(_))),
        "a refused commit logged the relocation"
    );

    // The worker is back on the recorded folder: a file written there is the next turn's
    // outside edit, and the turn runs.
    rig.write_file("late.txt", "written after the failed relocate\n");
    let turn = send_prompt(&mut rig, 21, thread);
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    let kinds = log.iter().map(kind).collect::<Vec<_>>();
    let ended = log.iter().find_map(|event| {
        if let DomainEvent::TurnEnded(ended) = event
            && ended.turn == turn
        {
            return Some(ended.stop);
        }
        None
    });
    assert_eq!(ended, Some(StopReason::EndTurn), "{kinds:?}");
    let edited = rig.matter_log().iter().any(|event| {
        matches!(event, DomainEvent::OutsideEdits(edits)
            if edits.files.iter().any(|file| file.path().as_str() == "late.txt"))
    });
    assert!(edited, "the turn did not version the recorded folder");
    rig.finish();
}
