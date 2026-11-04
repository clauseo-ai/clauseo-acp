//! Windows: write-back on NTFS, where `|` is no file name, names ignore case, and a file held
//! open without delete sharing cannot be deleted.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::MatterPath;

use super::{Names, Op, apply, respelled, writable};
use crate::checkpoint::Core;
use crate::history::{GixStore, HistoryStore, Oid};
use crate::stat::{CacheEntry, EntryState, Stat};
use crate::test_dir::TestDir;
use crate::types::{Limits, Retention, Settings};
use crate::{Error, OsProbe, write};

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

fn put(file: &Path) {
    write::create_dir_all(file.parent().unwrap()).unwrap();
    drop(write::create_new(file).unwrap());
}

fn settings(root: &Path) -> Settings {
    Settings {
        matters_root: root.join("root"),
        refused_trees: Vec::new(),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::new(OsProbe),
    }
}

/// A matter folder and its history, with an empty cache.
struct Matter {
    _dir: TestDir,
    folder: PathBuf,
    core: Core,
}

fn matter(name: &str) -> Matter {
    let dir = TestDir::new(name);
    let folder = dir.path().join("matter");
    write::create_dir_all(&folder).unwrap();
    let git_dir = dir.path().join("root").join("m").join("history.git");
    let core = Core::load(GixStore::create(&git_dir, &folder).unwrap()).unwrap();
    Matter {
        _dir: dir,
        folder,
        core,
    }
}

/// A stat that fails other than "not found" (`a|b.txt`, a name NTFS refuses) is an error, not
/// an absent file: neither the listing nor the write check passes over it.
#[test]
fn a_stat_that_fails_otherwise_than_not_found_is_an_error() {
    let dir = TestDir::new("writeback-win-bad-name");
    let bad = path("a|b.txt");
    let found = Names::default().metadata(dir.path(), &bad);
    assert!(
        matches!(&found, Err(Error::Io { op: "stat", .. })),
        "{found:?}"
    );
    let other = Names::default().other_spelling(dir.path(), &bad);
    assert!(
        matches!(&other, Err(Error::Io { op: "stat", .. })),
        "{other:?}"
    );
    let ok = writable(dir.path(), &settings(dir.path()), &bad);
    assert!(matches!(&ok, Err(Error::Io { op: "stat", .. })), "{ok:?}");
}

/// A path through folders is writable; one through a file is not.
#[test]
fn only_a_path_through_folders_is_writable() {
    let dir = TestDir::new("writeback-win-through");
    put(&dir.path().join("sub").join("a.txt"));
    put(&dir.path().join("f.txt"));
    let settings = settings(dir.path());
    assert!(writable(dir.path(), &settings, &path("sub/a.txt")).unwrap());
    assert!(writable(dir.path(), &settings, &path("sub/new.txt")).unwrap());
    assert!(!writable(dir.path(), &settings, &path("f.txt/a.txt")).unwrap());
}

/// Writing `memo.txt` would replace `Memo.txt`, another file to the history; deleting an
/// unrelated file in the same step does not make that write ours.
#[test]
fn a_write_onto_another_files_other_spelling_clashes() {
    let dir = TestDir::new("writeback-win-respelled");
    put(&dir.path().join("Memo.txt"));
    put(&dir.path().join("x.txt"));
    let write_memo = Op::Write(path("memo.txt"), Oid([0; 20]));
    let delete_x = Op::Delete(path("x.txt"));
    let clashes = respelled(&mut Names::default(), dir.path(), &[&write_memo, &delete_x]).unwrap();
    assert_eq!(clashes, [path("memo.txt")]);
}

/// Undoing a case-only rename deletes `Brief.txt` and writes `brief.txt`: the file the write
/// finds is the one the step deletes, so nothing clashes.
#[test]
fn undoing_a_case_only_rename_does_not_clash() {
    let dir = TestDir::new("writeback-win-case-rename");
    put(&dir.path().join("Brief.txt"));
    let delete = Op::Delete(path("Brief.txt"));
    let write_back = Op::Write(path("brief.txt"), Oid([0; 20]));
    let clashes = respelled(&mut Names::default(), dir.path(), &[&delete, &write_back]).unwrap();
    assert!(clashes.is_empty(), "{clashes:?}");
}

/// A delete Windows refuses because another program holds the file open without delete sharing
/// (a sharing violation) fails the write-back with `matter.file_locked` naming that file, and
/// leaves it in place. `apply` runs after the caller's check, so a file held when it starts is
/// one a program opened between the check and the write.
#[test]
fn a_delete_of_a_file_held_open_is_file_locked_and_keeps_the_file() {
    let mut matter = matter("writeback-win-held");
    let file = matter.folder.join("x.txt");
    put(&file);
    let target = path("x.txt");
    let stat = Stat::from_metadata(&std::fs::symlink_metadata(&file).unwrap());
    matter.core.cache.entries.push(CacheEntry {
        path: target.clone(),
        stat,
        state: EntryState::Skipped,
    });
    let held = write::open_exclusive(&file).unwrap();
    let result = apply(&matter.core, &matter.folder, &[&Op::Delete(target)]);
    drop(held);
    assert!(
        matches!(&result, Err(Error::FileLocked { paths }) if *paths == [path("x.txt")]),
        "{result:?}"
    );
    assert!(std::fs::symlink_metadata(&file).is_ok());
}

/// `text` written to the new file `file`, making its folder.
fn put_text(file: &Path, text: &str) {
    use std::io::Write as _;
    write::create_dir_all(file.parent().unwrap()).unwrap();
    write::create_new(file)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

/// `text` stored as a blob of the matter's history, from a file outside the folder.
fn blob(matter: &mut Matter, text: &str) -> Oid {
    const PROBE: &dyn crate::PlaceholderProbe = &OsProbe;
    let source = matter
        .folder
        .with_file_name(format!("blob-{}", text.replace(' ', "-")));
    put_text(&source, text);
    let len = u64::try_from(text.len()).unwrap();
    let guard = crate::no_download::NoDownload::begin(PROBE);
    let (oid, _) = matter
        .core
        .history
        .store_file(&source, len, u64::MAX, &guard)
        .unwrap();
    oid
}

/// Puts `text` in the matter's file `name` and records its stat in the cache, as the
/// caller's `outside-edits` checkpoint does just before `apply`.
fn cached(matter: &mut Matter, name: &str, text: &str) -> PathBuf {
    let file = matter.folder.join(name);
    put_text(&file, text);
    let stat = Stat::from_metadata(&std::fs::symlink_metadata(&file).unwrap());
    matter.core.cache.entries.push(CacheEntry {
        path: path(name),
        stat,
        state: EntryState::Skipped,
    });
    file
}

/// The `.clauseo-<n>.tmp` files left in `folder`.
fn temp_files(folder: &Path) -> Vec<String> {
    std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".clauseo-"))
        .collect()
}

/// A restore over a file held open with no sharing at all, opened after the check passed:
/// renaming over it fails, and the write-back fails with `matter.file_locked` naming that file
/// alone. The file before it (`a.txt`) is restored already and stays so, a partial restore a
/// retry finishes; the held file keeps its text, and no temp file is left.
#[test]
fn a_restore_over_a_file_held_with_no_sharing_is_file_locked() {
    let mut matter = matter("writeback-win-rename-held");
    cached(&mut matter, "a.txt", "a now");
    let held_file = cached(&mut matter, "b.txt", "b now");
    let a_then = blob(&mut matter, "a then");
    let b_then = blob(&mut matter, "b then!");
    let ops = [
        Op::Write(path("a.txt"), a_then),
        Op::Write(path("b.txt"), b_then),
    ];
    let held = write::open_exclusive(&held_file).unwrap();
    let result = apply(
        &matter.core,
        &matter.folder,
        &ops.iter().collect::<Vec<_>>(),
    );
    drop(held);
    assert!(
        matches!(&result, Err(Error::FileLocked { paths }) if *paths == [path("b.txt")]),
        "{result:?}"
    );
    let read = |name: &str| std::fs::read_to_string(matter.folder.join(name)).unwrap();
    assert_eq!(read("a.txt"), "a then", "renamed before the held file");
    assert_eq!(read("b.txt"), "b now", "the held file is untouched");
    assert_eq!(temp_files(&matter.folder), Vec::<String>::new());
}

/// As Word holds a document: open with read sharing only (`FILE_SHARE_READ`), and its owner
/// file `~$ief.docx` beside it. Renaming over it fails (an access denied or a sharing
/// violation), and the write-back fails with `matter.file_locked` naming it.
#[test]
fn a_restore_over_a_file_word_holds_is_file_locked() {
    let mut matter = matter("writeback-win-rename-word");
    let held_file = cached(&mut matter, "Brief.docx", "brief now");
    put_text(&matter.folder.join("~$ief.docx"), "owner");
    let then = blob(&mut matter, "brief then");
    let held = write::open_as_word(&held_file).unwrap();
    let op = Op::Write(path("Brief.docx"), then);
    let result = apply(&matter.core, &matter.folder, &[&op]);
    drop(held);
    assert!(
        matches!(&result, Err(Error::FileLocked { paths }) if *paths == [path("Brief.docx")]),
        "{result:?}"
    );
    assert_eq!(std::fs::read_to_string(&held_file).unwrap(), "brief now");
    assert_eq!(temp_files(&matter.folder), Vec::<String>::new());
}

/// A folder Windows cannot make (`a|b`) fails the write-back with that folder's name, not
/// with a temp file inside it.
#[test]
fn a_folder_that_cannot_be_made_fails_by_its_own_name() {
    let matter = matter("writeback-win-bad-folder");
    let op = Op::Write(path("a|b/c.txt"), Oid([0; 20]));
    let result = apply(&matter.core, &matter.folder, &[&op]);
    let bad = matter.folder.join("a|b");
    assert!(
        matches!(&result, Err(Error::Io { op: "create", path, .. }) if *path == bad),
        "{result:?}"
    );
}
