//! A restore, the undo of one, and a tracked-changes decision, whole, when a write in place
//! fails part way or a file is refused: what the caller gets, what is on disk after, and that
//! restoring the version before brings the file back. On Linux the rename's refusal is
//! simulated ("rename refused", and the later steps armed with `write::fail_at`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use clauseo_protocol::{CheckpointId, MatterId, MatterPath, Timestamp};

use crate::test_dir::TestDir;
use crate::types::{Imported, Limits, Marker, NewMatter, Retention, Settings};
use crate::{Error, Matter, OsProbe, RefusedBecause, ReplaceFile, Restore, RestoreFiles, write};

fn cid(n: u64) -> CheckpointId {
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

fn at(n: u64) -> Timestamp {
    Timestamp::new(1_700_000_000_000 + i64::try_from(n).unwrap() * 1000).unwrap()
}

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

/// A text of `len` bytes, one letter after another from `first`.
fn text(first: u8, len: usize) -> String {
    (0..len)
        .map(|i| char::from(first + u8::try_from(i % 26).unwrap()))
        .collect()
}

/// A matter folder in a test's own folder.
struct Fx {
    dir: TestDir,
    folder: PathBuf,
}

impl Fx {
    fn file(&self, name: &str) -> PathBuf {
        self.folder.join(name)
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.file(name)).unwrap()
    }

    /// `text` as the file `name`'s content, a new file in its place.
    fn put(&self, name: &str, text: &str) {
        let file = self.file(name);
        if file.exists() {
            write::remove_file(&file).unwrap();
        }
        put(&file, text);
    }
}

fn put(file: &Path, text: &str) {
    use std::io::Write as _;
    write::create_dir_all(file.parent().unwrap()).unwrap();
    write::create_new(file)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

/// A folder holding `files`, imported as a matter (checkpoint 1).
fn imported(name: &str, files: &[(&str, &str)]) -> (Fx, Matter) {
    let dir = TestDir::new(name);
    let folder = dir.path().join("m");
    for (name, text) in files {
        put(&folder.join(name), text);
    }
    let settings = Settings {
        matters_root: dir.path().join("root"),
        refused_trees: Vec::new(),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::new(OsProbe),
        trash: Arc::new(crate::FolderTrash::new(dir.path().join("trash"))),
    };
    let new = NewMatter {
        id: "01910000-0000-7000-8000-000000000001"
            .parse::<MatterId>()
            .unwrap(),
        folder: folder.clone(),
        created_at: at(1),
        marker: Marker::None,
        checkpoint: cid(1),
    };
    let imported = Matter::import(settings, new, &[], &mut |_| {}, &AtomicBool::new(false));
    let Imported::Done { matter, .. } = imported.unwrap() else {
        panic!("the import was cancelled");
    };
    (Fx { dir, folder }, *matter)
}

/// Restores all of `from` (taking back the restore `undoes`), with the steps `steps` failing.
fn restore(
    matter: &mut Matter,
    from: u64,
    undoes: Option<u64>,
    ids: (u64, u64),
    steps: Option<&'static str>,
) -> Result<crate::Restored, Error> {
    write::fail_at(steps);
    let done = matter.restore(&Restore {
        from: cid(from),
        files: RestoreFiles::All,
        outside_edits: cid(ids.0),
        checkpoint: cid(ids.1),
        at: at(ids.1),
        undoes: undoes.map(cid),
    });
    write::fail_at(None);
    done
}

/// The parts of [`Error::PartlyWritten`]: the file, whether it was put back, and `of`.
fn partly(error: &Error) -> (String, bool, usize) {
    let Error::PartlyWritten {
        path, put_back, of, ..
    } = error
    else {
        panic!("not PartlyWritten: {error:?}");
    };
    (path.as_str().to_owned(), *put_back, *of)
}

fn on_timeline(matter: &Matter, n: u64) -> bool {
    matter.timeline().iter().any(|entry| entry.id == cid(n))
}

/// A restore whose write in place fails part way and is not put back: `matter.file_write_failed`
/// naming the file (not "couldn't save a version"), no restore checkpoint, the file torn; then
/// restoring the version before (the outside-edits checkpoint) brings it back byte for byte.
#[test]
fn a_torn_restore_names_the_file_and_the_version_before_brings_it_back() {
    let old = text(b'a', 200 * 1024);
    let (fx, mut matter) = imported("flow-torn", &[("Lease.docx", &old)]);
    let now = text(b'A', 200 * 1024);
    fx.put("Lease.docx", &now);
    let error = restore(&mut matter, 1, None, (2, 3), Some("in place torn+put back")).unwrap_err();
    assert_eq!(partly(&error), ("Lease.docx".to_owned(), false, 1));
    assert_eq!(
        error.code(),
        clauseo_protocol::ErrorCode::MatterFileWriteFailed
    );
    assert!(on_timeline(&matter, 2) && !on_timeline(&matter, 3));
    let chunk = 64 * 1024;
    // The restore writes `old` over `now`: its first chunk, then the rest of `now`.
    let (head, tail) = (old.get(..chunk).unwrap(), now.get(chunk..).unwrap());
    let torn = format!("{head}{tail}");
    assert!(
        fx.read("Lease.docx") == torn,
        "not torn as the failure left it"
    );

    restore(&mut matter, 2, None, (4, 5), None).unwrap();
    assert!(
        fx.read("Lease.docx") == now,
        "the version before is not back"
    );
}

/// A restore whose write in place fails part way and is put back: the same error, saying so,
/// and the file as it was before the restore, byte for byte.
#[test]
fn a_torn_restore_put_back_leaves_the_file_as_it_was() {
    let old = text(b'a', 200 * 1024);
    let (fx, mut matter) = imported("flow-put-back", &[("Lease.docx", &old)]);
    let now = text(b'A', 200 * 1024);
    fx.put("Lease.docx", &now);
    let error = restore(&mut matter, 1, None, (2, 3), Some("in place torn")).unwrap_err();
    assert_eq!(partly(&error), ("Lease.docx".to_owned(), true, 1));
    assert!(!on_timeline(&matter, 3));
    assert!(fx.read("Lease.docx") == now, "the file is not as it was");
}

/// Accepting or rejecting tracked changes whose write in place fails part way: an error, never
/// the success it was before, whether the file was put back or not.
#[test]
fn a_tear_during_a_tracked_changes_decision_is_an_error_not_a_success() {
    let (fx, mut matter) = imported("flow-replace", &[("Lease.docx", "with changes")]);
    let expected = fx.dir.path().join("expected.docx");
    let new = fx.dir.path().join("new.docx");
    put(&expected, "with changes");
    put(&new, "changes accepted");
    let lease = path("Lease.docx");
    let mut decide = |ids: (u64, u64), steps| {
        write::fail_at(Some(steps));
        let done = matter.replace_file(&ReplaceFile {
            path: &lease,
            expected: &expected,
            new: &new,
            outside_edits: cid(ids.0),
            checkpoint: cid(ids.1),
            at: at(ids.1),
        });
        write::fail_at(None);
        done
    };
    let error = decide((2, 3), "in place torn").unwrap_err();
    assert_eq!(partly(&error), ("Lease.docx".to_owned(), true, 1));
    assert_eq!(fx.read("Lease.docx"), "with changes");
    let error = decide((4, 5), "in place cut+put back").unwrap_err();
    assert_eq!(partly(&error), ("Lease.docx".to_owned(), false, 1));
    assert!(!on_timeline(&matter, 3) && !on_timeline(&matter, 5));
}

/// A restore of three files whose middle one has a second name: the other two are written and
/// checkpointed, the middle one refused and left alone with its link; taking that restore back
/// puts back only the two it wrote.
#[test]
fn undoing_a_restore_with_a_refused_file_puts_back_only_what_it_wrote() {
    let files = [("a.docx", "a1"), ("b.docx", "b1"), ("c.docx", "c1")];
    let (fx, mut matter) = imported("flow-undo-partial", &files);
    for (name, text) in [("a.docx", "a2"), ("b.docx", "b2"), ("c.docx", "c2")] {
        fx.put(name, text);
    }
    let link = fx.dir.path().join("elsewhere.docx");
    write::hard_link(&fx.file("b.docx"), &link).unwrap();
    let done = restore(&mut matter, 1, None, (2, 3), Some("rename refused")).unwrap();
    assert_eq!(done.written, [path("a.docx"), path("c.docx")]);
    assert_eq!(done.refused, [(path("b.docx"), RefusedBecause::SecondName)]);
    let read = |fx: &Fx| ["a.docx", "b.docx", "c.docx"].map(|name| fx.read(name));
    assert_eq!(read(&fx), ["a1", "b2", "c1"]);
    assert!(on_timeline(&matter, 3));

    let undone = restore(&mut matter, 2, Some(3), (4, 5), None).unwrap();
    assert_eq!(undone.written, [path("a.docx"), path("c.docx")]);
    assert_eq!(read(&fx), ["a2", "b2", "c2"]);
    assert_eq!(std::fs::read_to_string(&link).unwrap(), "b2");
}
