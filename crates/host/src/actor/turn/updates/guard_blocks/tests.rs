//! The host's read of the guard's record file: complete lines only, from where it stopped, a
//! line that does not parse skipped and the next one read, a file cut short read from its start,
//! a link or a second link never read; a paper named by its name alone, shown with nothing
//! invisible (#1122's `shown.rs`) and cut to a length, and only when the matter holds it.

use std::path::{Path, PathBuf};

use clauseo_protocol::{MatterPath, NoticeKind, Timestamp};

use super::{MAX_NAME, MAX_READ, read_new, shown_name};
use crate::pre_write_guard::record::{Agent, Reason, Record};
use crate::write;

/// A folder for one test, removed on drop: the matter in `matter`, the record beside it.
struct Folder(PathBuf);

impl Folder {
    fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "clauseo-guard-blocks-{test}-{}",
            std::process::id()
        ));
        drop(write::remove_dir_all(&path));
        write::create_dir_all(&path.join("matter").join("Drafts")).unwrap();
        for paper in ["A.docx", "B.docx", "C.docx", "Drafts/B.docx"] {
            write::write_file(&path.join("matter").join(paper), b"docx").unwrap();
        }
        Self(path)
    }

    fn file(&self) -> PathBuf {
        self.0.join("guard-blocks.jsonl")
    }

    fn matter(&self) -> PathBuf {
        self.0.join("matter")
    }

    /// What is new in the record past `offset`, which moves on.
    fn read(&self, offset: &mut u64) -> Vec<NoticeKind> {
        let (next, kinds) = read_new(&self.file(), *offset, &self.matter());
        *offset = next;
        kinds
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

/// The guard's line for `record`: a newline before and after it.
fn line(record: &Record) -> String {
    format!("\n{}\n", serde_json::to_string(record).unwrap())
}

/// The notice kind naming `name`.
fn named(name: &str) -> NoticeKind {
    NoticeKind::HeldOpenBlocked {
        file: MatterPath::new(name).unwrap(),
    }
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
    let mut offset = 0;
    assert_eq!(folder.read(&mut offset), Vec::new(), "no file yet");
    let first = line(&held("A.docx"));
    let second = line(&held("Drafts/B.docx"));
    let (head, tail) = second.split_at(10);
    append(
        &file,
        &format!("{first}not json\n{{\"reason\":\"other\"}}\n{head}"),
    );
    assert_eq!(folder.read(&mut offset), vec![named("A.docx")]);
    assert_eq!(folder.read(&mut offset), Vec::new(), "the half line waits");
    append(&file, tail);
    assert_eq!(folder.read(&mut offset), vec![named("B.docx")]);
    assert_eq!(
        folder.read(&mut offset),
        Vec::new(),
        "nothing is read twice"
    );
}

/// A stray part of a line, with no newline after it, spoils neither the guard's next line
/// (which starts with a newline of its own) nor the ones after it.
#[test]
fn a_stray_partial_line_never_spoils_the_next_real_one() {
    let folder = Folder::new("stray");
    let file = folder.file();
    let mut offset = 0;
    append(&file, "x");
    append(&file, &line(&held("A.docx")));
    append(&file, "{\"at\":");
    append(&file, &line(&held("C.docx")));
    assert_eq!(
        folder.read(&mut offset),
        vec![named("A.docx"), named("C.docx")]
    );
}

#[test]
fn a_file_cut_short_or_replaced_is_read_from_its_start() {
    let folder = Folder::new("cut");
    let file = folder.file();
    let mut offset = 0;
    append(
        &file,
        &format!("{}{}", line(&held("A.docx")), line(&held("B.docx"))),
    );
    assert_eq!(folder.read(&mut offset).len(), 2);
    write::write_file(&file, line(&held("C.docx")).as_bytes()).unwrap();
    assert_eq!(folder.read(&mut offset), vec![named("C.docx")]);
}

#[test]
fn a_line_past_the_cap_is_passed_over() {
    let folder = Folder::new("long");
    let file = folder.file();
    let mut offset = 0;
    let long = "x".repeat(usize::try_from(MAX_READ).unwrap() + 10);
    append(&file, &format!("{long}{}", line(&held("A.docx"))));
    assert_eq!(folder.read(&mut offset), Vec::new());
    assert_eq!(folder.read(&mut offset), vec![named("A.docx")]);
}

/// A recorded path the matter does not hold is a step stopped, the paper not named; the
/// `.codex` folder and a record with no path are their own kinds.
#[test]
fn only_a_paper_the_matter_holds_is_named() {
    let folder = Folder::new("matter");
    let folder_line = Record {
        reason: Reason::CodexFolder,
        path: None,
        ..held("A.docx")
    };
    let no_path = Record {
        path: None,
        ..held("A.docx")
    };
    let lines = [
        line(&held("Drafts/Missing.docx")),
        line(&held("Elsewhere/A.docx")),
        line(&folder_line),
        line(&no_path),
        line(&held("Drafts/B.docx")),
    ];
    append(&folder.file(), &lines.concat());
    assert_eq!(
        folder.read(&mut 0),
        vec![
            NoticeKind::GuardStepBlocked,
            NoticeKind::GuardStepBlocked,
            NoticeKind::CodexFolderBlocked,
            NoticeKind::GuardStepBlocked,
            named("B.docx"),
        ]
    );
}

/// A name shows on one line with nothing that hides, breaks or turns it, by #1122's rule
/// (`shown.rs`): bidi controls (U+202A to U+202E, U+2066 to U+2069), the line and paragraph
/// separators, zero-width and control characters and `\` written out as escapes; a
/// right-to-left name isolated; cut first to [`MAX_NAME`] characters, its middle elided.
#[test]
fn a_name_shows_with_every_invisible_character_written_out() {
    assert_eq!(shown_name("Lease.docx"), "Lease.docx");
    for c in ('\u{202a}'..='\u{202e}').chain('\u{2066}'..='\u{2069}') {
        let shown = shown_name(&format!("a{c}b.docx"));
        assert_eq!(shown, format!("a{}b.docx", c.escape_default()), "{c:?}");
        assert!(!shown.contains(c), "{c:?}");
    }
    for (c, written) in [
        ('\u{2028}', r"\u{2028}"),
        ('\u{2029}', r"\u{2029}"),
        ('\u{200b}', r"\u{200b}"),
        ('\u{200d}', r"\u{200d}"),
        ('\u{feff}', r"\u{feff}"),
        ('\u{7}', r"\u{7}"),
        ('\u{1b}', r"\u{1b}"),
        ('\u{85}', r"\u{85}"),
        ('\n', r"\n"),
        ('\\', r"\\"),
    ] {
        assert_eq!(
            shown_name(&format!("a{c}b")),
            format!("a{written}b"),
            "{c:?}"
        );
    }
    // A right-to-left name is shown as #1122's rule shows it, between isolates of its own.
    let rtl = shown_name("עסקה.docx");
    assert_eq!(rtl, crate::shown::text("עסקה.docx"));
    assert!(
        rtl.starts_with('\u{2066}') && rtl.ends_with('\u{2069}'),
        "{rtl:?}"
    );
}

#[test]
fn a_long_name_is_cut_in_the_middle_to_a_fixed_length() {
    let exact = "x".repeat(MAX_NAME);
    assert_eq!(shown_name(&exact), exact);
    let long = format!("{}{}.docx", "a".repeat(40), "b".repeat(40));
    let shown = shown_name(&long);
    assert_eq!(shown.chars().count(), MAX_NAME);
    assert_eq!(shown, format!("{}…{}.docx", "a".repeat(30), "b".repeat(24)));
    // Characters, not bytes: a cut never splits one.
    let wide = "दस्तावेज़".repeat(10);
    assert_eq!(shown_name(&wide).chars().count(), MAX_NAME);
    // The cut comes before the isolates, so they stay whole: as many opened as closed, and
    // the text inside them cut to the length.
    let rtl = shown_name(&"ע".repeat(100));
    let opens = |c: &char| ('\u{2066}'..='\u{2068}').contains(c);
    let opened = rtl.chars().filter(opens).count();
    assert!(opened > 0, "{rtl:?}");
    assert_eq!(opened, rtl.matches('\u{2069}').count(), "{rtl:?}");
    let text = rtl.chars().filter(|c| !opens(c) && *c != '\u{2069}');
    assert_eq!(text.count(), MAX_NAME);
}

/// A held paper whose name has an invisible character shows as an escape, which no matter path
/// may hold (`\`), so the step is told unnamed rather than with the character.
#[test]
fn a_name_with_an_invisible_character_is_told_unnamed() {
    let folder = Folder::new("invisible");
    let name = "Lease\u{202e}xcod.exe";
    write::write_file(&folder.matter().join(name), b"docx").unwrap();
    append(&folder.file(), &line(&held(name)));
    assert_eq!(folder.read(&mut 0), vec![NoticeKind::GuardStepBlocked]);
}

/// A hard link at the record's place (two links) reads as nothing: the file it names is
/// never read as the record.
#[test]
fn a_second_link_at_the_records_place_reads_as_nothing() {
    let folder = Folder::new("hard-link");
    let target = folder.0.join("elsewhere.jsonl");
    append(&target, &line(&held("A.docx")));
    write::hard_link(&target, &folder.file()).unwrap();
    assert_eq!(folder.read(&mut 0), Vec::new());
}

#[cfg(unix)]
#[test]
fn a_link_or_a_folder_in_its_place_reads_as_nothing() {
    let folder = Folder::new("link");
    let target = folder.0.join("elsewhere.jsonl");
    append(&target, &line(&held("A.docx")));
    write::symlink(&target, &folder.file()).unwrap();
    assert_eq!(folder.read(&mut 0), Vec::new());
    let dir = Folder::new("dir");
    write::create_dir_all(&dir.file()).unwrap();
    assert_eq!(dir.read(&mut 0), Vec::new());
}
