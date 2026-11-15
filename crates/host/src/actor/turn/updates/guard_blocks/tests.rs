//! The host's read of the guard's record file: complete lines only, from where it stopped, a
//! line that does not parse dropped, a file cut short read from its start, a link never
//! followed.

use std::path::{Path, PathBuf};

use clauseo_protocol::{MatterPath, Timestamp};

use super::{MAX_READ, Offset, read_new};
use crate::pre_write_guard::record::{Agent, Reason, Record};
use crate::write;

/// A folder for one test, removed on drop.
struct Folder(PathBuf);

impl Folder {
    fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "clauseo-guard-blocks-{test}-{}",
            std::process::id()
        ));
        drop(write::remove_dir_all(&path));
        write::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self) -> PathBuf {
        self.0.join("guard-blocks.jsonl")
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}

/// The record of a refused write to `file`.
fn held(file: &str) -> Record {
    Record {
        at: Timestamp::ZERO,
        agent: Agent::Codex,
        tool: "apply_patch".to_owned(),
        reason: Reason::HeldOpen,
        path: Some(MatterPath::new(file).unwrap()),
    }
}

fn line(record: &Record) -> String {
    format!("{}\n", serde_json::to_string(record).unwrap())
}

fn append(path: &Path, text: &str) {
    let mut old = std::fs::read(path).unwrap_or_default();
    old.extend_from_slice(text.as_bytes());
    write::write_file(path, &old).unwrap();
}

#[test]
fn new_complete_lines_are_read_once_and_a_half_line_waits() {
    let folder = Folder::new("lines");
    let file = folder.file();
    let mut offset = Offset::default();
    assert_eq!(read_new(&file, &mut offset), Vec::new(), "no file yet");
    let first = line(&held("A.docx"));
    let second = line(&held("Drafts/B.docx"));
    let (head, tail) = second.split_at(10);
    append(
        &file,
        &format!("{first}not json\n{{\"reason\":\"other\"}}\n{head}"),
    );
    assert_eq!(read_new(&file, &mut offset), vec![held("A.docx")]);
    assert_eq!(
        read_new(&file, &mut offset),
        Vec::new(),
        "the half line waits"
    );
    append(&file, tail);
    assert_eq!(read_new(&file, &mut offset), vec![held("Drafts/B.docx")]);
    assert_eq!(
        read_new(&file, &mut offset),
        Vec::new(),
        "nothing is read twice"
    );
}

#[test]
fn a_file_cut_short_or_replaced_is_read_from_its_start() {
    let folder = Folder::new("cut");
    let file = folder.file();
    let mut offset = Offset::default();
    append(
        &file,
        &format!("{}{}", line(&held("A.docx")), line(&held("B.docx"))),
    );
    assert_eq!(read_new(&file, &mut offset).len(), 2);
    write::write_file(&file, line(&held("C.docx")).as_bytes()).unwrap();
    assert_eq!(read_new(&file, &mut offset), vec![held("C.docx")]);
}

#[test]
fn a_line_past_the_cap_is_passed_over() {
    let folder = Folder::new("long");
    let file = folder.file();
    let mut offset = Offset::default();
    let long = "x".repeat(usize::try_from(MAX_READ).unwrap() + 10);
    append(&file, &format!("{long}\n{}", line(&held("A.docx"))));
    assert_eq!(read_new(&file, &mut offset), Vec::new());
    assert_eq!(read_new(&file, &mut offset), vec![held("A.docx")]);
}

#[cfg(unix)]
#[test]
fn a_link_or_a_folder_in_its_place_reads_as_nothing() {
    let folder = Folder::new("link");
    let target = folder.0.join("elsewhere.jsonl");
    append(&target, &line(&held("A.docx")));
    write::symlink(&target, &folder.file()).unwrap();
    assert_eq!(read_new(&folder.file(), &mut Offset::default()), Vec::new());
    let dir = Folder::new("dir");
    write::create_dir_all(&dir.file()).unwrap();
    assert_eq!(read_new(&dir.file(), &mut Offset::default()), Vec::new());
}
