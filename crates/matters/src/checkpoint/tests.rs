//! Tests of [`Core`]: the stat cache's time after racy re-reads, a tip without a trailer, a file
//! that grows over the cap while it is read, which skipped files are listed again, and files
//! that vanish while the walk runs.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use clauseo_protocol::{
    ChangeKind, CheckpointId, CheckpointKind, MatterPath, SkippedFileReason, Timestamp,
};

use super::walk_folder::{VANISHED_WAITS, pauses, worth_listing};
use super::{Core, Spec};
use crate::history::{GixStore, HistoryStore};
use crate::stat::{CacheEntry, EntryState, FileTime, Stat, StatCache};
use crate::test_dir::TestDir;
use crate::types::{Checkpoint, CheckpointOutcome, Limits, Retention, Settings, TimelineEntry};
use crate::{PlaceholderProbe, write};

/// Never cloud-only; when `grow` names a file, appends to it once, the first time the walk
/// asks about it (after the walk took its stat). Each file of `vanish` is removed the first
/// time the walk asks about it (once only), so its read finds it gone, when the walk reads it
/// ([`not_vouched`]); the first time the walk asks about one file of `vanish_other`, the other
/// is removed, so its stat finds it gone.
#[derive(Default)]
struct Probe {
    grow: Option<(PathBuf, usize)>,
    grown: AtomicBool,
    vanish: Vec<PathBuf>,
    removed: Mutex<HashSet<PathBuf>>,
    vanish_other: Option<[PathBuf; 2]>,
    other_gone: AtomicBool,
}

impl PlaceholderProbe for Probe {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        if let Some((file, bytes)) = &self.grow
            && file == path
            && !self.grown.swap(true, Ordering::Relaxed)
        {
            write::append(path, &vec![b'x'; *bytes]).unwrap();
        }
        if self.vanish.iter().any(|file| file == path)
            && self.removed.lock().unwrap().insert(path.to_owned())
        {
            write::remove_file(path).unwrap();
        }
        if let Some([a, b]) = &self.vanish_other
            && (path == a || path == b)
            && !self.other_gone.swap(true, Ordering::Relaxed)
        {
            write::remove_file(if path == a { b } else { a }).unwrap();
        }
        false
    }
}

/// A folder and a git dir for one test.
struct Setup {
    _dir: TestDir,
    folder: PathBuf,
    git_dir: PathBuf,
    settings: Settings,
}

impl Setup {
    fn new(name: &str, probe: Probe, file_cap_bytes: u64) -> Self {
        let dir = TestDir::new(name);
        let folder = dir.path().join("matter");
        write::create_dir_all(&folder).unwrap();
        let root = dir.path().join("root");
        let git_dir = root.join("m").join("history.git");
        let settings = Settings {
            matters_root: root,
            refused_trees: Vec::new(),
            limits: Limits {
                file_cap_bytes,
                ..Limits::DEFAULT
            },
            retention: Retention::KeepAll,
            probe: Arc::new(probe),
        };
        Self {
            _dir: dir,
            folder,
            git_dir,
            settings,
        }
    }

    fn put(&self, name: &str, bytes: &[u8]) {
        put_at(&self.folder.join(name), bytes);
    }

    fn create(&self) -> Core {
        Core::load(GixStore::create(&self.git_dir, &self.folder).unwrap()).unwrap()
    }

    fn reopen(&self) -> Core {
        Core::load(GixStore::open(&self.git_dir, &self.folder).unwrap()).unwrap()
    }

    fn take(&self, core: &mut Core, n: u64, kind: CheckpointKind) -> Checkpoint {
        core.take(&self.folder, &self.settings, &spec(n, kind))
            .unwrap()
    }

    /// Walks and commits checkpoint `n`; returns it and the bytes the walk read.
    fn take_counting(&self, core: &mut Core, n: u64) -> (Checkpoint, u64) {
        let mut bytes = 0;
        let walked = core
            .scan_folder(&self.folder, &self.settings, None, &mut |done| {
                bytes = done.bytes;
            })
            .unwrap()
            .unwrap();
        let checkpoint = core
            .commit(walked, &spec(n, CheckpointKind::BeforeTurn))
            .unwrap();
        (checkpoint, bytes)
    }
}

/// Writes `bytes` to `path` by a temp file and a rename, as Word's save swaps a file in.
fn put_at(path: &Path, bytes: &[u8]) {
    write::replace(path, &mut |file| {
        file.write_all(bytes).unwrap();
        Ok(())
    })
    .unwrap();
}

/// Gives the file at `path` an mtime long past and unlike any a cache holds for it, so the next
/// walk reads it: its stat no longer matches the cached one. A file of a Probe's `vanish` goes
/// only when the walk asks about it, and is found gone only if the walk then reads it; the cache
/// vouches for a file whose stat is unchanged and older than the racy window, and writing many
/// files can take a loaded machine longer than that window.
fn not_vouched(path: &Path) {
    write::set_mtime(path, in_2020(60)).unwrap();
}

fn cid(n: u64) -> CheckpointId {
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

fn spec(n: u64, kind: CheckpointKind) -> Spec<'static> {
    Spec {
        entry: TimelineEntry {
            id: cid(n),
            kind,
            thread: None,
            turn: None,
        },
        at: Timestamp::new(1_700_000_000_000 + i64::try_from(n).unwrap() * 1000).unwrap(),
        tool_paths: None,
    }
}

/// 2020-01-01T00:00:00Z plus `secs`.
fn in_2020(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_577_836_800 + secs)
}

#[test]
fn a_racy_reread_writes_the_cache_so_the_next_checkpoint_reads_nothing() {
    let setup = Setup::new("checkpoint-racy-save", Probe::default(), u64::MAX);
    setup.put("doc.txt", &[b'd'; 100]);
    let mut core = setup.create();
    setup.take(&mut core, 1, CheckpointKind::Import);
    // An mtime long past, then a cache whose own time is one second after it: the file is
    // racily clean when the matter opens again.
    write::set_mtime(&setup.folder.join("doc.txt"), in_2020(0)).unwrap();
    let reused = setup.take(&mut core, 2, CheckpointKind::BeforeTurn);
    assert_eq!(reused.outcome, CheckpointOutcome::Reused(cid(1)));
    drop(core);
    write::set_mtime(&setup.git_dir.join("index"), in_2020(1)).unwrap();
    let mut core = setup.reopen();

    let (first, read) = setup.take_counting(&mut core, 3);
    assert_eq!(first.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_eq!(read, 100, "a racily clean file is read again");
    let (second, read) = setup.take_counting(&mut core, 4);
    assert_eq!(second.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_eq!(read, 0, "the cache was written after the racy re-read");
}

#[test]
fn a_tip_without_a_trailer_with_the_same_tree_is_built_on() {
    let setup = Setup::new("checkpoint-tip-same-tree", Probe::default(), u64::MAX);
    setup.put("a.txt", b"one");
    let mut core = setup.create();
    setup.take(&mut core, 1, CheckpointKind::Import);
    let (import, tree) = core.timeline.tip().unwrap();
    // A later version's commit without a trailer, on the same tree.
    let squash = core
        .history
        .write_commit(
            tree,
            Some(import),
            "Kept by a later version\n",
            1_700_000_100,
        )
        .unwrap();
    let mut cache = core.cache.clone();
    core.history
        .publish(squash, Some(import), &mut cache)
        .unwrap();
    drop(core);

    let mut core = setup.reopen();
    assert_eq!(core.timeline.entries.len(), 1);
    assert_eq!(core.timeline.tip(), Some((squash, tree)));
    let reused = setup.take(&mut core, 2, CheckpointKind::BeforeTurn);
    assert_eq!(reused.outcome, CheckpointOutcome::Reused(cid(1)));

    setup.put("a.txt", b"two");
    let created = setup.take(&mut core, 3, CheckpointKind::BeforeTurn);
    assert_eq!(created.outcome, CheckpointOutcome::Created(cid(3)));
    let commits: Vec<_> = core
        .history
        .timeline()
        .unwrap()
        .iter()
        .map(|record| record.id)
        .collect();
    assert_eq!(commits.len(), 3);
    assert_eq!(commits[..2], [import, squash]);
    assert_eq!(core.timeline.entries.len(), 2);
}

#[test]
fn a_tip_without_a_trailer_with_another_tree_is_never_reused() {
    let setup = Setup::new("checkpoint-tip-other-tree", Probe::default(), u64::MAX);
    setup.put("a.txt", b"one");
    let mut core = setup.create();
    setup.take(&mut core, 1, CheckpointKind::Import);
    let (import, _) = core.timeline.tip().unwrap();
    let empty = core.history.write_tree(&[]).unwrap();
    let squash = core
        .history
        .write_commit(
            empty,
            Some(import),
            "Emptied by a later version\n",
            1_700_000_100,
        )
        .unwrap();
    core.history
        .publish(squash, Some(import), &mut StatCache::default())
        .unwrap();
    drop(core);

    // The folder equals the last checkpoint but not the tip: a new checkpoint on the tip.
    let mut core = setup.reopen();
    let created = setup.take(&mut core, 2, CheckpointKind::BeforeTurn);
    assert_eq!(created.outcome, CheckpointOutcome::Created(cid(2)));
    assert_eq!(created.changes.len(), 1);
    assert_eq!(created.changes[0].path().as_str(), "a.txt");
    assert!(matches!(created.changes[0].change(), ChangeKind::Added));
    let records = core.history.timeline().unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(records[1].id, squash);
}

#[test]
fn a_file_grown_over_the_cap_while_read_is_skipped() {
    let mut setup = Setup::new("checkpoint-grew-at-read", Probe::default(), 10);
    let big = setup.folder.join("big.pdf");
    setup.settings.probe = Arc::new(Probe {
        grow: Some((big, 20)),
        ..Probe::default()
    });
    setup.put("big.pdf", b"small");
    setup.put("a.txt", b"one");
    let mut core = setup.create();

    let import = setup.take(&mut core, 1, CheckpointKind::Import);
    assert_eq!(import.outcome, CheckpointOutcome::Created(cid(1)));
    assert_eq!(import.skipped.len(), 1);
    assert_eq!(import.skipped[0].path.as_str(), "big.pdf");
    assert_eq!(import.skipped[0].reason, SkippedFileReason::OverSizeCap);
    let in_tree: Vec<_> = core
        .cache
        .entries
        .iter()
        .filter(|entry| entry.state.blob().is_some())
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(in_tree, ["a.txt"]);

    // The stat taken after the failed read is the one cached: not listed again.
    let next = setup.take(&mut core, 2, CheckpointKind::BeforeTurn);
    assert_eq!(next.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_eq!(next.skipped, []);
}

#[test]
fn a_skipped_file_is_listed_again_only_for_size_or_mtime() {
    let stat = Stat {
        mtime: FileTime {
            secs: 100,
            nanos: 5,
        },
        ctime: FileTime {
            secs: 100,
            nanos: 5,
        },
        dev: 1,
        ino: 2,
        size: 10,
    };
    let old = CacheEntry {
        path: MatterPath::new("a.pdf").unwrap(),
        stat,
        state: EntryState::Skipped,
    };
    let touched = Stat {
        ctime: FileTime {
            secs: 200,
            nanos: 0,
        },
        dev: 3,
        ino: 4,
        ..stat
    };
    assert!(!worth_listing(Some(&old), &touched));
    assert!(!worth_listing(Some(&old), &stat));
    assert!(worth_listing(Some(&old), &Stat { size: 11, ..stat }));
    let later = FileTime {
        secs: 100,
        nanos: 6,
    };
    assert!(worth_listing(
        Some(&old),
        &Stat {
            mtime: later,
            ..stat
        }
    ));
    assert!(worth_listing(None, &stat));
}

/// The changes of checkpoint `checkpoint` as (path, kind) pairs.
fn kinds(checkpoint: &Checkpoint) -> Vec<(String, ChangeKind)> {
    checkpoint
        .changes
        .iter()
        .map(|change| (change.path().as_str().to_owned(), change.change().clone()))
        .collect()
}

/// Word saves by swapping files with renames, so a document can be missing for a moment: one
/// gone when the walk reads it and back, with new content, before the walk's first wait ends is
/// changed, not deleted (clauseo-ai/clauseo-acp-private#224).
#[test]
fn a_file_swapped_in_while_the_walk_reads_it_is_changed_not_deleted() {
    let mut setup = Setup::new("checkpoint-vanish-read-back", Probe::default(), u64::MAX);
    setup.put("a.txt", b"a");
    setup.put("doc.docx", b"one");
    let mut core = setup.create();
    setup.take(&mut core, 1, CheckpointKind::Import);
    let doc = setup.folder.join("doc.docx");
    not_vouched(&doc);
    setup.settings.probe = Arc::new(Probe {
        vanish: vec![doc.clone()],
        ..Probe::default()
    });
    let back = doc;
    pauses::on_pause(move |_| {
        if !back.exists() {
            put_at(&back, b"two");
        }
    });

    let checkpoint = setup.take(&mut core, 2, CheckpointKind::BeforeTurn);
    assert_eq!(
        kinds(&checkpoint),
        [("doc.docx".to_owned(), ChangeKind::Modified)]
    );
    assert_eq!(checkpoint.outcome, CheckpointOutcome::Created(cid(2)));
    assert_eq!(pauses::take().0, 1, "one wait, then it was back");
}

/// The same when the file is gone at its stat, before the walk reads it.
#[test]
fn a_file_swapped_in_while_the_walk_stats_it_is_changed_not_deleted() {
    let mut setup = Setup::new("checkpoint-vanish-stat-back", Probe::default(), u64::MAX);
    setup.put("a.txt", b"one");
    setup.put("b.txt", b"one");
    let mut core = setup.create();
    setup.take(&mut core, 1, CheckpointKind::Import);
    let (a, b) = (setup.folder.join("a.txt"), setup.folder.join("b.txt"));
    setup.settings.probe = Arc::new(Probe {
        vanish_other: Some([a.clone(), b.clone()]),
        ..Probe::default()
    });
    pauses::on_pause(move |_| {
        for file in [&a, &b] {
            if !file.exists() {
                put_at(file, b"two");
            }
        }
    });

    let checkpoint = setup.take(&mut core, 2, CheckpointKind::BeforeTurn);
    let changes = kinds(&checkpoint);
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].1, ChangeKind::Modified, "{changes:?}");
    assert_eq!(pauses::take().0, 1);
}

/// A file gone for good is deleted, after every wait.
#[test]
fn a_file_gone_for_good_is_deleted_after_the_waits() {
    let mut setup = Setup::new("checkpoint-vanish-gone", Probe::default(), u64::MAX);
    setup.put("a.txt", b"a");
    setup.put("doc.docx", b"one");
    let mut core = setup.create();
    setup.take(&mut core, 1, CheckpointKind::Import);
    not_vouched(&setup.folder.join("doc.docx"));
    setup.settings.probe = Arc::new(Probe {
        vanish: vec![setup.folder.join("doc.docx")],
        ..Probe::default()
    });

    let checkpoint = setup.take(&mut core, 2, CheckpointKind::BeforeTurn);
    let total: Duration = VANISHED_WAITS.iter().sum();
    assert_eq!(pauses::take(), (VANISHED_WAITS.len(), total));
    assert_eq!(
        kinds(&checkpoint),
        [("doc.docx".to_owned(), ChangeKind::Deleted)]
    );
}

/// Many files gone for good cost the walk's waits once, not once per file: each wait is for
/// every file still gone.
#[test]
fn many_files_gone_wait_no_longer_than_one() {
    let mut setup = Setup::new("checkpoint-vanish-many", Probe::default(), u64::MAX);
    let names: Vec<String> = (0..200).map(|n| format!("f{n:03}.txt")).collect();
    for name in &names {
        setup.put(name, name.as_bytes());
    }
    // Every file older than the cache by far, so the import's cache vouches for each one
    // whatever the clock: as when writing 200 files took the test longer than the racy window.
    for name in &names {
        write::set_mtime(&setup.folder.join(name), in_2020(0)).unwrap();
    }
    let mut core = setup.create();
    setup.take(&mut core, 1, CheckpointKind::Import);
    for name in &names {
        not_vouched(&setup.folder.join(name));
    }
    setup.settings.probe = Arc::new(Probe {
        vanish: names.iter().map(|name| setup.folder.join(name)).collect(),
        ..Probe::default()
    });

    let checkpoint = setup.take(&mut core, 2, CheckpointKind::BeforeTurn);
    let (waits, waited) = pauses::take();
    assert_eq!(waits, VANISHED_WAITS.len());
    assert_eq!(waited, VANISHED_WAITS.iter().sum::<Duration>());
    assert!(waited <= Duration::from_millis(300));
    let changes = kinds(&checkpoint);
    assert_eq!(changes.len(), 200, "{changes:?}");
    assert!(
        changes.iter().all(|(_, kind)| *kind == ChangeKind::Deleted),
        "{changes:?}"
    );
}
