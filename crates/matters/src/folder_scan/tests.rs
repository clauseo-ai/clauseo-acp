//! The folder scan: its totals are the import's, cancel stops it, and it reads no cloud-only
//! file nor downloads a folder (the `no_download` fault hooks).

use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_protocol::{CheckpointId, MatterId, Timestamp};

use super::{FolderScan, FolderTotals, scan_folder};
use crate::no_download::fault;
use crate::test_dir::TestDir;
use crate::types::{Imported, Limits, Marker, NewMatter, Retention, Settings};
use crate::{Error, Matter, PlaceholderProbe, write};

/// Says cloud-only for every file named `cloud*`.
struct CloudNamed;

impl PlaceholderProbe for CloudNamed {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("cloud"))
    }
}

struct Fixture {
    _dir: TestDir,
    folder: PathBuf,
    settings: Settings,
}

impl Fixture {
    /// A folder holding `files`, with a size cap of 8 bytes.
    fn new(name: &str, files: &[(&str, &[u8])]) -> Self {
        let dir = TestDir::new(name);
        let folder = dir.path().join("m");
        write::create_dir_all(&folder).unwrap();
        for (name, bytes) in files {
            let file = folder.join(name);
            write::create_dir_all(file.parent().unwrap()).unwrap();
            write::create_new(&file).unwrap().write_all(bytes).unwrap();
        }
        let settings = Settings {
            matters_root: dir.path().join("root"),
            refused_trees: Vec::new(),
            limits: Limits {
                file_cap_bytes: 8,
                ..Limits::DEFAULT
            },
            retention: Retention::KeepAll,
            probe: Arc::new(CloudNamed),
        };
        Self {
            _dir: dir,
            folder,
            settings,
        }
    }

    fn scan(&self, cancel: &AtomicBool) -> (Result<FolderScan, Error>, Vec<Step>) {
        let mut seen = Vec::new();
        let result = scan_folder(
            &self.folder,
            &[],
            &self.settings,
            &mut |step| {
                seen.push((
                    step.files,
                    step.bytes,
                    step.cloud_only,
                    step.now_reading.as_str().to_owned(),
                ));
            },
            cancel,
        );
        (result, seen)
    }
}

/// One progress step: files, bytes, cloud-only, the file reached.
type Step = (u64, u64, u64, String);

/// Files of every kind the walk tells apart: to version, cloud-only, over the cap, excluded.
const FILES: &[(&str, &[u8])] = &[
    ("a.txt", b"one"),
    ("sub/b.docx", b"twelve"),
    ("sub/deep/c.pdf", b"x"),
    ("cloud-d.docx", b"in the cloud"),
    ("sub/cloud-e.txt", b"ee"),
    ("big.bin", b"more than eight"),
    ("~$a.docx", b"lock"),
    (".DS_Store", b"junk"),
];

#[test]
fn totals_are_the_import_summary_s() {
    let fx = Fixture::new("folder-scan-totals", FILES);
    let (result, seen) = fx.scan(&AtomicBool::new(false));
    let Ok(FolderScan::Done(totals)) = result else {
        panic!("{result:?}");
    };

    let new = NewMatter {
        id: "01910000-0000-7000-8000-000000000001"
            .parse::<MatterId>()
            .unwrap(),
        folder: fx.folder.clone(),
        created_at: Timestamp::new(1_700_000_000_000).unwrap(),
        marker: Marker::None,
        checkpoint: "01900000-0000-7000-8000-000000000001"
            .parse::<CheckpointId>()
            .unwrap(),
    };
    let imported =
        Matter::import(fx.settings, new, &[], &mut |_| {}, &AtomicBool::new(false)).unwrap();
    let Imported::Done { summary, .. } = imported else {
        panic!("the import was cancelled");
    };
    assert_eq!(
        totals,
        FolderTotals {
            files: 3,
            bytes: 10,
            cloud_only: 2
        }
    );
    assert_eq!(totals.files, u64::from(summary.files));
    assert_eq!(totals.bytes, summary.bytes.get());
    assert_eq!(totals.cloud_only, u64::from(summary.cloud_only));
    // One step per file reached (excluded ones are never reached), the last at the totals.
    assert_eq!(seen.len(), 6);
    let last = seen.last().unwrap();
    assert_eq!((last.0, last.1, last.2), (3, 10, 2));
    let mut reached: Vec<&str> = seen.iter().map(|step| step.3.as_str()).collect();
    reached.sort_unstable();
    assert_eq!(
        reached,
        [
            "a.txt",
            "big.bin",
            "cloud-d.docx",
            "sub/b.docx",
            "sub/cloud-e.txt",
            "sub/deep/c.pdf"
        ]
    );
}

#[test]
fn cancel_before_the_first_file_stops_it() {
    let fx = Fixture::new("folder-scan-cancel-first", FILES);
    let (result, seen) = fx.scan(&AtomicBool::new(true));
    assert_eq!(result.unwrap(), FolderScan::Cancelled);
    assert_eq!(seen, []);
}

#[test]
fn cancel_mid_walk_stops_it_at_the_next_file() {
    let fx = Fixture::new("folder-scan-cancel-mid", FILES);
    let cancel = AtomicBool::new(false);
    let mut steps = 0;
    let result = scan_folder(
        &fx.folder,
        &[],
        &fx.settings,
        &mut |_| {
            steps += 1;
            cancel.store(true, Ordering::Relaxed);
        },
        &cancel,
    );
    assert_eq!(result.unwrap(), FolderScan::Cancelled);
    assert_eq!(steps, 1);
}

#[test]
fn it_reads_no_cloud_only_file_and_keeps_an_undownloaded_folder_so() {
    let fx = Fixture::new(
        "folder-scan-no-download",
        &[
            ("a.txt", b"one"),
            ("cloud-b.txt", b"two"),
            ("evicted/c.txt", b"three"),
        ],
    );
    // A read of the cloud-only file, or a listing of the folder, fails as on a Mac under the
    // guard, and is counted.
    fault::fail(&fx.folder.join("cloud-b.txt"), 0, ErrorKind::Deadlock);
    fault::fail(&fx.folder.join("evicted"), 0, ErrorKind::Deadlock);

    let (result, _) = fx.scan(&AtomicBool::new(false));

    assert_eq!(
        result.unwrap(),
        FolderScan::Done(FolderTotals {
            files: 1,
            bytes: 3,
            cloud_only: 1
        })
    );
    assert_eq!(
        fault::failed(&fx.folder.join("cloud-b.txt")),
        0,
        "never read"
    );
    // The folder not downloaded was tried once, and the scan went on without it.
    assert_eq!(fault::failed(&fx.folder.join("evicted")), 1);
    #[cfg(target_os = "macos")]
    assert!(
        fault::take_policies().iter().all(
            |&policy| policy == crate::no_download::macos::IOPOL_MATERIALIZE_DATALESS_FILES_OFF
        ),
        "every listing ran with downloads off"
    );
    fault::clear();
}

#[test]
fn it_refuses_what_import_refuses() {
    let fx = Fixture::new("folder-scan-refused", &[("a.txt", b"one")]);
    let refused = [fx.folder.clone()];
    let result = scan_folder(
        &fx.folder,
        &refused,
        &fx.settings,
        &mut |_| panic!("no walk"),
        &AtomicBool::new(false),
    );
    assert!(
        matches!(result, Err(Error::FolderRefused { .. })),
        "{result:?}"
    );
}
