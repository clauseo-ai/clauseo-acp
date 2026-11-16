//! What a write-back reports besides its result ([`super::Unfinished`]): the temp files it could
//! not remove and the files whose step after the rename failed, by name, whether it went or
//! failed. And a write-back of many files holds no descriptor per file until its renames.

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterPath;

use super::{Op, Unfinished, apply_or_partial};
use crate::checkpoint::Core;
use crate::history::{GixStore, HistoryStore, Oid};
use crate::stat::{CacheEntry, EntryState, Stat};
use crate::test_dir::TestDir;
use crate::{Error, OsProbe, write};

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

/// A matter folder and its history, with an empty cache.
struct Matter {
    _dir: TestDir,
    folder: PathBuf,
    core: Core,
    trash: crate::FolderTrash,
}

fn matter(name: &str) -> Matter {
    let dir = TestDir::new(name);
    let folder = dir.path().join("matter");
    write::create_dir_all(&folder).unwrap();
    let git_dir = dir.path().join("root").join("m").join("history.git");
    let core = Core::load(GixStore::create(&git_dir, &folder).unwrap()).unwrap();
    let trash = crate::FolderTrash::new(dir.path().join("trash"));
    Matter {
        _dir: dir,
        folder,
        core,
        trash,
    }
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
        .with_file_name(format!("blob-{}", text.replace([' ', '/'], "-")));
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

/// Puts `text` in the matter's file `name` and records its stat in the cache, as the caller's
/// `outside-edits` checkpoint does just before the write-back.
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

/// Runs the write-back of `ops` on `matter`.
fn write_back(matter: &Matter, ops: &[Op]) -> Result<super::Deleted, super::Failed> {
    let ops: Vec<&Op> = ops.iter().collect();
    apply_or_partial(&matter.core, &matter.folder, &matter.trash, &ops)
}

/// Temp files the sweep could not remove (here: its removal made to fail) are named, by their
/// paths in the matter, after a write-back that went and after one that failed.
#[test]
fn a_write_back_names_the_temp_files_it_could_not_remove() {
    let mut matter = matter("writeback-left-behind");
    cached(&mut matter, "a.txt", "a now");
    cached(&mut matter, "sub/b.txt", "b now");
    put_text(&matter.folder.join(".clauseo-4.tmp"), "crash");
    put_text(&matter.folder.join("sub/.clauseo-2.tmp"), "crash");
    let ops = [
        Op::Write(path("a.txt"), blob(&mut matter, "a then")),
        Op::Write(path("sub/b.txt"), blob(&mut matter, "b then")),
    ];
    write::fail_at(Some("sweep"));
    let done = write_back(&matter, &ops);
    write::fail_at(None);
    let left = Unfinished {
        owner_only: Vec::new(),
        left_behind: vec![path(".clauseo-4.tmp"), path("sub/.clauseo-2.tmp")],
    };
    assert_eq!(done.unwrap().unfinished, left);
    let read = |name: &str| std::fs::read_to_string(matter.folder.join(name)).unwrap();
    assert_eq!(read("a.txt"), "a then");
    assert_eq!(read("sub/b.txt"), "b then");

    // Changed since the cache: the write-back fails, and still names them.
    write::append(&matter.folder.join("a.txt"), b"!").unwrap();
    write::fail_at(Some("sweep"));
    let failed = write_back(&matter, &ops);
    write::fail_at(None);
    let failed = failed.unwrap_err();
    assert!(
        matches!(failed.error, Error::FileLocked { .. }),
        "{:?}",
        failed.error
    );
    assert_eq!(failed.unfinished, left);
}

/// A write-back whose step after a rename fails (macOS: the ACL and the mode) goes, and names
/// that file as its owner's alone; no temp file is left.
#[test]
fn a_write_back_names_a_file_whose_step_after_the_rename_failed() {
    let mut matter = matter("writeback-owner-only");
    cached(&mut matter, "Lease.docx", "lease now");
    let ops = [Op::Write(
        path("Lease.docx"),
        blob(&mut matter, "lease then"),
    )];
    write::fail_at(Some("after rename"));
    let done = write_back(&matter, &ops);
    write::fail_at(None);
    let done = done.unwrap();
    assert_eq!(
        done.unfinished,
        Unfinished {
            owner_only: vec![path("Lease.docx")],
            left_behind: Vec::new(),
        }
    );
    let text = std::fs::read_to_string(matter.folder.join("Lease.docx")).unwrap();
    assert_eq!(text, "lease then");
    assert_eq!(
        write::temps_in(&matter.folder),
        Vec::<std::ffi::OsString>::new()
    );
}

/// A restore of 300 files that exist, with the process's open-file limit lowered to 256
/// (launchd's default on macOS): staging holds no descriptor per file, so it goes.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_write_back_of_300_files_fits_in_256_descriptors() {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
    let mut matter = matter("writeback-many");
    let mut ops = Vec::new();
    for n in 0..300 {
        let name = format!("file-{n:03}.txt");
        cached(&mut matter, &name, &format!("{n} now"));
        ops.push(Op::Write(
            path(&name),
            blob(&mut matter, &format!("{n} then")),
        ));
    }
    let limit = getrlimit(Resource::Nofile);
    let lowered = Rlimit {
        current: Some(limit.current.map_or(256, |current| current.min(256))),
        maximum: limit.maximum,
    };
    setrlimit(Resource::Nofile, lowered).unwrap();
    let done = write_back(&matter, &ops);
    setrlimit(Resource::Nofile, limit).unwrap();
    assert_eq!(done.unwrap().unfinished, Unfinished::default());
    for n in 0..300 {
        let text = std::fs::read_to_string(matter.folder.join(format!("file-{n:03}.txt")));
        assert_eq!(text.unwrap(), format!("{n} then"));
    }
}
