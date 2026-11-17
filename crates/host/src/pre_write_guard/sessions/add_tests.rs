//! Adding a session to the ids file: its rename into place is tried again for a moment (a
//! scanner holding the new file, Defender on Windows), then fails with the last try's error.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{RENAME_PAUSE, RENAME_TRIES, add, add_with, file, read};
use crate::write;

/// The conversation's session id, as Codex's thread ids look.
const MAIN: &str = "01a1e4b4-30ac-7f81-b3fe-c7d36d7f97b5";

/// A guard folder whose ids file holds [`MAIN`]. Removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "clauseo-host-sessions-add-{test}-{}",
            std::process::id()
        ));
        write::remove_dir_all(&root).unwrap();
        write::create_dir_all(&root).unwrap();
        add(&root, MAIN).unwrap();
        Self(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}

/// A rename that fails its first `fails` tries (as a scanner holding the new file makes it
/// fail on Windows), then renames; with the pauses `add_with` asked for.
struct Flaky {
    fails: u32,
    tries: Cell<u32>,
    pauses: RefCell<Vec<Duration>>,
}

impl Flaky {
    fn new(fails: u32) -> Self {
        Self {
            fails,
            tries: Cell::new(0),
            pauses: RefCell::new(Vec::new()),
        }
    }

    /// [`add_with`] of `id` in `folder` with this rename and recorded pauses.
    fn add(&self, folder: &Path, id: &str) -> std::io::Result<()> {
        let rename = |temp: &Path, path: &Path| {
            self.tries.set(self.tries.get() + 1);
            if self.tries.get() <= self.fails {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            write::rename_file(temp, path)
        };
        add_with(folder, id, &rename, &|time| {
            self.pauses.borrow_mut().push(time);
        })
    }
}

/// The files in `folder`, by name.
fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A rename that fails, then works within the tries, puts the ids file in place, the pauses
/// between the tries about half a second in all, and no temp file left.
#[test]
fn the_ids_files_rename_is_tried_again() {
    let scratch = Scratch::new("rename-retry");
    let fork = "01a1e4b4-4000-7000-8000-00000000f0f0";
    let flaky = Flaky::new(RENAME_TRIES - 1);
    flaky.add(&scratch.0, fork).unwrap();
    assert_eq!(read(&file(&scratch.0)).unwrap(), [MAIN, fork]);
    assert_eq!(flaky.tries.get(), RENAME_TRIES);
    let pauses = flaky.pauses.borrow();
    assert_eq!(*pauses, vec![RENAME_PAUSE; 4]);
    assert_eq!(pauses.iter().sum::<Duration>(), Duration::from_millis(500));
    assert_eq!(names_in(&scratch.0), ["sessions.json"]);
}

/// A rename that fails every try fails the add with its error after the last one, the ids file
/// as it was and no temp file left.
#[test]
fn the_ids_files_rename_fails_after_its_tries() {
    let scratch = Scratch::new("rename-fails");
    let flaky = Flaky::new(u32::MAX);
    let error = flaky
        .add(&scratch.0, "01a1e4b4-4000-7000-8000-00000000f0f0")
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(flaky.tries.get(), RENAME_TRIES);
    assert_eq!(flaky.pauses.borrow().len(), 4);
    assert_eq!(read(&file(&scratch.0)).unwrap(), [MAIN]);
    assert_eq!(names_in(&scratch.0), ["sessions.json"]);
}
