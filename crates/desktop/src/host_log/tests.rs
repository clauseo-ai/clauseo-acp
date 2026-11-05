//! The host's log file as the desktop starts it: the folder it opens under the data folder, the
//! event it keeps, and a data folder that cannot resolve. `install` sets the process's global
//! subscriber, so only one test here installs a log (nextest runs each test in its own
//! process).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_settings::{Env, KnownFolder, PlatformDirs};

use super::*;
use crate::test_support::Scratch;

/// The folders of a home at `root`; the data folder comes from [`Vars`].
struct Dirs(PathBuf);

impl PlatformDirs for Dirs {
    fn home(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }

    fn known_folder(&self, _folder: KnownFolder) -> Option<PathBuf> {
        Some(self.0.join("known"))
    }
}

/// `CLAUSEO_DATA_DIR` set to `data`, and no other variable.
struct Vars(PathBuf);

impl Env for Vars {
    fn var(&self, name: &str) -> Option<OsString> {
        (name == "CLAUSEO_DATA_DIR").then(|| self.0.clone().into_os_string())
    }
}

/// The log's text, read from `host.log` in the data folder's `logs`.
fn log_text(data: &Path) -> String {
    std::fs::read_to_string(data.join("logs").join("host.log")).unwrap()
}

#[test]
fn the_event_is_in_host_log_once_flushed() {
    let scratch = Scratch::folder("host-log", &[]);
    let data = scratch.path().join("data");
    let log = install(&Dirs(scratch.path().to_owned()), &Vars(data.clone()));
    assert!(log.0.is_some(), "the log opens under the data folder");
    tracing::warn!("the desktop's host log test");
    log.flush();
    let text = log_text(&data);
    assert!(text.contains("the desktop's host log test"), "{text}");
    assert!(
        !text.contains(&scratch.path().display().to_string()),
        "no path: {text}"
    );
}

#[test]
fn a_data_folder_that_cannot_resolve_holds_no_log() {
    let scratch = Scratch::folder("host-log-relative", &[]);
    // A relative CLAUSEO_DATA_DIR is refused by `Locations::resolve`: no log, no error.
    let log = install(
        &Dirs(scratch.path().to_owned()),
        &Vars(PathBuf::from("relative-data")),
    );
    assert!(log.0.is_none());
    log.flush();
    assert!(!scratch.path().join("relative-data").exists());
}
