//! A write in place that fails after it began ([`crate::write`]'s `in_place`): the write-back
//! stops there with [`Error::PartlyWritten`], and the version confirmed just before is written
//! back through the same open file; the error says whether that worked. One that fails before
//! its first byte, or a delete refused for its permissions, is a refused file instead, and the
//! files after it are still written. On Linux the rename's refusal is simulated ("rename
//! refused", and the later steps armed with `write::fail_at`).

use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::Path;

use rustix::fs::{CWD, FileType, Mode, mknodat};

use super::in_place_tests::{record_stat, refused_write_back, saved};
use super::outcome_tests::{Matter, blob, matter, path, write_back};
use super::{Failed, Op};
use crate::history::{HistoryStore as _, Oid};
use crate::{Error, RefusedBecause, write};

/// A text of `len` bytes, one letter after another from `first` (`b'a'` or `b'A'`): two such
/// texts differ at every byte.
fn text(first: u8, len: usize) -> String {
    (0..len)
        .map(|i| char::from(first + u8::try_from(i % 26).unwrap()))
        .collect()
}

/// `text` stored as a blob of the matter's history.
fn stored(matter: &mut Matter, text: &str) -> Oid {
    matter.core.history.write_blob(text.as_bytes()).unwrap()
}

fn bytes(file: &Path) -> String {
    std::fs::read_to_string(file).unwrap()
}

/// Writes `ops` with every rename refused and the steps `steps` of the write in place failing.
fn failing(matter: &Matter, ops: &[Op], steps: &'static str) -> Failed {
    write::fail_at(Some(steps));
    let done = write_back(matter, ops);
    write::fail_at(None);
    done.unwrap_err()
}

/// The parts of [`Error::PartlyWritten`]: the file, whether it was put back, and `of`.
fn partly(failed: &Failed) -> (String, bool, usize) {
    let Error::PartlyWritten {
        path, put_back, of, ..
    } = &failed.error
    else {
        panic!("not PartlyWritten: {:?}", failed.error);
    };
    (path.as_str().to_owned(), *put_back, *of)
}

/// `text` from byte `from` to byte `to` (ASCII, so any byte is a boundary).
fn part(text: &str, from: usize, to: usize) -> &str {
    text.get(from..to).unwrap()
}

/// A file larger than one 64 KiB chunk, its write failing after the first chunk: the old
/// content is written back whole, so the file is as it was byte for byte, the same inode, the
/// folder unchanged; and no temp file is left.
#[test]
fn a_large_file_failing_after_its_first_chunk_is_put_back_as_it_was() {
    let mut matter = matter("torn-large");
    let old = text(b'a', 200 * 1024);
    let file = saved(&mut matter, "Lease.docx", &old);
    let inode = std::fs::metadata(&file).unwrap().ino();
    let new = text(b'A', 200 * 1024);
    let ops = [Op::Write(path("Lease.docx"), stored(&mut matter, &new))];
    let failed = failing(&matter, &ops, "in place torn");
    assert_eq!(partly(&failed), ("Lease.docx".to_owned(), true, 1));
    assert!(!failed.changed);
    assert_eq!(failed.unfinished.refused, []);
    assert!(bytes(&file) == old, "the file is not as it was");
    assert_eq!(std::fs::metadata(&file).unwrap().ino(), inode);
    assert_eq!(
        write::temps_in(&matter.folder),
        Vec::<std::ffi::OsString>::new()
    );
}

/// New content shorter than the old, the cut to length failing (the old content's tail still
/// after the new), or the sync failing: each put back as it was.
#[test]
fn a_failed_cut_or_sync_is_put_back_as_it_was() {
    for step in ["in place cut", "in place sync"] {
        let mut matter = matter("torn-cut");
        let old = text(b'a', 100 * 1024);
        let file = saved(&mut matter, "Lease.docx", &old);
        let ops = [Op::Write(path("Lease.docx"), blob(&mut matter, "short"))];
        let failed = failing(&matter, &ops, step);
        assert_eq!(
            partly(&failed),
            ("Lease.docx".to_owned(), true, 1),
            "{step}"
        );
        assert!(bytes(&file) == old, "{step}: the file is not as it was");
    }
}

/// When writing the old content back fails too, the error says the file may be torn, the
/// folder counts as changed, and the file is what the failed write left: the new content's first
/// chunk over the old, or the new content then the old content's tail.
#[test]
fn a_file_not_put_back_is_named_as_torn() {
    let mut matter = matter("torn-not-put-back");
    let old = text(b'a', 200 * 1024);
    let file = saved(&mut matter, "Lease.docx", &old);
    let new = text(b'A', 200 * 1024);
    let ops = [Op::Write(path("Lease.docx"), stored(&mut matter, &new))];
    let failed = failing(&matter, &ops, "in place torn+put back");
    assert_eq!(partly(&failed), ("Lease.docx".to_owned(), false, 1));
    assert!(failed.changed);
    let chunk = 64 * 1024;
    let torn = format!("{}{}", part(&new, 0, chunk), part(&old, chunk, old.len()));
    assert!(bytes(&file) == torn, "not the first chunk over the old");

    let mut matter = self::matter("torn-tail");
    let file = saved(&mut matter, "Lease.docx", &old);
    let ops = [Op::Write(path("Lease.docx"), blob(&mut matter, "short"))];
    let failed = failing(&matter, &ops, "in place cut+put back");
    assert_eq!(partly(&failed), ("Lease.docx".to_owned(), false, 1));
    assert!(
        bytes(&file) == format!("short{}", part(&old, 5, old.len())),
        "not the old tail"
    );
}

/// A write in place that fails part way stops the write-back: the file after it is not
/// written, and `of` counts every file of the write-back.
#[test]
fn a_file_failing_part_way_stops_the_write_back() {
    let mut matter = matter("torn-stops");
    let a = saved(&mut matter, "a.docx", "a now");
    let b = saved(&mut matter, "b.docx", "b now");
    let ops = [
        Op::Write(path("a.docx"), blob(&mut matter, "a then")),
        Op::Write(path("b.docx"), blob(&mut matter, "b then")),
    ];
    let failed = failing(&matter, &ops, "in place torn");
    assert_eq!(partly(&failed), ("a.docx".to_owned(), true, 2));
    assert_eq!((bytes(&a), bytes(&b)), ("a now".into(), "b now".into()));
}

/// A write in place failing before its first byte is no failure of the write-back: the file
/// is refused (its saved copy not confirmed), as it was, and the next file is written.
#[test]
fn a_write_in_place_failing_before_its_first_byte_is_refused() {
    let mut matter = matter("torn-before");
    let a = saved(&mut matter, "a.docx", "a now");
    let b = saved(&mut matter, "b.docx", "b now");
    let ops = [
        Op::Write(path("a.docx"), blob(&mut matter, "a then")),
        Op::Write(path("b.docx"), blob(&mut matter, "b then")),
    ];
    write::fail_at(Some("in place"));
    let done = write_back(&matter, &ops);
    write::fail_at(None);
    let done = done.unwrap();
    let refused = [
        (path("a.docx"), RefusedBecause::SavedCopyUnconfirmed),
        (path("b.docx"), RefusedBecause::SavedCopyUnconfirmed),
    ];
    assert_eq!(done.unfinished.refused, refused);
    assert_eq!((bytes(&a), bytes(&b)), ("a now".into(), "b now".into()));
}

/// Another file put at the name after the write in place opened the target: refused as
/// unconfirmed, before a byte is written to either file.
#[test]
fn a_file_swapped_since_it_was_opened_is_refused_and_both_left_alone() {
    let mut matter = matter("torn-swapped");
    let file = saved(&mut matter, "Lease.docx", "lease now");
    let other = matter.folder.with_file_name("other.docx");
    super::outcome_tests::put_text(&other, "another file");
    let ops = [Op::Write(
        path("Lease.docx"),
        blob(&mut matter, "lease then"),
    )];
    let to = file.clone();
    write::after_open(move || write::rename(&other, &to).unwrap());
    let done = refused_write_back(&matter, &ops).unwrap();
    assert_eq!(
        done.unfinished.refused,
        [(path("Lease.docx"), RefusedBecause::SavedCopyUnconfirmed)]
    );
    assert_eq!(bytes(&file), "another file");
}

/// A FIFO where the file was: the open does not wait for a writer, and it is refused as
/// unconfirmed (not a regular file).
#[test]
fn a_fifo_target_is_refused_without_waiting() {
    let mut matter = matter("torn-fifo");
    saved(&mut matter, "Lease.docx", "lease now");
    let file = matter.folder.join("Lease.docx");
    write::remove_file(&file).unwrap();
    mknodat(CWD, &file, FileType::Fifo, Mode::from_raw_mode(0o644), 0).unwrap();
    record_stat(&mut matter, 0, "Lease.docx");
    let ops = [Op::Write(
        path("Lease.docx"),
        blob(&mut matter, "lease then"),
    )];
    let done = refused_write_back(&matter, &ops).unwrap();
    assert_eq!(
        done.unfinished.refused,
        [(path("Lease.docx"), RefusedBecause::SavedCopyUnconfirmed)]
    );
    assert!(
        std::fs::symlink_metadata(&file)
            .unwrap()
            .file_type()
            .is_fifo()
    );
}

/// A delete the folder refuses for its permissions (a folder that denies removing its files):
/// the file is named as refused, as it was, and the write after it is still made. Not as root,
/// whom the folder's mode does not stop.
#[test]
fn a_delete_refused_for_permissions_is_named_and_the_rest_written() {
    use std::os::unix::fs::PermissionsExt as _;
    let mut matter = matter("delete-refused");
    // In path order, as the cache holds its entries.
    let a = saved(&mut matter, "a.docx", "a now");
    let kept = saved(&mut matter, "locked/kept.docx", "kept");
    let locked = matter.folder.join("locked");
    write::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let ops = [
        Op::Delete(path("locked/kept.docx")),
        Op::Write(path("a.docx"), blob(&mut matter, "a then")),
    ];
    let done = write_back(&matter, &ops);
    write::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    let done = done.unwrap();
    assert_eq!(
        done.unfinished.refused,
        [(path("locked/kept.docx"), RefusedBecause::Permissions)]
    );
    assert!(done.paths.is_empty(), "{:?}", done.paths);
    assert_eq!((bytes(&kept), bytes(&a)), ("kept".into(), "a then".into()));
    let ops: Vec<&Op> = ops.iter().collect();
    assert!(matches!(
        done.unfinished.refused_error(&ops),
        Some(Error::FilesRefused { of: 2, .. })
    ));
}
