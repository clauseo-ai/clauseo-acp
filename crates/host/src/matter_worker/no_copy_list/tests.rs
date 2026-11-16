//! The host's side of the no-copy list: the changes are appended to the file the launch made,
//! never to another one at its place.

use std::path::PathBuf;

use clauseo_protocol::{MatterPath, SkippedFile, SkippedFileReason};

use super::update;
use crate::pre_write_guard::no_copy::{HEADER, List, MAX_BYTES, MAX_FILES, NoCopyList};
use crate::write::{self, RecordId};

fn folder(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "clauseo-host-no-copy-list-{test}-{}",
        std::process::id()
    ));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

fn cloud(path: &str) -> SkippedFile {
    SkippedFile {
        path: MatterPath::new(path).unwrap(),
        reason: SkippedFileReason::CloudOnly,
    }
}

fn made(root: &std::path::Path) -> NoCopyList {
    crate::pre_write_guard::no_copy::make(root).unwrap()
}

#[test]
fn each_turns_changes_are_appended_to_the_file_made() {
    let root = folder("appended");
    let list = made(&root);
    update(Some(&list), &[cloud("B.pdf"), cloud("A.pdf")]);
    update(Some(&list), &[cloud("B.pdf"), cloud("A.pdf")]);
    update(Some(&list), &[cloud("A.pdf")]);
    update(None, &[cloud("C.pdf")]);
    assert_eq!(
        std::fs::read_to_string(&list.file).unwrap(),
        format!("{HEADER}add cloud-only A.pdf\nadd cloud-only B.pdf\nremove B.pdf\n")
    );
    let reopened = std::fs::File::open(&list.file).unwrap();
    assert_eq!(RecordId::of(&reopened).unwrap(), list.id, "the same file");
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn another_file_or_a_link_at_its_place_is_never_written() {
    let root = folder("replaced");
    let list = made(&root);
    write::remove_file(&list.file).unwrap();
    write::write_file(&list.file, HEADER.as_bytes()).unwrap();
    update(Some(&list), &[cloud("A.pdf")]);
    assert_eq!(std::fs::read_to_string(&list.file).unwrap(), HEADER);
    // A hard link to a file elsewhere, with the identity of that file: two links, untouched.
    let target = root.join("profile.txt");
    write::write_file(&target, HEADER.as_bytes()).unwrap();
    write::remove_file(&list.file).unwrap();
    write::hard_link(&target, &list.file).unwrap();
    let id = RecordId::of(&std::fs::File::open(&target).unwrap()).unwrap();
    let linked = NoCopyList {
        file: list.file,
        id,
    };
    update(Some(&linked), &[cloud("A.pdf")]);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), HEADER);
    write::remove_dir_all(&root).unwrap();
}

/// 20,000 files with paths of about 440 bytes come to more than the guard reads: the host stops
/// appending at the guard's limit, the guard still reads the list (never as empty), and the next
/// turn's update appends nothing more, so the file does not grow turn by turn.
#[test]
fn a_list_at_the_guards_limit_is_still_read_and_never_grows() {
    let root = folder("limit");
    let list = made(&root);
    let long = "x".repeat(140);
    let files: Vec<SkippedFile> = (0..MAX_FILES)
        .map(|i| cloud(&format!("{long}/{long}/{long}-{i:05}.pdf")))
        .collect();
    update(Some(&list), &files);
    let len = std::fs::metadata(&list.file).unwrap().len();
    assert!(len <= MAX_BYTES, "{len} bytes");
    assert!(len > MAX_BYTES - 1024, "filled to the limit: {len} bytes");
    let text = std::fs::read_to_string(&list.file).unwrap();
    assert!(text.ends_with('\n'), "whole lines only");
    let first = MatterPath::new(&format!("{long}/{long}/{long}-00000.pdf")).unwrap();
    assert!(
        !List::read(&list).changes(&[]).is_empty(),
        "the list reads as the files it names, not as empty"
    );
    assert!(List::read(&list).names(&first));
    update(Some(&list), &files);
    update(Some(&list), &files);
    assert_eq!(
        std::fs::metadata(&list.file).unwrap().len(),
        len,
        "nothing re-appended"
    );
    write::remove_dir_all(&root).unwrap();
}
