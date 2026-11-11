//! An import of a folder whose `.clauseo/matter.json` names a matter this install has no record
//! of (its data was wiped, or the folder came from a colleague's install) adopts that matter: it
//! is imported under the marker's id and the marker is kept byte for byte, even when the import
//! fails. Two imports of one folder at once make one matter, and a marker naming a matter whose
//! history is here with no row is set aside instead (decision record 0022).

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_protocol::{
    CheckpointKind, CommandEnvelope, CommandOutcome, HostCommand, HostError, HostQuery, MatterId,
    MatterSource, QueryResult, RestoreRequest, StreamId, Title,
};

use crate::support::{Dir, Running, command_id, foreign_uuid, wait};

/// A marker naming `id` as another install wrote it: its own time and spacing.
fn foreign_marker(id: MatterId) -> String {
    format!(r#"{{ "version": 1, "matter_id": "{id}", "created_at": 1600000000000 }}"#)
}

fn marker(folder: &Path) -> PathBuf {
    folder.join(".clauseo").join("matter.json")
}

fn import_command(host: &Running, folder: &Path) -> HostCommand {
    HostCommand::CreateMatter {
        name: Title::new("Kaveri Agro").unwrap(),
        source: MatterSource::ExistingFolder {
            folder: host.folder_ref(folder),
        },
        client: None,
        next_date: None,
    }
}

fn created(answer: Result<CommandOutcome, HostError>) -> MatterId {
    match answer {
        Ok(CommandOutcome::MatterCreated { matter }) => matter,
        other => panic!("{other:?}"),
    }
}

/// The histories under the data folder.
fn histories(dir: &Dir) -> usize {
    std::fs::read_dir(dir.data().join("matters")).map_or(0, Iterator::count)
}

#[test]
fn a_marker_naming_a_matter_this_install_never_had_is_adopted_under_its_id() {
    let dir = Dir::new("adopt_foreign", &[]);
    let foreign = MatterId::new(foreign_uuid(42)).unwrap();
    let bytes = foreign_marker(foreign);
    let folder = dir.folder(
        "Copied",
        &[
            (".clauseo/matter.json", bytes.as_str()),
            ("brief.txt", "Clause 4\n"),
        ],
    );
    let host = dir.start(0);
    assert_eq!(
        created(host.dispatch(import_command(&host, &folder))),
        foreign
    );
    assert_eq!(std::fs::read_to_string(marker(&folder)).unwrap(), bytes);
    assert!(
        dir.data()
            .join("matters")
            .join(foreign.to_string())
            .is_dir()
    );
    assert!(
        host.matters()
            .iter()
            .any(|summary| summary.matter == foreign)
    );

    // Its import checkpoint opens on its worker.
    let mut events = host.events(StreamId::Matter(foreign));
    events.until("the import checkpoint", |e| {
        e.checkpoint(CheckpointKind::Import).is_some()
    });
    let checkpoint = events.checkpoint(CheckpointKind::Import).unwrap();
    let preview = host.query(HostQuery::PreviewRestore {
        undoes: None,
        matter: foreign,
        checkpoint,
        scope: RestoreRequest::Matter,
    });
    assert!(
        matches!(preview, Ok(QueryResult::RestorePreview(_))),
        "{preview:?}"
    );

    // The same folder again is answered with it, and nothing more is made.
    let listed = host.matters().len();
    let made = histories(&dir);
    assert_eq!(
        created(host.dispatch(import_command(&host, &folder))),
        foreign
    );
    assert_eq!((host.matters().len(), histories(&dir)), (listed, made));
    assert_eq!(std::fs::read_to_string(marker(&folder)).unwrap(), bytes);
    host.shutdown();
}

/// Two "New matter"s of one folder dispatched together get the same matter, for a plain folder
/// and for one whose marker the import adopts.
#[test]
fn two_imports_of_one_folder_at_once_make_one_matter() {
    let dir = Dir::new("adopt_twice", &[]);
    let foreign = MatterId::new(foreign_uuid(43)).unwrap();
    let bytes = foreign_marker(foreign);
    let plain = dir.folder("Plain", &[("a.txt", "a\n")]);
    let marked = dir.folder(
        "Marked",
        &[(".clauseo/matter.json", bytes.as_str()), ("b.txt", "b\n")],
    );
    let host = dir.start(0);
    let made = histories(&dir);
    for folder in [&plain, &marked] {
        let listed = host.matters().len();
        let send = || {
            host.handle.dispatch(CommandEnvelope {
                command_id: command_id(),
                command: import_command(&host, folder),
            })
        };
        let (first, second) = wait("two imports", futures::future::join(send(), send()));
        let (first, second) = (created(first), created(second));
        assert_eq!(first, second, "{}", folder.display());
        assert_eq!(host.matters().len(), listed + 1, "{}", folder.display());
    }
    assert_eq!(histories(&dir), made + 2);
    assert_eq!(std::fs::read_to_string(marker(&marked)).unwrap(), bytes);
    assert!(
        host.matters()
            .iter()
            .any(|summary| summary.matter == foreign)
    );
    host.shutdown();
}

/// An adopting import that fails (a file nobody may read, so Unix only, and not as root) leaves
/// the marker as it was, also after the 10 s marker re-check a failed import arms.
#[cfg(unix)]
#[test]
fn a_failed_adopting_import_leaves_the_marker_byte_for_byte() {
    let dir = Dir::new("adopt_failed", &[]);
    let foreign = MatterId::new(foreign_uuid(44)).unwrap();
    let bytes = foreign_marker(foreign);
    let folder = dir.folder(
        "Held",
        &[
            (".clauseo/matter.json", bytes.as_str()),
            ("brief.txt", "x\n"),
        ],
    );
    crate::write::set_mode(&folder.join("brief.txt"), 0o000);
    let reads_anyway = std::fs::read(folder.join("brief.txt")).is_ok();
    let host = dir.start(0);
    let answer = host.dispatch(import_command(&host, &folder));
    if !reads_anyway {
        assert!(answer.is_err(), "{answer:?}");
        assert!(
            host.matters()
                .iter()
                .all(|summary| summary.matter != foreign)
        );
        assert!(
            !dir.data()
                .join("matters")
                .join(foreign.to_string())
                .exists()
        );
    }
    host.clock.advance(Duration::from_secs(600));
    host.settle();
    assert_eq!(std::fs::read_to_string(marker(&folder)).unwrap(), bytes);
    crate::write::set_mode(&folder.join("brief.txt"), 0o644);
    host.shutdown();
}

/// A marker naming a matter whose history is here but on no row (a record that failed) is set
/// aside, and the folder imported as a new matter: two histories are never mixed.
#[test]
fn a_marker_naming_a_history_with_no_row_is_set_aside() {
    let dir = Dir::new("adopt_history", &[]);
    let foreign = MatterId::new(foreign_uuid(45)).unwrap();
    let bytes = foreign_marker(foreign);
    let folder = dir.folder("Orphan", &[(".clauseo/matter.json", bytes.as_str())]);
    let history = dir.data().join("matters").join(foreign.to_string());
    crate::write::create_dir_all(&history);
    let host = dir.start(0);
    let matter = created(host.dispatch(import_command(&host, &folder)));
    assert_ne!(matter, foreign);
    let now = std::fs::read_to_string(marker(&folder)).unwrap();
    assert!(now.contains(&matter.to_string()), "{now}");
    let aside: Vec<_> = std::fs::read_dir(folder.join(".clauseo"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name().unwrap() != "matter.json")
        .collect();
    assert_eq!(aside.len(), 1, "{aside:?}");
    assert_eq!(std::fs::read_to_string(&aside[0]).unwrap(), bytes);
    assert!(history.is_dir(), "the other history is left alone");
    host.shutdown();
}
