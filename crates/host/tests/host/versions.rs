//! Versions: `RestoreVersion` (files written, the restore's events, the note on the next
//! prompt, a restore with nothing to write, the refusals), `CompareVersions` (two versions, and
//! a version with the folder now), and the `CheckpointRow` the store keeps for every checkpoint.

use clauseo_protocol::{
    ChangeKind, CheckpointId, CheckpointKind, CommandOutcome, DomainEvent, ErrorCode, FileChange,
    HostCommand, HostQuery, MatterId, MatterPath, QueryResult, RestoreRequest, RestoreScope,
    StopReason, StreamId,
};
use clauseo_store::{Store, StoreOptions};

use crate::observe::Events;
use crate::permissions::{PERM, until_first_request};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};
use crate::write;

/// The matter's events, read until caught up.
pub(crate) fn matter_events(host: &Running, matter: MatterId) -> Events {
    let mut events = host.events(StreamId::Matter(matter));
    events.until_caught_up();
    events
}

/// Every `CheckpointRecorded` of the matter's log, with the seq it was logged at.
pub(crate) fn recorded(events: &Events) -> Vec<(CheckpointId, CheckpointKind, u64)> {
    events
        .seen
        .iter()
        .filter_map(|envelope| match &envelope.event {
            DomainEvent::CheckpointRecorded(recorded) => {
                Some((recorded.checkpoint(), recorded.kind(), envelope.seq.get()))
            }
            _ => None,
        })
        .collect()
}

fn restore_brief(matter: MatterId, checkpoint: CheckpointId) -> HostCommand {
    HostCommand::RestoreVersion {
        matter,
        checkpoint,
        scope: RestoreRequest::Files {
            paths: vec![MatterPath::new("brief.txt").unwrap()],
        },
    }
}

#[test]
fn restore_writes_files_notes_next_prompt_and_rows_survive() {
    let dir = Dir::new(
        "restore_writes_files_notes_next_prompt_and_rows_survive",
        &[(FAKE, Scenario::Local("restore-note"))],
    );
    let host = dir.start(0);
    let (matter, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Rewrite the brief.");
    let ended = events.until_turn_ended(first);
    assert_eq!(ended.stop, StopReason::EndTurn);
    let after = ended.checkpoint_after.expect("turn 1 changed brief.txt");
    let read = || std::fs::read_to_string(folder.join("brief.txt")).unwrap();
    assert_eq!(read(), "Clause 4 rewritten\n");

    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();

    // The two checkpoints compare as the turn's change.
    let compared = host
        .query(HostQuery::CompareVersions {
            matter,
            from: import,
            to: Some(after),
        })
        .unwrap();
    let QueryResult::FileChanges(changes) = compared else {
        panic!("CompareVersions answered {compared:?}");
    };
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].path().as_str(), "brief.txt");
    assert_eq!(changes[0].change(), &ChangeKind::Modified);

    // The preview lists the one file the restore will write, and writes nothing.
    write::write(&folder.join("added.txt"), b"new since\n");
    let previewed = host
        .query(HostQuery::PreviewRestore {
            matter,
            checkpoint: import,
            scope: RestoreRequest::Matter,
        })
        .unwrap();
    let QueryResult::RestorePreview(preview) = previewed else {
        panic!("PreviewRestore answered {previewed:?}");
    };
    assert_eq!(preview.changes.len(), 1, "{preview:?}");
    assert_eq!(preview.changes[0].path().as_str(), "brief.txt");
    assert_eq!(preview.added_since, [MatterPath::new("added.txt").unwrap()]);
    assert!(
        preview.kept.is_empty() && preview.locked.is_empty(),
        "{preview:?}"
    );
    assert_eq!(read(), "Clause 4 rewritten\n", "the preview wrote nothing");

    let outcome = host.ok(restore_brief(matter, import));
    let CommandOutcome::VersionRestored { checkpoint } = outcome else {
        panic!("RestoreVersion answered {outcome:?}");
    };
    assert_eq!(read(), "Clause 4 draft\n", "the restore put brief.txt back");
    let log = matter_events(&host, matter);
    let restored = log
        .events()
        .find_map(|event| match event {
            DomainEvent::VersionRestored(restored) => Some(restored.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no VersionRestored: {}", log.dump()));
    assert_eq!(restored.from, import);
    assert_eq!(restored.checkpoint, checkpoint);
    assert_eq!(
        restored.scope,
        RestoreScope::Files {
            paths: vec![MatterPath::new("brief.txt").unwrap()]
        }
    );
    assert_eq!(log.checkpoint(CheckpointKind::Restore), Some(checkpoint));

    // An edit outside Clauseo, then the next prompt: its before-turn checkpoint is new, and
    // the prompt opens with the note naming brief.txt (the scenario expects it).
    write::write(&folder.join("extra.txt"), b"typed by the lawyer\n");
    let (second, _) = host.prompt(thread, "Carry on.");
    let ended = events.until_turn_ended(second);
    host.settle();
    events.drain_ready();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let logged = recorded(&matter_events(&host, matter));
    let kinds: Vec<_> = logged.iter().map(|(_, kind, _)| *kind).collect();
    assert_eq!(
        kinds,
        [
            CheckpointKind::Import,
            CheckpointKind::AfterTurn,
            CheckpointKind::OutsideEdits,
            CheckpointKind::Restore,
            CheckpointKind::BeforeTurn,
        ]
    );
    host.shutdown();
    assert_every_checkpoint_has_its_row(&dir, matter, &logged);
}

/// Reads the store after shutdown: every `CheckpointRecorded` in `logged` has its row, with
/// the matter, the `seq` of its event and a commit of its own.
pub(crate) fn assert_every_checkpoint_has_its_row(
    dir: &Dir,
    matter: MatterId,
    logged: &[(CheckpointId, CheckpointKind, u64)],
) {
    let store = wait("Store::open", Store::open(StoreOptions::new(dir.data()))).unwrap();
    let mut commits = Vec::new();
    for &(id, kind, seq) in logged {
        let row = wait("checkpoint", store.checkpoint(id))
            .unwrap()
            .unwrap_or_else(|| panic!("no row for the {kind:?} checkpoint"));
        assert_eq!(
            (row.id, row.matter, row.seq.get()),
            (id, matter, seq),
            "{kind:?}"
        );
        assert_eq!(row.commit_oid.len(), 40, "{kind:?}: {}", row.commit_oid);
        assert!(row.commit_oid.bytes().all(|b| b.is_ascii_hexdigit()));
        commits.push(row.commit_oid);
    }
    commits.sort();
    commits.dedup();
    assert_eq!(
        commits.len(),
        logged.len(),
        "each checkpoint has its own commit"
    );
    wait("Store::close", store.close()).unwrap();
}

#[test]
fn restore_of_the_current_version_writes_nothing() {
    let dir = Dir::new(
        "restore_of_the_current_version_writes_nothing",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (matter, _, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let before = matter_events(&host, matter);
    let import = before.checkpoint(CheckpointKind::Import).unwrap();

    // The folder already is that version: the outcome names it, and nothing is logged.
    let outcome = host.ok(restore_brief(matter, import));
    assert_eq!(
        outcome,
        CommandOutcome::VersionRestored { checkpoint: import }
    );
    host.settle();
    let after = matter_events(&host, matter);
    assert_eq!(after.head(), before.head(), "logged: {}", after.dump());
    host.shutdown();
}

#[test]
fn restore_refused_while_turn_runs() {
    let dir = Dir::new(
        "restore_refused_while_turn_runs",
        &[(PERM, Scenario::Bundled("permission-kinds"))],
    );
    let host = dir.start(0);
    let (matter, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], PERM);
    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();

    // A turn waiting on a permission holds the matter's writer.
    let mut events = host.events(StreamId::Thread(thread));
    host.prompt(thread, "Go.");
    until_first_request(&mut events);
    let head = matter_events(&host, matter).head();
    let error = host.refused(restore_brief(matter, import), ErrorCode::MatterTurnRunning);
    assert!(error.to_string().contains("a turn is running"), "{error}");
    assert_eq!(
        std::fs::read_to_string(folder.join("brief.txt")).unwrap(),
        "x\n"
    );
    assert_eq!(
        matter_events(&host, matter).head(),
        head,
        "a refused restore logged"
    );
    host.shutdown();
}

#[test]
fn restore_and_preview_refused_for_another_matter_or_a_bad_scope() {
    let dir = Dir::new(
        "restore_and_preview_refused_for_another_matter_or_a_bad_scope",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (matter, _, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let (other, _, _) = matter_with_thread(&dir, &host, "o", &[("brief.txt", "y\n")], FAKE);
    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();
    let foreign = matter_events(&host, other)
        .checkpoint(CheckpointKind::Import)
        .unwrap();

    let error = host.refused(restore_brief(matter, foreign), ErrorCode::CommandInvalid);
    assert!(
        error.to_string().contains("not a version of this matter"),
        "{error}"
    );
    let preview = |checkpoint, scope| HostQuery::PreviewRestore {
        matter,
        checkpoint,
        scope,
    };
    let error = host
        .query(preview(foreign, RestoreRequest::Matter))
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");

    // The query's scope gets the command's shape checks.
    let brief = MatterPath::new("brief.txt").unwrap();
    for (paths, why) in [
        (Vec::new(), "at least one file"),
        (vec![brief.clone(), brief], "twice"),
    ] {
        let error = host
            .query(preview(import, RestoreRequest::Files { paths }))
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::CommandInvalid, "{why}: {error}");
    }
    host.shutdown();
}

/// The changes of a `CompareVersions` answer as `(path, kind)`, in the order the answer gives.
fn changes_of(answer: &QueryResult) -> Vec<(String, ChangeKind)> {
    let QueryResult::FileChanges(changes) = answer else {
        panic!("CompareVersions answered {answer:?}");
    };
    changes
        .iter()
        .map(|change: &FileChange| (change.path().as_str().to_owned(), change.change().clone()))
        .collect()
}

#[test]
fn compare_with_the_folder_now_lists_edits_adds_and_deletes_outside_a_turn() {
    let dir = Dir::new(
        "compare_with_the_folder_now_lists_edits_adds_and_deletes_outside_a_turn",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (matter, _, folder) = matter_with_thread(
        &dir,
        &host,
        "m",
        &[("brief.txt", "x\n"), ("gone.txt", "bye\n")],
        FAKE,
    );
    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();

    // The lawyer edits, adds and deletes files with no turn running.
    write::write(&folder.join("brief.txt"), b"x, edited\n");
    write::write(&folder.join("added.txt"), b"new\n");
    write::remove_file(&folder.join("gone.txt"));

    // No `to` means the folder now; `null` means the same.
    let answer = host
        .query(HostQuery::CompareVersions {
            matter,
            from: import,
            to: None,
        })
        .unwrap();
    assert_eq!(
        changes_of(&answer),
        [
            ("added.txt".to_owned(), ChangeKind::Added),
            ("brief.txt".to_owned(), ChangeKind::Modified),
            ("gone.txt".to_owned(), ChangeKind::Deleted),
        ]
    );
    host.shutdown();
}

#[test]
fn compare_with_the_folder_now_is_empty_when_nothing_changed() {
    let dir = Dir::new(
        "compare_with_the_folder_now_is_empty_when_nothing_changed",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (matter, _, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();

    let answer = host
        .query(HostQuery::CompareVersions {
            matter,
            from: import,
            to: None,
        })
        .unwrap();
    assert_eq!(answer, QueryResult::FileChanges(Vec::new()));
    host.shutdown();
}

#[test]
fn compare_with_the_folder_now_refuses_a_version_of_another_matter() {
    let dir = Dir::new(
        "compare_with_the_folder_now_refuses_a_version_of_another_matter",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (matter, _, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let (other, _, _) = matter_with_thread(&dir, &host, "o", &[("brief.txt", "y\n")], FAKE);
    let foreign = matter_events(&host, other)
        .checkpoint(CheckpointKind::Import)
        .unwrap();

    let error = host
        .query(HostQuery::CompareVersions {
            matter,
            from: foreign,
            to: None,
        })
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
    assert!(
        error.to_string().contains("not a version of this matter"),
        "{error}"
    );
    host.shutdown();
}
