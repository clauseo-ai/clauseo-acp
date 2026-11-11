//! The hard rule of [`step`]: undo never deletes a file that was in the folder before the step,
//! versioned or skipped for any reason, and never writes over a skipped one; a file the step
//! truly added is still deleted.

use std::collections::{BTreeSet, HashSet};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_protocol::{CheckpointId, CheckpointKind, SkippedFileReason, Timestamp};

use super::step;
use crate::checkpoint::{Core, Spec};
use crate::history::{GixStore, HistoryStore};
use crate::no_download::fault;
use crate::test_dir::TestDir;
use crate::types::{Limits, Retention, Settings, TimelineEntry};
use crate::writeback::Op;
use crate::{PlaceholderProbe, write};

/// The size cap of the first checkpoint: `over-size-cap` files are 20 bytes.
const CAP: u64 = 10;

/// Cloud-only for the paths marked.
#[derive(Default)]
struct Probe {
    marked: Arc<Mutex<HashSet<PathBuf>>>,
}

impl PlaceholderProbe for Probe {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        self.marked.lock().unwrap().contains(path)
    }
}

fn settings(root: &Path, probe: &Arc<Probe>, cap: u64) -> Settings {
    Settings {
        matters_root: root.to_path_buf(),
        refused_trees: Vec::new(),
        limits: Limits {
            file_cap_bytes: cap,
            import_warn_bytes: u64::MAX,
        },
        retention: Retention::KeepAll,
        probe: Arc::clone(probe) as Arc<dyn PlaceholderProbe + Send + Sync>,
    }
}

fn put(folder: &Path, path: &str, bytes: &[u8]) {
    let abs = folder.join(path);
    write::replace(&abs, &mut |file| {
        file.write_all(bytes).unwrap();
        Ok(())
    })
    .unwrap();
}

fn cid(n: u64) -> CheckpointId {
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

fn spec(n: u64) -> Spec<'static> {
    Spec {
        entry: TimelineEntry {
            id: cid(n),
            kind: if n == 1 {
                CheckpointKind::BeforeTurn
            } else {
                CheckpointKind::AfterTurn
            },
            thread: None,
            turn: None,
        },
        at: Timestamp::new(1_700_000_000_000 + i64::try_from(n).unwrap() * 1000).unwrap(),
        tool_paths: None,
    }
}

/// The files not versioned for `reason` before the step, with their contents: `<reason>-same`
/// is read unchanged by the step, `<reason>-changed` is rewritten; for a folder that did not
/// list, the files in it. Every reason is here; a new one fails to build until it is added.
fn skipped_files(reason: SkippedFileReason) -> Vec<(String, Vec<u8>)> {
    let (name, bytes) = match reason {
        SkippedFileReason::CloudOnly => ("cloud", b"in the cloud".to_vec()),
        SkippedFileReason::OverSizeCap => ("big", vec![7_u8; 20]),
        SkippedFileReason::UnreadFolder => {
            return vec![
                ("unread/same.txt".to_owned(), b"unread".to_vec()),
                ("unread/changed.txt".to_owned(), b"unread".to_vec()),
            ];
        }
        SkippedFileReason::Unknown => panic!("the walk never skips a file for an unknown reason"),
    };
    vec![
        (format!("{name}-same.txt"), bytes.clone()),
        (format!("{name}-changed.txt"), bytes),
    ]
}

const REASONS: [SkippedFileReason; 3] = [
    SkippedFileReason::CloudOnly,
    SkippedFileReason::OverSizeCap,
    SkippedFileReason::UnreadFolder,
];

#[test]
fn undo_never_deletes_or_overwrites_a_file_that_was_there_before_the_step() {
    let dir = TestDir::new("undo-hard-rule");
    let folder = dir.path().join("matter");
    write::create_dir_all(&folder).unwrap();
    let root = dir.path().join("root");
    let probe = Arc::new(Probe::default());
    let mut core =
        Core::load(GixStore::create(&root.join("m/history.git"), &folder).unwrap()).unwrap();

    // Before the step: two read files and, for every reason, two skipped ones.
    put(&folder, "read.txt", b"read v1");
    put(&folder, "gone.txt", b"gone");
    let mut skipped = BTreeSet::new();
    write::create_dir_all(&folder.join("unread")).unwrap();
    for reason in REASONS {
        for (name, bytes) in skipped_files(reason) {
            put(&folder, &name, &bytes);
            if reason == SkippedFileReason::CloudOnly {
                probe.marked.lock().unwrap().insert(folder.join(&name));
            }
            skipped.insert(name);
        }
    }
    // `unread/` does not list at the step's start (permission denied), and does after.
    fault::clear();
    fault::fail(&folder.join("unread"), 0, io::ErrorKind::PermissionDenied);
    let first = core.take(&folder, &settings(&root, &probe, CAP), &spec(1));
    fault::clear();
    first.unwrap();
    let mut there: BTreeSet<String> = skipped.clone();
    there.extend(["read.txt".to_owned(), "gone.txt".to_owned()]);

    // The step: every skipped file is downloaded (and the cap raised, so the big ones read);
    // the `-changed` ones get the bytes of the deleted `gone.txt`, which would make them a
    // rename's new name; one file is modified and one truly added.
    probe.marked.lock().unwrap().clear();
    for name in skipped.iter().filter(|name| name.contains("-changed")) {
        put(&folder, name, b"gone");
    }
    put(&folder, "read.txt", b"read v2");
    write::remove_file(&folder.join("gone.txt")).unwrap();
    put(&folder, "new.txt", b"made by the agent");
    let wide = settings(&root, &probe, 1_000);
    core.take(&folder, &wide, &spec(2)).unwrap();

    let steps = step(&core, &folder, &wide, cid(1), cid(2)).unwrap();
    for step in &steps {
        for op in &step.ops {
            match op {
                Op::Delete(path) => assert!(
                    !there.contains(path.as_str()),
                    "undo would delete {path}, which was there before the step"
                ),
                Op::Write(path, _) => assert!(
                    !skipped.contains(path.as_str()),
                    "undo would write over {path}, which had no earlier copy"
                ),
            }
        }
    }
    // The `-same` files are baselines, no change at all; the `-changed` ones, and every file of
    // the folder that did not list (nothing known of them, so both added), are kept. A file the
    // step added may have come out of that folder: `new.txt` is kept too.
    let names: Vec<(&str, bool)> = steps
        .iter()
        .map(|step| (step.path.as_str(), step.unversioned.is_some()))
        .collect();
    assert_eq!(
        names,
        [
            ("big-changed.txt", true),
            ("cloud-changed.txt", true),
            ("gone.txt", false),
            ("new.txt", true),
            ("read.txt", false),
            ("unread/changed.txt", true),
            ("unread/same.txt", true),
        ]
    );
    for step in steps.iter().filter(|step| step.unversioned.is_some()) {
        assert_eq!(step.ops.len(), 0, "{} is kept as it is", step.path);
    }
    let new = steps
        .iter()
        .find(|step| step.path.as_str() == "new.txt")
        .unwrap();
    assert_eq!(new.unversioned, Some(SkippedFileReason::UnreadFolder));
}

/// The guard against a path the step's first checkpoint holds: a deletion of one is kept, by its
/// exact name; a case-only rename's new name is another name, left to the respelled check.
#[test]
fn a_deletion_of_a_path_the_first_tree_holds_is_kept() {
    use std::collections::BTreeMap;

    use clauseo_protocol::MatterPath;

    use super::kept_because;
    use crate::checkpoint::SizedFile;
    use crate::history::Oid;
    use crate::skip_record::SkipRecord;

    let memo = |text: &str| MatterPath::new(text).unwrap();
    let before = [SizedFile {
        path: memo("memo.txt"),
        blob: Oid([1; 20]),
        size: 4,
    }];
    let start = SkipRecord::default();
    let there = BTreeMap::new();
    let delete = |text: &str| [Op::Delete(memo(text))];
    assert_eq!(
        kept_because(&delete("memo.txt"), &there, Some(&start), &before),
        Some(SkippedFileReason::Unknown)
    );
    assert_eq!(
        kept_because(&delete("Memo.txt"), &there, Some(&start), &before),
        None
    );
    assert_eq!(
        kept_because(&delete("new.txt"), &there, Some(&start), &before),
        None
    );
    assert_eq!(
        kept_because(&delete("new.txt"), &there, None, &before),
        Some(SkippedFileReason::Unknown)
    );
    // A folder did not list at the start.
    let gap = SkipRecord {
        unread: vec!["Sub".to_owned()],
        ..SkipRecord::default()
    };
    assert_eq!(
        kept_because(&delete("new.txt"), &there, Some(&gap), &before),
        Some(SkippedFileReason::UnreadFolder)
    );
}
