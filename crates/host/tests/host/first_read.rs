//! A cloud-only file the agent only read: the OS downloads it with the same content and
//! mtime, so it is the file's baseline, not a change. The turn has no `FilesChanged`, so the
//! room shows no "changed" line and offers no Undo, and the file stays.

use std::path::Path;
use std::sync::Arc;

use clauseo_host::WithPlaceholderProbe as _;
use clauseo_matters::PlaceholderProbe;
use clauseo_protocol::{CheckpointKind, DomainEvent, StopReason, StreamId};

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
