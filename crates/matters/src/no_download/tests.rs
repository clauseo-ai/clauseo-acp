//! The guard and the "not downloaded" rule. The guard's policy tests run on macOS only (CI's
//! macOS runner); the not-downloaded tests (`deadlock.rs`) run everywhere, through the fault
//! hooks of `fault.rs`.

mod deadlock;
#[cfg(target_os = "macos")]
mod policy;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use clauseo_protocol::{CheckpointId, MatterId, MatterPath, ThreadId, Timestamp, TurnId};

use super::not_downloaded;
use crate::test_dir::TestDir;
use crate::types::{Imported, Limits, Marker, NewMatter, Retention, Settings, TurnMeta};
use crate::{Matter, PlaceholderProbe, write};

/// Checkpoint `n`.
fn cid(n: u64) -> CheckpointId {
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

fn at(n: u64) -> Timestamp {
    Timestamp::new(1_700_000_000_000 + i64::try_from(n).unwrap() * 1000).unwrap()
}

fn thread() -> ThreadId {
    "01920000-0000-7000-8000-000000000001".parse().unwrap()
}

fn turn(n: u64) -> TurnId {
    format!("01930000-0000-7000-8000-{n:012x}").parse().unwrap()
}

/// The checkpoint `n` of turn `n`.
fn meta(n: u64) -> TurnMeta {
    TurnMeta {
        checkpoint: cid(n),
        thread: thread(),
        turn: turn(n),
        at: at(n),
    }
}

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

/// Writes a new file, making its folders.
fn put(file: &Path, bytes: &[u8]) {
    use std::io::Write as _;
    write::create_dir_all(file.parent().unwrap()).unwrap();
    write::create_new(file).unwrap().write_all(bytes).unwrap();
}

/// A probe that never says cloud-only.
struct Local;

impl PlaceholderProbe for Local {
    fn is_cloud_only(&self, _path: &Path, _metadata: &std::fs::Metadata) -> bool {
        false
    }
}

/// A matter imported from a folder holding `files`, in a test's own folder.
struct Fixture {
    _dir: TestDir,
    folder: PathBuf,
    settings: Settings,
}

impl Fixture {
    fn new(name: &str, files: &[(&str, &[u8])]) -> Self {
        Self::with_probe(name, files, Arc::new(Local))
    }

    fn with_probe(
        name: &str,
        files: &[(&str, &[u8])],
        probe: Arc<dyn PlaceholderProbe + Send + Sync>,
    ) -> Self {
        let dir = TestDir::new(name);
        let folder = dir.path().join("m");
        write::create_dir_all(&folder).unwrap();
        for (name, bytes) in files {
            put(&folder.join(name), bytes);
        }
        let settings = Settings {
            matters_root: dir.path().join("root"),
            limits: Limits::DEFAULT,
            retention: Retention::KeepAll,
            probe,
        };
        Self {
            _dir: dir,
            folder,
            settings,
        }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.folder.join(name)
    }

    /// Imports the folder as matter 1 (root checkpoint 1); returns it and its summary.
    fn import(&self) -> (Matter, clauseo_protocol::ImportSummary) {
        let new = NewMatter {
            id: "01910000-0000-7000-8000-000000000001"
                .parse::<MatterId>()
                .unwrap(),
            folder: self.folder.clone(),
            created_at: at(1),
            marker: Marker::None,
            checkpoint: cid(1),
        };
        let imported = Matter::import(
            self.settings.clone(),
            new,
            &[],
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        let Imported::Done {
            matter, summary, ..
        } = imported
        else {
            panic!("the import was cancelled");
        };
        (*matter, summary)
    }
}

#[test]
fn not_downloaded_is_edeadlk_by_name() {
    assert!(not_downloaded(&io::Error::from(io::ErrorKind::Deadlock)));
    assert!(!not_downloaded(&io::Error::from(io::ErrorKind::WouldBlock)));
    assert!(!not_downloaded(&io::Error::from(
        io::ErrorKind::PermissionDenied
    )));
    // 11 is EDEADLK on macOS but EAGAIN on Linux: the rule follows the OS's meaning.
    let eleven = io::Error::from_raw_os_error(11);
    assert_eq!(not_downloaded(&eleven), cfg!(target_os = "macos"));
}
