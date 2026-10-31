//! What the store refuses or reports: a commit that is not stored, a commit whose objects
//! vanished, a wrong expected timeline, bad tree names, entries that are not files, a subtree
//! that does not match its id, and a stat cache git wrote during a merge.

use clauseo_protocol::MatterPath;

use super::{file, git_id, new_store, tree_entry, write_file};
use crate::Error;
use crate::history::{HistoryStore, Oid, TIMELINE_REF, TreeFile};
use crate::no_download::NoDownload;
use crate::stat::{CacheEntry, EntryState, FileTime, Stat, StatCache};
use crate::test_dir::TestDir;
use crate::write;
use crate::{OsProbe, PlaceholderProbe};

/// A guard for a test's direct reads, with the OS probe.
fn no_download() -> NoDownload<'static> {
    const PROBE: &dyn PlaceholderProbe = &OsProbe;
    NoDownload::begin(PROBE)
}

/// The id of a gix object id.
fn oid(id: gix::ObjectId) -> Oid {
    Oid(id.as_slice().try_into().unwrap())
}

/// The loose object file of `id` under `git_dir`.
fn object_path(git_dir: &std::path::Path, id: Oid) -> std::path::PathBuf {
    let hex = id.to_hex();
    let (folder, name) = hex.split_at(2);
    git_dir.join("objects").join(folder).join(name)
}

/// A commit that was never stored is not published: the timeline would name nothing.
#[test]
fn publish_refuses_a_commit_that_is_not_stored() {
    let dir = TestDir::new("history-publish-missing");
    let (git_dir, mut store) = new_store(&dir);
    let published = store.publish(Oid([7; 20]), None, &mut StatCache::default());
    assert!(
        matches!(&published, Err(Error::History { op: "publish", .. })),
        "{published:?}"
    );
    assert!(!git_dir.join(TIMELINE_REF).exists());
}

/// A checkpoint is not published when an object written for it has vanished before the
/// publish: syncing it fails, so the timeline never names a commit with a missing file.
#[test]
fn publish_refuses_when_an_object_written_for_it_has_vanished() {
    let dir = TestDir::new("history-publish-vanished");
    let (git_dir, mut store) = new_store(&dir);
    let path = dir.path().join("lease.txt");
    write_file(&path, b"the signed lease\n");
    let (blob, _) = store
        .store_file(&path, 17, u64::MAX, &no_download())
        .unwrap();
    let tree = store
        .write_tree(&[TreeFile {
            path: "lease.txt".to_owned(),
            blob,
        }])
        .unwrap();
    let commit = store.write_commit(tree, None, "First", 1).unwrap();
    write::remove_file(&object_path(&git_dir, blob)).unwrap();

    let published = store.publish(commit, None, &mut StatCache::default());
    assert!(
        matches!(&published, Err(Error::Io { op: "open", .. })),
        "{published:?}"
    );
    assert!(!git_dir.join(TIMELINE_REF).exists());
}

/// A publish that finds the timeline elsewhere names, in its error, the commit the timeline is
/// at and the one expected.
#[test]
fn a_wrong_expected_timeline_names_both_commits() {
    let dir = TestDir::new("history-publish-names");
    let (_git_dir, mut store) = new_store(&dir);
    let mut cache = StatCache::default();
    let tree = store.write_tree(&[]).unwrap();
    let first = store.write_commit(tree, None, "First", 1).unwrap();
    store.publish(first, None, &mut cache).unwrap();
    let second = store.write_commit(tree, Some(first), "Second", 2).unwrap();

    let message = store
        .publish(second, Some(second), &mut cache)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains(&first.to_hex()) && message.contains(&second.to_hex()),
        "{message}"
    );
}

/// The store's debug output, as logs show it, names its git dir.
#[test]
fn the_store_debug_output_names_its_git_dir() {
    let dir = TestDir::new("history-debug");
    let (git_dir, store) = new_store(&dir);
    let shown = format!("{store:?}");
    assert!(shown.starts_with("GixStore {"), "{shown}");
    assert!(shown.contains(&format!("{git_dir:?}")), "{shown}");
}

/// A path with an empty segment or a NUL byte in a name has no tree entry git could read, so
/// writing its tree is an error.
#[test]
fn write_tree_refuses_an_empty_name_or_a_nul_byte() {
    for path in ["a//b.txt", "folder/a\0b.txt", "a\0b.txt"] {
        let dir = TestDir::new("history-tree-bad-name");
        let (_git_dir, mut store) = new_store(&dir);
        let written = store.write_tree(&[file(path, 1)]);
        assert!(
            matches!(
                &written,
                Err(Error::History {
                    op: "write tree",
                    ..
                })
            ),
            "{path:?}: {written:?}"
        );
    }
}

/// A symlink or a submodule in a tree is not a file: `file_at` finds nothing there, and finds
/// the regular file beside them.
#[test]
fn file_at_finds_no_file_at_a_symlink_or_a_submodule() {
    let dir = TestDir::new("history-file-at-link");
    let (git_dir, store) = new_store(&dir);
    let blob = git_id("blob", b"target\n");
    let mut data = tree_entry("100644", "a.txt", blob);
    data.extend(tree_entry("120000", "link", blob));
    data.extend(tree_entry("160000", "sub", Oid([9; 20])));
    let objects = gix::odb::loose::Store::at(git_dir.join("objects"), gix::hash::Kind::Sha1);
    let tree = oid(gix::objs::Write::write_buf(&objects, gix::objs::Kind::Tree, &data).unwrap());

    assert_eq!(store.file_at(tree, "a.txt").unwrap(), Some(blob));
    assert_eq!(store.file_at(tree, "link").unwrap(), None);
    assert_eq!(store.file_at(tree, "sub").unwrap(), None);
}

/// A subtree whose object does not hash to its id (a well-formed tree of other bytes) makes
/// `changed_files` fail with the read error that names it, not a generic diff error.
#[test]
fn changed_files_reports_a_subtree_that_does_not_match_its_id() {
    let dir = TestDir::new("history-diff-corrupt");
    let (git_dir, mut store) = new_store(&dir);
    let a = store.write_tree(&[file("folder/x.txt", 1)]).unwrap();
    let b = store.write_tree(&[file("folder/x.txt", 2)]).unwrap();
    let subtree = git_id("tree", &tree_entry("100644", "x.txt", Oid([2; 20])));
    let object = object_path(&git_dir, subtree);
    write::remove_file(&object).unwrap();
    let objects = gix::odb::loose::Store::at(git_dir.join("objects"), gix::hash::Kind::Sha1);
    gix::objs::Write::write_buf_with_known_id(
        &objects,
        gix::objs::Kind::Tree,
        &tree_entry("100644", "y.txt", Oid([2; 20])),
        gix::ObjectId::Sha1(subtree.0),
    )
    .unwrap();

    let changed = store.changed_files(a, b);
    let message = match &changed {
        Err(Error::History {
            op: "read object",
            source,
        }) => source.to_string(),
        _ => String::new(),
    };
    assert!(
        message.contains("does not match its content"),
        "{changed:?}"
    );
}

/// A stat cache entry at a merge stage (git's conflict entries) is not one this module wrote:
/// the cache does not read and is rebuilt, while the same entry at stage 0 reads.
#[test]
fn a_stat_cache_with_a_conflict_entry_does_not_read() {
    use gix::index::entry::{Flags, Mode};

    for (stage, reads) in [(0, true), (1, false), (2, false)] {
        let dir = TestDir::new(&format!("history-cache-stage-{stage}"));
        let (git_dir, store) = new_store(&dir);
        let blob = Oid([1; 20]);
        let mut state = gix::index::State::new(gix::hash::Kind::Sha1);
        state.dangerously_push_entry(
            gix::index::entry::Stat::default(),
            gix::ObjectId::Sha1(blob.0),
            Flags::from_bits_retain(stage << 12),
            Mode::FILE,
            gix::bstr::BStr::new("a.txt"),
        );
        let index = git_dir.join("index");
        let file = gix::index::File::from_state(state, &index);
        let options = gix::index::write::Options {
            extensions: gix::index::write::Extensions::None,
            skip_hash: false,
        };
        write::replace(&index, &mut |out| {
            file.write_to(&mut *out, options)
                .map_err(|err| Error::history("write stat cache", err))?;
            Ok(())
        })
        .unwrap();

        let loaded = store.load_cache().unwrap();
        let expected = reads.then(|| StatCache {
            entries: vec![CacheEntry {
                path: MatterPath::new("a.txt").unwrap(),
                stat: Stat {
                    mtime: FileTime { secs: 0, nanos: 0 },
                    ctime: FileTime { secs: 0, nanos: 0 },
                    dev: 0,
                    ino: 0,
                    size: 0,
                },
                state: EntryState::Read(blob),
            }],
            written_at: loaded.as_ref().and_then(|cache| cache.written_at),
        });
        assert_eq!(loaded, expected, "stage {stage}");
    }
}
