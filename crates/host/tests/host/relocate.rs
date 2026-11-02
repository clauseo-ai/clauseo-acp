//! "Locate folder" (`RelocateMatter`), each matter's `folder_state`, and the relink of a deleted
//! `matter.json` when the matter opens.

use std::path::Path;

use clauseo_protocol::{
    CheckpointKind, CommandOutcome, DomainEvent, ErrorCode, FileChoice, FileDecision, FolderState,
    HostCommand, MatterId, MatterPath, StopReason, StreamId,
};

use crate::observe::Events;
use crate::permissions::{KINDS_ANSWERS, PERM, answer_from, until_first_request};
use crate::support::{Dir, FAKE, Running, Scenario, command_id};
use crate::write;

/// The agent id mapped to the `undo-note` scenario, whose first turn writes `notes/a.txt`.
const NOTE: &str = "note-agent";

/// `RelocateMatter` of `matter` to `folder`.
fn relocate(host: &Running, matter: MatterId, folder: &Path) -> HostCommand {
    HostCommand::RelocateMatter {
        matter,
        folder: host.folder_ref(folder),
    }
}

/// The `folder_state` `ListMatters` gives `matter`.
fn folder_state(host: &Running, matter: MatterId) -> FolderState {
    host.matters()
        .into_iter()
        .find(|summary| summary.matter == matter)
        .unwrap_or_else(|| panic!("matter {matter} not listed"))
        .folder_state
}

/// The matter's stream, read up to its `CaughtUp`.
fn matter_events(host: &Running, matter: MatterId) -> Events {
    let mut events = host.events(StreamId::Matter(matter));
    events.until_caught_up();
    events
}

/// The folders the matter's `MatterRelocated` events name, in order.
fn relocations(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::MatterRelocated(relocated) => Some(relocated.folder.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn moved_folder_is_located_and_the_next_turn_versions_it() {
    let dir = Dir::new(
        "moved_folder_is_located",
        &[
            (FAKE, Scenario::Bundled("prompt-stream")),
            (NOTE, Scenario::Local("undo-note")),
        ],
    );
    let host = dir.start(0);
    let old = dir.folder("a", &[("brief.txt", "Clause 4 draft\n")]);
    let matter = host.create_matter("a", &old);
    let chat = host.create_thread(matter, FAKE);
    let writer = host.create_thread(matter, NOTE);
    // A turn before the move, so the history holds one.
    let mut chat_events = host.events(StreamId::Thread(chat));
    let (first, _) = host.prompt(chat, "Summarise the brief.");
    assert_eq!(
        chat_events.until_turn_ended(first).stop,
        StopReason::EndTurn
    );
    assert_eq!(folder_state(&host, matter), FolderState::Found);
    // The chat's agent runs with the folder as its working directory, and Windows refuses to
    // move a folder a running process is in (sharing violation, os error 32), so the host
    // stops first and its agent exits.
    host.shutdown();

    let moved = dir.folder("moved", &[]).join("a");
    write::rename(&old, &moved);
    let host = dir.start(1);
    assert_eq!(folder_state(&host, matter), FolderState::Missing);

    assert_eq!(
        host.ok(relocate(&host, matter, &moved)),
        CommandOutcome::Done
    );
    assert_eq!(folder_state(&host, matter), FolderState::Found);
    let mut events = host.events(StreamId::Thread(writer));
    let (turn, _) = host.prompt(writer, "Write a note.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert!(ended.checkpoint_after.is_some(), "{}", events.dump());
    let note = MatterPath::new("notes/a.txt").unwrap();
    let changed = events.files_changed(turn).unwrap();
    assert!(
        changed.files.iter().any(|file| *file.path() == note),
        "{changed:?}"
    );
    assert!(moved.join("notes/a.txt").exists());
    assert!(!old.exists(), "the turn wrote into the old folder");

    // The history is intact in the new place: the step undoes there.
    let outcome = host.ok(HostCommand::UndoStep {
        thread: writer,
        turn,
        files: vec![FileDecision {
            path: note,
            choice: FileChoice::Undo,
        }],
    });
    assert!(
        matches!(outcome, CommandOutcome::StepUndone { .. }),
        "{outcome:?}"
    );
    assert!(!moved.join("notes/a.txt").exists());

    let logged = matter_events(&host, matter);
    let path = moved.to_string_lossy().into_owned();
    assert_eq!(
        relocations(&logged),
        std::slice::from_ref(&path),
        "{}",
        logged.dump()
    );
    assert!(logged.checkpoint(CheckpointKind::Import).is_some());
    assert!(logged.checkpoint(CheckpointKind::AfterTurn).is_some());
    host.shutdown();

    // The new path was stored: after a restart the folder is found and a turn runs there.
    let host = dir.start(2);
    assert_eq!(folder_state(&host, matter), FolderState::Found);
    assert_eq!(
        relocations(&matter_events(&host, matter)),
        [path],
        "the relocation is in the log once"
    );
    // A new thread's agent starts in the stored folder (`prompt-stream` has no session/load).
    let fresh = host.create_thread(matter, FAKE);
    let mut chat_events = host.events(StreamId::Thread(fresh));
    let (again, _) = host.prompt(fresh, "Summarise the brief again.");
    let ended = chat_events.until_turn_ended(again);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", chat_events.dump());
    assert_eq!(
        chat_events.count(|event| matches!(event, DomainEvent::TurnFailed(_))),
        0,
        "{}",
        chat_events.dump()
    );
    host.shutdown();
}

#[test]
fn locate_refusals_leave_the_matter_unchanged() {
    let dir = Dir::new("locate_refusals", &[]);
    let host = dir.start(0);
    let old = dir.folder("a", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("a", &old);
    let other_folder = dir.folder("outer/b", &[("lease.txt", "y\n")]);
    let other = host.create_matter("b", &other_folder);
    let moved = dir.work_path("a-moved");
    write::rename(&old, &moved);

    let marker = std::fs::read(other_folder.join(".clauseo/matter.json")).unwrap();
    let copy = dir.folder("copy", &[("brief.txt", "x\n")]);
    write::write(&copy.join(".clauseo/matter.json"), &marker);
    let plain = dir.folder("plain", &[("brief.txt", "x\n")]);
    let bad = dir.folder(
        "bad",
        &[("brief.txt", "x\n"), (".clauseo/matter.json", "{oops")],
    );
    let inside = dir.folder("outer/b/sub", &[]);
    let cases = [
        (
            "a marker naming another matter",
            copy,
            ErrorCode::MatterFolderMissing,
        ),
        ("no marker", plain, ErrorCode::MatterFolderMissing),
        ("an unreadable marker", bad, ErrorCode::MatterFolderMissing),
        (
            "no folder",
            dir.work_path("gone"),
            ErrorCode::MatterFolderMissing,
        ),
        (
            "another matter's folder",
            other_folder,
            ErrorCode::MatterFolderRefused,
        ),
        (
            "a folder holding another matter's",
            dir.work_path("outer"),
            ErrorCode::MatterFolderRefused,
        ),
        (
            "inside another matter's folder",
            inside,
            ErrorCode::MatterFolderRefused,
        ),
        (
            "the matters root",
            dir.data_dir().join("matters"),
            ErrorCode::MatterFolderRefused,
        ),
    ];
    let id = command_id();
    for (what, folder, code) in &cases {
        let refused = host
            .dispatch_as(id, relocate(&host, matter, folder))
            .expect_err(what);
        assert_eq!(refused.code(), *code, "{what}: {refused}");
        assert_eq!(folder_state(&host, matter), FolderState::Missing, "{what}");
    }
    assert_eq!(
        relocations(&matter_events(&host, matter)),
        Vec::<String>::new()
    );
    assert_eq!(folder_state(&host, other), FolderState::Found);

    // Nothing was kept, not even a receipt: the same command id now locates the matter.
    let located = host
        .dispatch_as(id, relocate(&host, matter, &moved))
        .unwrap();
    assert_eq!(located, CommandOutcome::Done);
    assert_eq!(folder_state(&host, matter), FolderState::Found);
    host.shutdown();
}

#[test]
fn relocate_while_a_turn_runs_is_refused() {
    let dir = Dir::new(
        "relocate_while_a_turn_runs",
        &[(PERM, Scenario::Bundled("permission-kinds"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "Clause 4 draft\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, PERM);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the four clauses.");
    until_first_request(&mut events);

    host.refused(
        relocate(&host, matter, &folder),
        ErrorCode::MatterTurnRunning,
    );
    assert_eq!(
        relocations(&matter_events(&host, matter)),
        Vec::<String>::new()
    );

    answer_from(&host, &mut events, thread, 0, &KINDS_ANSWERS);
    events.until_turn_ended(turn);
    host.settle();
    // With the writer free, locating the folder where it already is succeeds.
    assert_eq!(
        host.ok(relocate(&host, matter, &folder)),
        CommandOutcome::Done
    );
    host.shutdown();
}

#[test]
fn deleted_marker_is_relinked_on_open() {
    let dir = Dir::new(
        "deleted_marker_is_relinked",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, FAKE);
    host.shutdown();
    let marker = folder.join(".clauseo/matter.json");
    write::remove_file(&marker);

    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Summarise the brief.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert!(ended.checkpoint_after.is_some(), "{}", events.dump());
    let written = std::fs::read_to_string(&marker).unwrap();
    assert!(written.contains(&matter.to_string()), "{written}");
    host.shutdown();
}

#[test]
fn deleted_marker_another_matter_claims_is_not_relinked() {
    let dir = Dir::new(
        "deleted_marker_claimed",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let marker = folder.join(".clauseo/matter.json");
    // A first matter of the folder, forgotten: its folder still counts. With its marker gone the
    // folder imports again, as a second matter, the one this test prompts. (A listed matter's
    // folder can no longer be imported a second time: decision record 0022.)
    let first = host.create_matter("m", &folder);
    assert_eq!(
        host.ok(clauseo_protocol::HostCommand::ForgetMatter { matter: first }),
        clauseo_protocol::CommandOutcome::Done
    );
    write::remove_file(&marker);
    let matter = host.create_matter("m again", &folder);
    assert_ne!(first, matter);
    let thread = host.create_thread(matter, FAKE);
    host.shutdown();
    write::remove_file(&marker);

    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Summarise the brief.");
    let ended = events.until_turn_ended(turn);
    let failed = events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
        _ => None,
    });
    assert_eq!(
        failed,
        Some(ErrorCode::MatterFolderMissing),
        "{ended:?}: {}",
        events.dump()
    );
    assert!(
        !marker.exists(),
        "matter.json was written for a folder two matters record"
    );
    host.shutdown();
}
