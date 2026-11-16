//! The secret file taken through one handle (`write/secret.rs`, the review of #1093): a link or
//! a second name planted at `<run>/proxy-secret` is refused, and what it points at is left as it
//! was; a plain file is read, emptied through the handle and deleted. The links are made in the
//! test's own temp folder, against files the test made.

use std::path::Path;

use super::super::{Os, Source, proxy_secret};
use super::secret::{SECRET, Scratch, VAR, refusal};
use crate::browser_guard::proxy_auth::Password;
use crate::write;
use crate::write::secret::{Refused, take};

/// The refusal of anything but a plain file with one name.
const NOT_PLAIN: &str = "CLAUSEO_PROXY_SECRET_FILE must name a plain file with one name, not a \
                         link, a folder or a device";

/// The bytes at `file`, read without changing it.
fn content(file: &Path) -> Vec<u8> {
    std::fs::read(file).unwrap()
}

/// A plain file is read, then emptied through the guard's handle before its name goes: a handle
/// opened before the guard's sees it empty.
#[test]
fn a_plain_file_is_read_emptied_and_deleted() {
    let scratch = Scratch::new("plain");
    let file = scratch.file("proxy-secret", format!("{SECRET}\n").as_bytes());
    let held = std::fs::File::open(&file).unwrap();
    let read = proxy_secret(&source(&file), Os::CURRENT);
    assert_eq!(read, Ok(Password::parse(SECRET)));
    assert_eq!(
        held.metadata().unwrap().len(),
        0,
        "not emptied before the delete"
    );
    drop(held);
    assert!(Path::new(&file).symlink_metadata().is_err(), "not deleted");
}

/// A symlink at the name, to a file holding a valid password, is refused: the guard reads,
/// empties and deletes nothing, so the target keeps its bytes and the link stays.
#[cfg(unix)]
#[test]
fn a_symlink_at_the_name_is_refused_and_its_target_left() {
    let scratch = Scratch::new("symlink");
    let target = scratch.file("target", format!("{SECRET}\n").as_bytes());
    let link = scratch.0.join("proxy-secret");
    write::symlink(Path::new(&target), &link).unwrap();
    assert_eq!(refusal(Os::Unix, link.to_str().unwrap()), NOT_PLAIN);
    assert_eq!(
        content(Path::new(&target)),
        format!("{SECRET}\n").as_bytes()
    );
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
}

/// Windows: a symlink at the name is opened itself (`FILE_FLAG_OPEN_REPARSE_POINT`) and refused
/// as a reparse point; the target keeps its bytes. Skipped where the user may not make links.
#[cfg(windows)]
#[test]
fn a_reparse_point_at_the_name_is_refused_and_its_target_left() {
    let scratch = Scratch::new("reparse");
    let target = scratch.file("target", format!("{SECRET}\n").as_bytes());
    let link = scratch.0.join("proxy-secret");
    if write::symlink_file(Path::new(&target), &link).is_err() {
        return;
    }
    assert_eq!(take(&link, 128).unwrap_err(), Refused::NotPlain);
    assert_eq!(refusal(Os::Windows, link.to_str().unwrap()), NOT_PLAIN);
    assert_eq!(
        content(Path::new(&target)),
        format!("{SECRET}\n").as_bytes()
    );
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
}

/// A second name for another file (a hard link, two links in all) is refused: neither name is
/// read, emptied or deleted.
#[test]
fn a_file_with_two_names_is_refused_and_left() {
    let scratch = Scratch::new("hard");
    let target = scratch.file("target", format!("{SECRET}\n").as_bytes());
    let link = scratch.0.join("proxy-secret");
    write::hard_link(Path::new(&target), &link).unwrap();
    assert_eq!(take(&link, 128).unwrap_err(), Refused::NotPlain);
    assert_eq!(refusal(Os::CURRENT, link.to_str().unwrap()), NOT_PLAIN);
    for name in [Path::new(&target), link.as_path()] {
        assert_eq!(content(name), format!("{SECRET}\n").as_bytes(), "{name:?}");
    }
}

/// A folder at the name is refused before the open, and the open itself refuses one too; the
/// folder stays.
#[test]
fn a_folder_at_the_name_is_refused() {
    let scratch = Scratch::new("folder");
    let folder = scratch.0.join("proxy-secret");
    write::create_dir(&folder).unwrap();
    let error = refusal(Os::CURRENT, folder.to_str().unwrap());
    assert!(
        error.contains("must be the absolute path of an existing file"),
        "{error}"
    );
    let refused = take(&folder, 128).unwrap_err();
    assert!(
        matches!(refused, Refused::NotPlain | Refused::Unopened),
        "{refused:?}"
    );
    assert!(folder.is_dir());
}

/// Windows, the review's attack: a file another process holds open without delete sharing
/// fails the guard's one open (which asks for `DELETE`), so it is neither read nor emptied.
#[cfg(windows)]
#[test]
fn a_file_held_without_delete_sharing_is_refused_and_left() {
    let scratch = Scratch::new("held");
    let file = scratch.file("proxy-secret", format!("{SECRET}\n").as_bytes());
    let held = write::open_unshared(Path::new(&file)).unwrap();
    assert_eq!(refusal(Os::Windows, &file), super::secret::UNREADABLE);
    drop(held);
    assert_eq!(content(Path::new(&file)), format!("{SECRET}\n").as_bytes());
}

/// The source with the proxy set and [`VAR`] naming `file`.
fn source(file: &str) -> Source {
    super::source_for(Os::CURRENT, &[(VAR, file)], "")
}
