//! A cloud-only file the agent only read: the OS downloads it with the same content and
//! mtime, so it is the file's baseline, not a change. The turn has no `FilesChanged`, so the
//! room shows no "changed" line and offers no Undo, and the file stays. And one the agent
//! deleted: Clauseo kept no copy, so the turn's `FilesChanged` lists it as gone and a notice
//! follows it. And one the agent changed in place: no copy of how it was before, so a notice
//! names it at the turn's end.

use std::path::Path;
use std::sync::Arc;

use clauseo_host::WithPlaceholderProbe as _;
use clauseo_matters::PlaceholderProbe;
use clauseo_protocol::{
    CheckpointKind, DomainEvent, NoticeKind, SkippedFile, SkippedFileReason, StopReason, StreamId,
};

use crate::support::{Dir, FAKE, Scenario, matter_with_thread};

/// Cloud-only while `<file>.cloud.tmp` is beside it: an excluded name, never versioned, so the
/// agent's scenario "downloads" the file by deleting that marker.
struct MarkerProbe;

impl PlaceholderProbe for MarkerProbe {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        path.with_file_name(format!("{name}.cloud.tmp")).exists()
    }
}

#[test]
fn a_turn_that_only_read_a_cloud_only_file_has_no_files_changed() {
    let dir = Dir::new(
        "a_turn_that_only_read_a_cloud_only_file_has_no_files_changed",
        &[(FAKE, Scenario::Local("read-cloud-only"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_placeholder_probe(Arc::new(MarkerProbe))
    });
    let seed = [
        ("brief.txt", "Clause 4 draft\n"),
        ("notes.txt", "The lawyer's notes\n"),
        ("notes.txt.cloud.tmp", ""),
    ];
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", &seed, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the notes.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn);
    assert!(
        !folder.join("notes.txt.cloud.tmp").exists(),
        "the scenario downloads notes.txt"
    );

    // The import skipped the placeholder; the after-turn checkpoint versions it, yet the turn
    // changed nothing.
    let mut matter_events = host.events(StreamId::Matter(matter));
    matter_events.until_caught_up();
    let import = matter_events.events().find_map(|event| match event {
        DomainEvent::MatterCreated(created) => created.import(),
        _ => None,
    });
    assert_eq!(
        import.map(|summary| (summary.files, summary.cloud_only)),
        Some((1, 1)),
        "the import did not skip notes.txt: {}",
        matter_events.dump()
    );
    let after = matter_events.checkpoint(CheckpointKind::AfterTurn);
    assert!(after.is_some(), "no AfterTurn: {}", matter_events.dump());
    assert_eq!(ended.checkpoint_after, after);
    events.until_caught_up();
    assert!(
        events.files_changed(turn).is_none(),
        "a FilesChanged: {}",
        events.dump()
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("notes.txt")).unwrap(),
        "The lawyer's notes\n"
    );
    host.shutdown();
}

#[test]
fn a_cloud_only_file_the_turn_deleted_is_gone_in_files_changed_and_a_notice() {
    let dir = Dir::new(
        "a_cloud_only_file_the_turn_deleted_is_gone_in_files_changed_and_a_notice",
        &[(FAKE, Scenario::Local("delete-cloud-only"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_placeholder_probe(Arc::new(MarkerProbe))
    });
    let seed = [
        ("brief.txt", "Clause 4 draft\n"),
        ("notes.txt", "The lawyer's notes\n"),
        ("notes.txt.cloud.tmp", ""),
    ];
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &seed, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Delete the notes.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn);
    assert!(!folder.join("notes.txt").exists());
    events.until_caught_up();
    let notes = SkippedFile {
        path: clauseo_protocol::MatterPath::new("notes.txt").unwrap(),
        reason: SkippedFileReason::CloudOnly,
    };
    let changed = events.files_changed(turn);
    let changed = changed.unwrap_or_else(|| panic!("no FilesChanged: {}", events.dump()));
    assert_eq!(changed.files, [], "{changed:?}");
    assert_eq!(changed.gone, std::slice::from_ref(&notes));
    // The notice comes right after the FilesChanged, naming the file.
    let all: Vec<&DomainEvent> = events.events().collect();
    let at = all
        .iter()
        .position(|event| matches!(event, DomainEvent::FilesChanged(_)))
        .unwrap();
    let Some(DomainEvent::Notice(notice)) = all.get(at + 1) else {
        panic!("no notice after FilesChanged: {}", events.dump());
    };
    assert_eq!(
        notice.kind,
        Some(NoticeKind::FilesGoneNoCopy {
            files: vec![notes],
            more: 0,
        })
    );
    assert_eq!(
        notice.text,
        "notes.txt was deleted during this answer. Clauseo had no copy of it to bring back."
    );
    host.shutdown();
}

#[test]
fn a_cloud_only_file_the_turn_changed_in_place_is_named_in_a_notice() {
    let dir = Dir::new(
        "a_cloud_only_file_the_turn_changed_in_place_is_named_in_a_notice",
        &[(FAKE, Scenario::Local("change-cloud-only"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_placeholder_probe(Arc::new(MarkerProbe))
    });
    let seed = [
        ("brief.txt", "Clause 4 draft\n"),
        ("notes.txt", "The lawyer's notes\n"),
        ("notes.txt.cloud.tmp", ""),
    ];
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &seed, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Rewrite the notes.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn);
    assert_eq!(
        std::fs::read_to_string(folder.join("notes.txt")).unwrap(),
        "Rewritten by the agent, longer than before\n"
    );
    events.until_caught_up();
    let notices: Vec<_> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => notice.kind.clone(),
            _ => None,
        })
        .filter(|kind| matches!(kind, NoticeKind::FilesChangedNoCopy { .. }))
        .collect();
    let notes = SkippedFile {
        path: clauseo_protocol::MatterPath::new("notes.txt").unwrap(),
        reason: SkippedFileReason::CloudOnly,
    };
    assert_eq!(
        notices,
        [NoticeKind::FilesChangedNoCopy {
            files: vec![notes],
            more: 0,
        }],
        "{}",
        events.dump()
    );
    host.shutdown();
}
