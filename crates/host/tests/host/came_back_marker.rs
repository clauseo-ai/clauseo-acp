//! The marker re-check (`made::recheck_marker`): a create or import that failed on its worker
//! removed the `.clauseo/matter.json` it wrote; a sync client can put it back, and every later
//! "New matter" there would be refused as already a matter. 10 s later (host clock) the host
//! removes it once more, only when it names the id minted for that create or import.
//!
//! The import fails because the folder holds a file nobody may read (mode 000, so Unix only):
//! the walk after `matter.json` is written fails on it. `bring_back_after_delete` puts the marker
//! back the moment the failed import removes it, which also tells the test the minted id.
//!
//! The re-check runs off the actor on a thread of its own, so the tests wait for its result
//! in real time, up to the watchdog.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_protocol::{HostCommand, MatterId, MatterSource, Title};

use crate::support::{Dir, Running, foreign_uuid, wait};
use crate::write;

/// `made::MARKER_RECHECK`.
const RECHECK: Duration = Duration::from_secs(10);
/// The off-actor job's wait (`prepare::PREPARE_WAIT`), armed when the re-check starts.
const PREPARE_WAIT: Duration = Duration::from_secs(5);

/// A folder whose import fails after its marker is written: `brief.txt` cannot be read.
fn unreadable_folder(dir: &Dir) -> PathBuf {
    let folder = dir.folder("held", &[("brief.txt", "Clause 4\n"), ("notes.txt", "n\n")]);
    write::set_mode(&folder.join("brief.txt"), 0o000);
    folder
}

/// Imports `folder`, which must fail on the worker.
fn failed_import(host: &Running, name: &str, folder: &Path) {
    let folder_ref = host.folder_ref(folder);
    let refused = host.dispatch(HostCommand::CreateMatter {
        name: Title::new(name).unwrap(),
        source: MatterSource::ExistingFolder { folder: folder_ref },
        client: None,
        next_date: None,
    });
    assert!(
        refused.is_err(),
        "the import of an unreadable file: {refused:?}"
    );
}

fn marker(folder: &Path) -> PathBuf {
    folder.join(".clauseo").join("matter.json")
}

/// The id `folder`'s marker names.
fn marker_id(folder: &Path) -> MatterId {
    let text = std::fs::read_to_string(marker(folder)).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let id = json["matter_id"]
        .as_str()
        .unwrap_or_else(|| panic!("{text}"));
    MatterId::new(uuid::Uuid::parse_str(id).unwrap()).unwrap()
}

/// Writes `folder`'s marker naming `id`, as `matters` writes one.
fn write_marker(folder: &Path, id: MatterId) {
    let json = serde_json::json!({ "version": 1, "matter_id": id.to_string(), "created_at": 0 });
    write::write(&marker(folder), json.to_string().as_bytes());
}

/// Waits, in real time up to the watchdog, until `done`.
fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    wait(what, async {
        while !done() {
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
    });
}

#[test]
fn came_back_marker_of_a_failed_import_is_removed_once() {
    let dir = Dir::new("came_back_marker_removed", &[]);
    let host = dir.start(0);
    let folder = unreadable_folder(&dir);
    clauseo_matters::test_support::bring_back_after_delete(marker(&folder));
    failed_import(&host, "held", &folder);
    assert!(
        marker(&folder).exists(),
        "the hook did not put the marker back"
    );
    let minted = marker_id(&folder);
    assert!(
        host.matters().iter().all(|m| m.matter != minted),
        "the failed import is listed"
    );

    host.wait_armed(RECHECK);
    host.clock.advance(Duration::from_millis(9_999));
    host.settle();
    assert!(marker(&folder).exists(), "removed before 10 s");
    host.clock.advance(Duration::from_millis(1));
    eventually("the marker removed", || !marker(&folder).exists());
    assert!(
        !folder.join(".clauseo").exists(),
        "the empty .clauseo was left"
    );
    assert!(folder.join("brief.txt").exists());

    // Once only: a marker put back again later stays.
    write_marker(&folder, minted);
    host.clock.advance(Duration::from_secs(600));
    host.settle();
    assert_eq!(marker_id(&folder), minted);
    host.shutdown();
}

#[test]
fn came_back_marker_naming_another_matter_is_left() {
    let dir = Dir::new("came_back_marker_other", &[]);
    let host = dir.start(0);
    let folder = unreadable_folder(&dir);
    failed_import(&host, "held", &folder);
    assert!(
        !folder.join(".clauseo").exists(),
        "the failed import left .clauseo"
    );
    // A marker another computer's Clauseo wrote, synced in before the re-check.
    let other = MatterId::new(foreign_uuid(7)).unwrap();
    write_marker(&folder, other);

    host.wait_armed(RECHECK);
    let before = host.clock.armed();
    host.clock.advance(RECHECK);
    // The re-check started (its off-actor wait armed), then answered (that wait dropped).
    host.wait_armed(PREPARE_WAIT);
    eventually("the re-check answered", || host.clock.armed() < before);
    assert_eq!(
        marker_id(&folder),
        other,
        "another matter's marker was touched"
    );
    host.shutdown();
}
