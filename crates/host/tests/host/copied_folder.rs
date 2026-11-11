//! A folder whose marker names a matter this install knows (listed or forgotten), recorded at
//! another path: a copy or a move (decision record 0022). A copy (the recorded folder still
//! there) becomes a new matter with its marker set aside, and the matter it was copied from is
//! untouched. A move (the recorded folder gone) is that matter at the new path: a listed one is
//! relinked there and opens, a forgotten one comes back there. The scan passes for all four.

use std::path::{Path, PathBuf};

use clauseo_protocol::{
    CheckpointKind, CommandOutcome, HostCommand, HostQuery, MatterId, QueryResult, RestoreRequest,
    StreamId,
};

use crate::matters::{created, import, summary};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};

fn dir(test: &str) -> Dir {
    Dir::new(test, &[(FAKE, Scenario::Local("replay-seed"))])
}

fn marker(folder: &Path) -> PathBuf {
    folder.join(".clauseo").join("matter.json")
}

/// Copies `from` to `to`, every file with its bytes, `.clauseo` included.
fn copy_folder(from: &Path, to: &Path) {
    crate::write::create_dir_all(to);
    for entry in std::fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_folder(&path, &target);
        } else {
            crate::write::write(&target, &std::fs::read(&path).unwrap());
        }
    }
}

/// `ScanFolder` of `folder`, which must end `Done`.
fn scans(host: &Running, folder: &Path) {
    let folder = host.folder_ref(folder);
    assert_eq!(
        host.dispatch(HostCommand::ScanFolder { folder }),
        Ok(CommandOutcome::Done)
    );
}

/// The files in `folder`'s `.clauseo` other than `matter.json`, with their bytes.
fn set_aside(folder: &Path) -> Vec<(String, Vec<u8>)> {
    let mut found: Vec<_> = std::fs::read_dir(folder.join(".clauseo"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name().unwrap() != "matter.json")
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&path).unwrap())
        })
        .collect();
    found.sort();
    found
}

/// `matter`'s import checkpoint previews on its worker: it opens from its folder.
fn opens(host: &Running, matter: MatterId) {
    let mut events = host.events(StreamId::Matter(matter));
    events.until("the import checkpoint", |e| {
        e.checkpoint(CheckpointKind::Import).is_some()
    });
    let checkpoint = events.checkpoint(CheckpointKind::Import).unwrap();
    let preview = host.query(HostQuery::PreviewRestore {
        undoes: None,
        matter,
        checkpoint,
        scope: RestoreRequest::Matter,
    });
    assert!(
        matches!(preview, Ok(QueryResult::RestorePreview(_))),
        "{preview:?}"
    );
}

/// The copy became a new matter: a new id, no conversations, its own marker naming it, and the
/// copied marker set aside byte for byte.
fn a_new_matter(host: &Running, copy: &Path, old: MatterId, copied: &[u8]) -> MatterId {
    let new = created(import(host, "Copy", copy, (None, None)));
    assert_ne!(new, old);
    assert_eq!(host.threads(new).len(), 0);
    let now = std::fs::read_to_string(marker(copy)).unwrap();
    assert!(now.contains(&new.to_string()), "{now}");
    let aside = set_aside(copy);
    assert_eq!(aside.len(), 1, "{aside:?}");
    assert!(aside[0].0.starts_with("matter.old-"), "{aside:?}");
    assert_eq!(aside[0].1, copied);
    new
}

#[test]
fn a_forgotten_matters_folder_copied_beside_it_becomes_a_new_matter() {
    let dir = dir("copied_forgotten");
    let host = dir.start(0);
    let (old, _thread, folder) = matter_with_thread(&dir, &host, "A", &[("a.txt", "a\n")], FAKE);
    host.ok(HostCommand::ForgetMatter { matter: old });
    let copied = std::fs::read(marker(&folder)).unwrap();
    let copy = dir.work_path("A copy");
    copy_folder(&folder, &copy);
    scans(&host, &copy);
    a_new_matter(&host, &copy, old, &copied);
    assert!(
        host.matters().iter().all(|m| m.matter != old),
        "A stays forgotten"
    );
    assert_eq!(std::fs::read(marker(&folder)).unwrap(), copied);
    host.shutdown();

    // A's row: still forgotten, at its own folder, its conversation still its own.
    let store = crate::titles::open_store(dir.data());
    let row = wait("matters", store.matters())
        .unwrap()
        .into_iter()
        .find(|row| row.id == old)
        .unwrap();
    assert!(row.forgotten.is_some(), "{row:?}");
    assert_eq!(row.folder_path, folder.to_string_lossy());
    wait("close", store.close()).unwrap();
    let host = dir.start(1);
    assert_eq!(created(import(&host, "A", &folder, (None, None))), old);
    assert_eq!(host.threads(old).len(), 1);
    host.shutdown();
}

#[test]
fn a_listed_matters_folder_copied_beside_it_becomes_a_new_matter() {
    let dir = dir("copied_listed");
    let host = dir.start(0);
    let (old, _thread, folder) = matter_with_thread(&dir, &host, "A", &[("a.txt", "a\n")], FAKE);
    let copied = std::fs::read(marker(&folder)).unwrap();
    let copy = dir.work_path("A copy");
    copy_folder(&folder, &copy);
    scans(&host, &copy);
    a_new_matter(&host, &copy, old, &copied);
    let a = summary(&host, old);
    assert_eq!(a.folder_display, folder.display().to_string());
    assert_eq!(host.threads(old).len(), 1);
    assert_eq!(std::fs::read(marker(&folder)).unwrap(), copied);
    opens(&host, old);
    host.shutdown();
}

#[test]
fn a_listed_matters_folder_moved_is_relinked_where_it_is_now() {
    let dir = dir("moved_listed");
    let host = dir.start(0);
    let (old, _thread, folder) = matter_with_thread(&dir, &host, "A", &[("a.txt", "a\n")], FAKE);
    let bytes = std::fs::read(marker(&folder)).unwrap();
    let moved = dir.work_path("A moved");
    crate::write::rename(&folder, &moved);
    scans(&host, &moved);
    let listed = host.matters().len();
    assert_eq!(created(import(&host, "A", &moved, (None, None))), old);
    assert_eq!(host.matters().len(), listed, "no second matter");
    assert_eq!(
        summary(&host, old).folder_display,
        moved.display().to_string()
    );
    assert_eq!(host.threads(old).len(), 1);
    assert_eq!(std::fs::read(marker(&moved)).unwrap(), bytes);
    assert_eq!(set_aside(&moved), []);
    opens(&host, old);
    host.shutdown();
}

#[test]
fn a_forgotten_matters_folder_moved_comes_back_where_it_is_now() {
    let dir = dir("moved_forgotten");
    let host = dir.start(0);
    let (old, _thread, folder) = matter_with_thread(&dir, &host, "A", &[("a.txt", "a\n")], FAKE);
    host.ok(HostCommand::ForgetMatter { matter: old });
    let moved = dir.work_path("A moved");
    crate::write::rename(&folder, &moved);
    scans(&host, &moved);
    assert_eq!(created(import(&host, "A", &moved, (None, None))), old);
    assert_eq!(
        summary(&host, old).folder_display,
        moved.display().to_string()
    );
    assert_eq!(host.threads(old).len(), 1);
    assert_eq!(set_aside(&moved), []);
    host.shutdown();
}
