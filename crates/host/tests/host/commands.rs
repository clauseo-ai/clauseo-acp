//! Commands: receipts, unknown ids, ids from another matter, a prompt's file outside its
//! matter, undo with its note.

use clauseo_protocol::{
    AgentId, CommandOutcome, DeliveryRequest, DomainEvent, ErrorCode, FileChoice, FileDecision,
    HostCommand, HostQuery, MatterId, MatterPath, PromptBlock, StopReason, StreamId, ThreadId,
};

use crate::support::{
    Dir, FAKE, Scenario, command_id, foreign_uuid, matter_with_thread, send_prompt, wait,
};

#[test]
fn command_id_repeat_and_conflict() {
    let dir = Dir::new(
        "command_id_repeat_and_conflict",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let id = command_id();
    let first = host
        .dispatch_as(id, send_prompt(thread, "Summarise the indemnity clause."))
        .unwrap();
    let CommandOutcome::PromptAccepted { turn, .. } = first else {
        panic!("SendPrompt answered {first:?}");
    };
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    let head = events.head();

    let again = host
        .dispatch_as(id, send_prompt(thread, "Summarise the indemnity clause."))
        .unwrap();
    assert_eq!(again, first, "a repeat returns the stored outcome");
    let error = host
        .dispatch_as(id, send_prompt(thread, "Summarise the cap instead."))
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandConflict, "{error}");

    host.settle();
    events.drain_ready();
    assert_eq!(events.head(), head, "a repeat logged: {}", events.dump());
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn unknown_ids_get_unknown_codes() {
    let dir = Dir::new(
        "unknown_ids_get_unknown_codes",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let thread = ThreadId::new(foreign_uuid(1)).unwrap();
    host.refused(
        send_prompt(thread, "Hello."),
        ErrorCode::CommandUnknownThread,
    );
    host.refused(
        HostCommand::CreateThread {
            matter: MatterId::new(foreign_uuid(2)).unwrap(),
            agent: AgentId::new(FAKE).unwrap(),
        },
        ErrorCode::CommandUnknownMatter,
    );
    host.shutdown();
}

#[test]
fn other_matter_ids_refused() {
    let dir = Dir::new(
        "other_matter_ids_refused",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (one, thread_one, _) =
        matter_with_thread(&dir, &host, "one", &[("brief.txt", "x\n")], FAKE);
    let (two, thread_two, _) =
        matter_with_thread(&dir, &host, "two", &[("brief.txt", "y\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread_one));
    let (turn_one, _) = host.prompt(thread_one, "Summarise the indemnity clause.");
    assert_eq!(events.until_turn_ended(turn_one).stop, StopReason::EndTurn);

    let import = |matter| {
        let mut events = host.events(StreamId::Matter(matter));
        events.until_caught_up();
        events
            .checkpoint(clauseo_protocol::CheckpointKind::Import)
            .unwrap_or_else(|| panic!("no import checkpoint: {}", events.dump()))
    };
    let (from_one, from_two) = (import(one), import(two));

    let error = host
        .query(HostQuery::CompareVersions {
            matter: two,
            from: from_one,
            to: from_two,
        })
        .unwrap_err();
    assert_eq!(
        error.code(),
        ErrorCode::CommandInvalid,
        "foreign checkpoint: {error}"
    );
    assert!(
        error.to_string().contains("not a version of this matter"),
        "foreign checkpoint: {error}"
    );
    // Its own checkpoints pass the check and reach the refusal that compare is not built.
    let own = host
        .query(HostQuery::CompareVersions {
            matter: two,
            from: from_two,
            to: from_two,
        })
        .unwrap_err();
    assert!(
        own.to_string().contains("not built yet"),
        "own checkpoints: {own}"
    );

    host.refused(
        HostCommand::UndoStep {
            thread: thread_two,
            turn: turn_one,
            files: vec![FileDecision {
                path: MatterPath::new("brief.txt").unwrap(),
                choice: FileChoice::Undo,
            }],
        },
        ErrorCode::CommandInvalid,
    );
    host.shutdown();
}

#[test]
fn prompt_file_outside_matter_refused() {
    let dir = Dir::new(
        "prompt_file_outside_matter_refused",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let elsewhere = dir.folder("elsewhere", &[("secret.txt", "s\n")]);
    let register =
        |path: std::path::PathBuf| wait("register_file", host.handle.register_file(path)).unwrap();
    let prompt = |file| HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::File { file }],
        delivery: DeliveryRequest::Queue,
    };
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let head = events.head();

    let id = command_id();
    let outside = register(elsewhere.join("secret.txt"));
    let error = host.dispatch_as(id, prompt(outside)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
    // Through `..` it is still outside.
    let dotted = register(folder.join("..").join("elsewhere").join("secret.txt"));
    let error = host.dispatch(prompt(dotted)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
    host.settle();
    events.drain_ready();
    assert_eq!(
        events.head(),
        head,
        "a refused prompt logged: {}",
        events.dump()
    );

    // No receipt was kept: the same id with a file in the matter is accepted, not a conflict.
    let inside = register(folder.join("brief.txt"));
    let accepted = host.dispatch_as(id, prompt(inside)).unwrap();
    let CommandOutcome::PromptAccepted { turn, .. } = accepted else {
        panic!("SendPrompt answered {accepted:?}");
    };
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.shutdown();
}

#[test]
fn undo_step_and_note_reach_agent() {
    let dir = Dir::new(
        "undo_step_and_note_reach_agent",
        &[(FAKE, Scenario::Local("undo-note"))],
    );
    let host = dir.start(0);
    let (_, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Write a note.");
    let ended = events.until_turn_ended(first);
    assert_eq!(ended.stop, StopReason::EndTurn);
    assert!(ended.checkpoint_after.is_some());
    assert!(folder.join("notes/a.txt").exists());

    let note = MatterPath::new("notes/a.txt").unwrap();
    let outcome = host.ok(HostCommand::UndoStep {
        thread,
        turn: first,
        files: vec![FileDecision {
            path: note.clone(),
            choice: FileChoice::Undo,
        }],
    });
    assert!(
        matches!(outcome, CommandOutcome::StepUndone { .. }),
        "{outcome:?}"
    );
    events.until("StepUndone", |e| {
        e.position(|event| matches!(event, DomainEvent::StepUndone(_)))
            .is_some()
    });
    let undone = events
        .events()
        .find_map(|event| match event {
            DomainEvent::StepUndone(undone) => Some(undone.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(undone.turn, first);
    assert_eq!(undone.restored, [note]);
    assert!(
        !folder.join("notes/a.txt").exists(),
        "the undo left notes/a.txt"
    );

    let (second, _) = host.prompt(thread, "Carry on.");
    let ended = events.until_turn_ended(second);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    host.shutdown();
}
