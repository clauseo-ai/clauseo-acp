//! `held_open_at` and `matter_path_in`: a path as an agent names it, resolved into the matter
//! folder before the owner-file check, so a symlink, a `..` part or (macOS, Windows) another
//! case spelling still finds Word's owner file, and a path outside the folder is never checked.

use std::path::{Path, PathBuf};

use clauseo_matters::{HeldOpen, held_open_at, matter_path_in};
use clauseo_protocol::MatterPath;

use crate::support::{self, TestDir};

const OWNER: &str = "owner file: open in Word";

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

/// `<dir>/m` with `Invoice.docx` open in Word (`~$nvoice.docx`) and `Free.docx`, and
/// `<dir>/m-elsewhere` beside it, whose name starts with the matter's, with an open
/// `Invoice.docx` too.
fn matter(dir: &TestDir) -> PathBuf {
    let folder = dir.path().join("m");
    for (name, text) in [
        ("m/Invoice.docx", "one"),
        ("m/~$nvoice.docx", OWNER),
        ("m/Free.docx", "free"),
        ("m-elsewhere/Invoice.docx", "x"),
        ("m-elsewhere/~$nvoice.docx", OWNER),
    ] {
        support::write(dir.path().join(name), text);
    }
    folder
}

fn owner() -> HeldOpen {
    HeldOpen::OwnerFile("~$nvoice.docx".to_owned())
}

#[test]
fn paths_inside_the_folder_are_checked_however_they_are_spelled() {
    let dir = TestDir::new("held-at-inside");
    let folder = matter(&dir);
    let held = |agent_path: &Path| held_open_at(&folder, agent_path);
    let invoice = Some((path("Invoice.docx"), owner()));
    assert_eq!(held(&folder.join("Invoice.docx")), invoice);
    assert_eq!(held(Path::new("Invoice.docx")), invoice);
    assert_eq!(
        held(&folder.join("sub/../Invoice.docx")),
        None,
        "no folder sub"
    );
    support::create_dir_all(folder.join("sub"));
    assert_eq!(held(&folder.join("sub/../Invoice.docx")), invoice);
    assert_eq!(held(&folder.join("Free.docx")), None);
    assert_eq!(
        matter_path_in(&folder, &folder.join("New.docx")),
        Some(path("New.docx")),
        "a missing file resolves through its parent"
    );
}

#[test]
fn paths_outside_the_folder_are_not_checked() {
    let dir = TestDir::new("held-at-outside");
    let folder = matter(&dir);
    let elsewhere = dir.path().join("m-elsewhere/Invoice.docx");
    assert_eq!(
        held_open_at(&folder, &elsewhere),
        None,
        "same text prefix, other folder"
    );
    assert_eq!(
        held_open_at(&folder, &folder.join("../m-elsewhere/Invoice.docx")),
        None
    );
    assert_eq!(held_open_at(&folder, &folder), None, "the folder itself");
}

/// Claude Code works in the resolved folder, so the agent's paths are physical while the
/// matter may have been picked through a symlink, and the other way round.
#[cfg(unix)]
#[test]
fn a_symlinked_folder_or_file_resolves_to_the_document() {
    let dir = TestDir::new("held-at-symlink");
    let folder = matter(&dir);
    let link = dir.path().join("link");
    support::symlink(&folder, &link);
    let invoice = Some(path("Invoice.docx"));
    let found = |root: &Path, agent: &Path| held_open_at(root, agent).map(|(path, _)| path);
    assert_eq!(found(&link, &folder.join("Invoice.docx")), invoice);
    assert_eq!(found(&folder, &link.join("Invoice.docx")), invoice);
    support::symlink(folder.join("Invoice.docx"), folder.join("alias.docx"));
    assert_eq!(found(&folder, &folder.join("alias.docx")), invoice);
    support::symlink(
        dir.path().join("m-elsewhere/Invoice.docx"),
        folder.join("away.docx"),
    );
    assert_eq!(
        found(&folder, &folder.join("away.docx")),
        None,
        "a link out of the folder"
    );
}

/// macOS and Windows: a folder spelled in another case is the same folder.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn another_case_spelling_of_the_folder_is_the_folder() {
    let dir = TestDir::new("held-at-case");
    let folder = matter(&dir);
    let upper = dir.path().join("M").join("Invoice.docx");
    assert_eq!(
        held_open_at(&folder, &upper).map(|(path, _)| path),
        Some(path("Invoice.docx"))
    );
}

/// A folder that does not resolve is compared as text: inside without `..`, else outside.
#[test]
fn an_unresolved_folder_is_compared_as_text() {
    let dir = TestDir::new("held-at-text");
    let folder = dir.path().join("gone");
    assert_eq!(
        matter_path_in(&folder, &folder.join("sub/a.docx")),
        Some(path("sub/a.docx"))
    );
    assert_eq!(
        matter_path_in(&folder, Path::new("a.docx")),
        Some(path("a.docx"))
    );
    assert_eq!(matter_path_in(&folder, &folder.join("../a.docx")), None);
    assert_eq!(matter_path_in(&folder, &dir.path().join("a.docx")), None);
}
