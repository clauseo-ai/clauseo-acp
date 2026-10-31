//! A matter whose folder moved (`Matter::locate`, "Locate folder") or lost its marker
//! (`Matter::relink`): what each accepts, what each refuses, and that a refusal changes nothing.

use std::path::{Path, PathBuf};

use clauseo_matters::{Error, Matter, MissingReason, NewMatter, Refusal};
use clauseo_protocol::ErrorCode;

use crate::fixture::{Fixture, at, cid, matter_id, meta};
use crate::support;

const MARKER: &str = ".clauseo/matter.json";

fn locate(fx: &Fixture, folder: &Path, refused: &[PathBuf]) -> Result<Matter, Error> {
    Matter::locate(fx.settings(), matter_id(1), folder, refused)
}

fn relink(fx: &Fixture, folder: &Path) -> Result<Matter, Error> {
    Matter::relink(fx.settings(), matter_id(1), folder, at(0), &[])
}

/// The history's `config`, which holds `core.worktree`.
fn config(fx: &Fixture) -> Vec<u8> {
    std::fs::read(fx.git_dir().join("config")).unwrap()
}

fn assert_missing(err: &Error, expected: MissingReason) {
    assert!(
        matches!(err, Error::FolderMissing { reason, .. } if *reason == expected),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::MatterFolderMissing);
}

fn assert_refused(err: &Error, expected: &Refusal) {
    assert!(
        matches!(err, Error::FolderRefused { reason, .. } if reason == expected),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::MatterFolderRefused);
}

/// Matter 1 imported with `a.txt`, then its folder renamed to `<dir>/moved`, which is returned.
fn moved(name: &str) -> (Fixture, PathBuf) {
    let fx = Fixture::new(name);
    fx.write("a.txt", "a");
    drop(fx.import());
    let moved = fx.dir.path().join("moved");
    support::rename(&fx.folder, &moved);
    (fx, moved)
}

#[test]
fn locate_opens_moved_folder() {
    let (fx, moved) = moved("locate-moved");
    let err = fx.open(&fx.folder).unwrap_err();
    assert_missing(&err, MissingReason::NoFolder);

    let mut matter = locate(&fx, &moved, &[]).unwrap();
    assert_eq!(matter.folder(), moved);
    assert_eq!(matter.before_turn(&meta(2)).unwrap().outcome.id(), cid(1));
    let text = String::from_utf8(config(&fx)).unwrap();
    let quoted = moved.to_str().unwrap().replace('\\', "\\\\");
    assert!(text.contains(&format!("worktree = \"{quoted}\"")), "{text}");
    drop(matter);
    assert_eq!(fx.open(&moved).unwrap().folder(), moved);
}

#[test]
fn locate_same_folder_writes_nothing() {
    let fx = Fixture::new("locate-same");
    fx.write("a.txt", "a");
    drop(fx.import());
    let path = fx.git_dir().join("config");
    let (before, modified) = (config(&fx), path.metadata().unwrap().modified().unwrap());

    let matter = locate(&fx, &fx.folder, &[]).unwrap();
    assert_eq!(matter.folder(), fx.folder);
    assert_eq!(config(&fx), before);
    assert_eq!(path.metadata().unwrap().modified().unwrap(), modified);
}

#[test]
fn locate_refuses_folder_without_marker_or_unreadable_or_missing() {
    let (fx, _moved) = moved("locate-no-marker");
    let before = config(&fx);

    let plain = fx.dir.path().join("plain");
    support::write(plain.join("a.txt"), "a");
    assert_missing(
        &locate(&fx, &plain, &[]).unwrap_err(),
        MissingReason::NoMatterJson,
    );
    assert!(!plain.join(".clauseo").exists());

    let garbage = fx.dir.path().join("garbage");
    support::write(garbage.join(MARKER), "{not json");
    assert_missing(
        &locate(&fx, &garbage, &[]).unwrap_err(),
        MissingReason::Unreadable,
    );
    assert_eq!(std::fs::read(garbage.join(MARKER)).unwrap(), b"{not json");

    let gone = fx.dir.path().join("gone");
    assert_missing(
        &locate(&fx, &gone, &[]).unwrap_err(),
        MissingReason::NoFolder,
    );
    assert_eq!(config(&fx), before);
}

#[test]
fn locate_refuses_other_matter() {
    let (fx, _moved) = moved("locate-other");
    let other = fx.dir.path().join("other");
    let new = NewMatter {
        id: matter_id(2),
        folder: other.clone(),
        checkpoint: cid(100),
        ..fx.new_matter()
    };
    drop(Matter::create(fx.settings(), new).unwrap());
    let (before, marker) = (config(&fx), std::fs::read(other.join(MARKER)).unwrap());

    let err = locate(&fx, &other, &[]).unwrap_err();
    assert_missing(&err, MissingReason::OtherMatter(matter_id(2)));
    assert_eq!(config(&fx), before);
    assert_eq!(std::fs::read(other.join(MARKER)).unwrap(), marker);
}

#[test]
fn locate_refuses_listed_folder_and_matters_root() {
    let (fx, moved) = moved("locate-refused");
    let before = config(&fx);

    let err = locate(&fx, &moved, std::slice::from_ref(&moved)).unwrap_err();
    assert_refused(&err, &Refusal::Listed(moved.clone()));

    // The folder, marker and all, moved inside the matters root.
    let inside = fx.root().join("inside");
    support::rename(&moved, &inside);
    let err = locate(&fx, &inside, &[]).unwrap_err();
    assert_refused(&err, &Refusal::MattersRoot);
    assert_eq!(config(&fx), before);
}

#[cfg(unix)]
#[test]
fn locate_refuses_symlinked_spelling_of_listed_folder() {
    let (fx, moved) = moved("locate-symlink");
    let link = fx.dir.path().join("link");
    support::symlink(&moved, &link);
    let err = locate(&fx, &link, std::slice::from_ref(&moved)).unwrap_err();
    assert_refused(&err, &Refusal::Listed(moved));
}

#[test]
fn relink_restores_deleted_marker() {
    let fx = Fixture::new("relink-deleted");
    fx.write("a.txt", "a");
    drop(fx.import());
    let original = fx.read(MARKER);
    support::remove_file(fx.file(MARKER));
    let err = fx.open(&fx.folder).unwrap_err();
    assert_missing(&err, MissingReason::NoMatterJson);

    let mut matter = relink(&fx, &fx.folder).unwrap();
    assert_eq!(fx.read(MARKER), original);
    assert_eq!(matter.before_turn(&meta(2)).unwrap().outcome.id(), cid(1));
    drop(matter);
    drop(fx.open(&fx.folder).unwrap());

    // The whole `.clauseo` gone.
    support::remove_dir_all(fx.file(".clauseo"));
    drop(relink(&fx, &fx.folder).unwrap());
    assert_eq!(fx.read(MARKER), original);
}

#[test]
fn relink_repairs_unreadable_marker() {
    let fx = Fixture::new("relink-unreadable");
    fx.write("a.txt", "a");
    drop(fx.import());
    let original = fx.read(MARKER);
    fx.write(MARKER, "{not json");
    assert_missing(&fx.open(&fx.folder).unwrap_err(), MissingReason::Unreadable);

    drop(relink(&fx, &fx.folder).unwrap());
    assert_eq!(fx.read(MARKER), original);
}

#[test]
fn relink_refuses_moved_folder() {
    let fx = Fixture::new("relink-moved");
    fx.write("a.txt", "a");
    drop(fx.import());
    support::remove_file(fx.file(MARKER));
    let moved = fx.dir.path().join("moved");
    support::rename(&fx.folder, &moved);
    let before = config(&fx);

    let err = relink(&fx, &moved).unwrap_err();
    assert_missing(&err, MissingReason::NoMatterJson);
    assert!(!moved.join(MARKER).exists());
    assert_eq!(config(&fx), before);

    support::write(moved.join(MARKER), "{not json");
    let err = relink(&fx, &moved).unwrap_err();
    assert_missing(&err, MissingReason::Unreadable);
    assert_eq!(std::fs::read(moved.join(MARKER)).unwrap(), b"{not json");

    // Listed: refused before the marker is looked at.
    let err = Matter::relink(
        fx.settings(),
        matter_id(1),
        &moved,
        at(0),
        std::slice::from_ref(&moved),
    )
    .unwrap_err();
    assert_refused(&err, &Refusal::Listed(moved));
}

/// The recorded folder spelled through a link is the same folder.
#[cfg(unix)]
#[test]
fn relink_through_symlinked_spelling() {
    let fx = Fixture::new("relink-symlink");
    fx.write("a.txt", "a");
    drop(fx.import());
    let original = fx.read(MARKER);
    support::remove_file(fx.file(MARKER));
    let link = fx.dir.path().join("link");
    support::symlink(&fx.folder, &link);

    drop(relink(&fx, &link).unwrap());
    assert_eq!(fx.read(MARKER), original);
}

#[test]
fn relink_never_overwrites_other_matter() {
    let fx = Fixture::new("relink-other");
    fx.write("a.txt", "a");
    drop(fx.import());
    support::remove_file(fx.file(MARKER));
    // The lawyer imported the folder again, as matter 2.
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let new = NewMatter {
        id: matter_id(2),
        checkpoint: cid(100),
        ..fx.new_matter()
    };
    drop(Matter::import(fx.settings(), new, &[], &mut |_| {}, &cancel).unwrap());
    let marker = fx.read(MARKER);

    let err = relink(&fx, &fx.folder).unwrap_err();
    assert_missing(&err, MissingReason::OtherMatter(matter_id(2)));
    assert_eq!(fx.read(MARKER), marker);
}

#[test]
fn relink_with_marker_is_open() {
    let fx = Fixture::new("relink-marker");
    fx.write("a.txt", "a");
    drop(fx.import());
    let marker = fx.file(MARKER);
    let (before, modified) = (
        fx.read(MARKER),
        marker.metadata().unwrap().modified().unwrap(),
    );

    let matter = relink(&fx, &fx.folder).unwrap();
    assert_eq!(matter.last_checkpoint(), Some(cid(1)));
    assert_eq!(fx.read(MARKER), before);
    assert_eq!(marker.metadata().unwrap().modified().unwrap(), modified);
}

#[test]
fn relink_writes_nothing_when_history_is_broken() {
    let fx = Fixture::new("relink-broken");
    fx.write("a.txt", "a");
    drop(fx.import());
    support::remove_file(fx.file(MARKER));
    support::remove_file(fx.git_dir().join("refs/clauseo/timeline"));

    let err = relink(&fx, &fx.folder).unwrap_err();
    assert_eq!(err.code(), ErrorCode::MatterCheckpointFailed, "{err:?}");
    assert!(!fx.exists(MARKER));
}
