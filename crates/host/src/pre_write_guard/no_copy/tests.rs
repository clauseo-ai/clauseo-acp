//! The list's lines: folded in order, a bad line skipped, nothing without the header, and the
//! changes the host appends.

use std::fmt::Write as _;

use clauseo_protocol::{MatterPath, SkippedFile, SkippedFileReason};

use super::{HEADER, List, MAX_BYTES, MAX_FILES, fitting, refusal, within_limit};

const CLOUD: SkippedFileReason = SkippedFileReason::CloudOnly;
const LARGE: SkippedFileReason = SkippedFileReason::OverSizeCap;

fn file(path: &str, reason: SkippedFileReason) -> SkippedFile {
    SkippedFile {
        path: MatterPath::new(path).unwrap(),
        reason,
    }
}

fn listed(list: &List, path: &str) -> Option<(String, SkippedFileReason)> {
    list.file(&MatterPath::new(path).unwrap())
        .map(|(path, reason)| (path.as_str().to_owned(), reason))
}

#[test]
fn lines_fold_in_order_and_a_bad_one_is_skipped() {
    let text = format!(
        "{HEADER}add cloud-only Drafts/Scan.pdf\nadd over-size-cap Video.mp4\nadd sideways x\n\
         add cloud-only ../out.txt\nremove Video.mp4\nnonsense\n\nadd over-size-cap \
         Drafts/Scan.pdf\nadd cloud-only Exhibits/A/One.pdf"
    );
    let list = List::decode(&text);
    assert_eq!(
        listed(&list, "Drafts/Scan.pdf"),
        Some(("Drafts/Scan.pdf".to_owned(), LARGE))
    );
    assert_eq!(listed(&list, "Video.mp4"), None);
    assert_eq!(listed(&list, "x"), None);
    assert_eq!(
        list.under(&MatterPath::new("Exhibits").unwrap())
            .map(|(path, _)| path.as_str().to_owned()),
        Some("Exhibits/A/One.pdf".to_owned())
    );
    assert_eq!(list.under(&MatterPath::new("Exhibits/B").unwrap()), None);
    assert_eq!(
        list.under(&MatterPath::new("Exhibits/A/One.pdf").unwrap()),
        None
    );
}

#[test]
fn without_its_header_a_list_names_nothing() {
    for text in [
        "add cloud-only Scan.pdf\n",
        "clauseo-no-copy 2\nadd cloud-only Scan.pdf\n",
        "",
    ] {
        assert_eq!(listed(&List::decode(text), "Scan.pdf"), None, "{text:?}");
    }
}

/// macOS and Windows find one file by another case; the list does too there.
#[test]
fn a_name_compares_as_this_systems_file_system_compares_it() {
    let list = List::decode(&format!("{HEADER}add cloud-only Drafts/Scan.pdf\n"));
    let other = listed(&list, "DRAFTS/scan.PDF");
    if cfg!(any(target_os = "macos", windows)) {
        assert_eq!(other, Some(("Drafts/Scan.pdf".to_owned(), CLOUD)));
    } else {
        assert_eq!(other, None);
    }
}

#[test]
fn the_changes_remove_what_went_and_add_what_came_sorted() {
    let list = List::decode(&format!(
        "{HEADER}add cloud-only Keep.pdf\nadd cloud-only Gone.pdf\nadd cloud-only Grew.mp4\n"
    ));
    let now = [
        file("Keep.pdf", CLOUD),
        file("Grew.mp4", LARGE),
        file("B.pdf", CLOUD),
        file("A.pdf", CLOUD),
        file("Unread/x", SkippedFileReason::UnreadFolder),
    ];
    let changes = list.changes(&now);
    assert_eq!(
        changes,
        "remove Gone.pdf\nadd cloud-only A.pdf\nadd cloud-only B.pdf\nadd over-size-cap Grew.mp4\n"
    );
    let folded = List::decode(&format!(
        "{HEADER}add cloud-only Keep.pdf\nadd cloud-only Gone.pdf\nadd cloud-only Grew.mp4\n\
         {changes}"
    ));
    assert_eq!(folded.changes(&now), "", "nothing more to change");
    assert_eq!(List::default().changes(&[]), "");
}

#[test]
fn at_most_the_first_files_are_listed() {
    let many: Vec<SkippedFile> = (0..=MAX_FILES)
        .map(|n| file(&format!("f{n:05}.pdf"), CLOUD))
        .collect();
    let changes = List::default().changes(&many);
    assert_eq!(changes.lines().count(), MAX_FILES);
    assert!(!changes.contains(&format!("f{MAX_FILES:05}.pdf")));
}

/// A list longer than the guard reads is read up to its last whole line within the limit, never
/// as empty; and the host appends only the whole lines that fit within it.
#[test]
fn a_list_past_the_limit_is_read_to_its_last_whole_line() {
    let limit = usize::try_from(MAX_BYTES).unwrap();
    let mut text = HEADER.to_owned();
    let mut n = 0;
    while text.len() <= limit {
        writeln!(text, "add cloud-only f{n:07}.pdf").unwrap();
        n += 1;
    }
    let kept = within_limit(text.as_bytes());
    assert!(kept.len() <= limit && kept.ends_with(b"\n"));
    let list = List::decode(std::str::from_utf8(kept).unwrap());
    assert!(listed(&list, "f0000000.pdf").is_some(), "read, not empty");
    assert!(
        listed(&list, &format!("f{:07}.pdf", n - 1)).is_none(),
        "the cut line left out"
    );

    let changes = "remove A.pdf\nadd cloud-only B.pdf\n";
    assert_eq!(fitting(changes, 0), changes);
    assert_eq!(fitting(changes, MAX_BYTES - 20), "remove A.pdf\n");
    assert_eq!(fitting(changes, MAX_BYTES - 5), "");
    assert_eq!(fitting(changes, MAX_BYTES + 5), "");
}

/// A cut at the limit that lands inside a character of a non-ASCII name still keeps the whole
/// lines before it.
#[test]
fn a_cut_inside_a_non_ascii_name_keeps_the_lines_before_it() {
    for (first, second) in [
        (
            "add cloud-only Müller.docx\n",
            "add cloud-only Akte/नोटिस.pdf\n",
        ),
        (
            "add cloud-only Akte/नोटिस.pdf\n",
            "add cloud-only Müller.docx\n",
        ),
    ] {
        let changes = format!("{first}{second}");
        let inside = second
            .char_indices()
            .find(|(_, c)| !c.is_ascii())
            .unwrap()
            .0
            + 1;
        assert!(!changes.is_char_boundary(first.len() + inside));
        let room = u64::try_from(first.len() + inside).unwrap();
        assert_eq!(fitting(&changes, MAX_BYTES - room), first, "{changes}");
        let inside = first.char_indices().find(|(_, c)| !c.is_ascii()).unwrap().0 + 1;
        let room = u64::try_from(inside).unwrap();
        assert_eq!(fitting(&changes, MAX_BYTES - room), "", "{changes}");
        let room = u64::try_from(changes.len()).unwrap();
        assert_eq!(fitting(&changes, MAX_BYTES - room), changes);
    }
}

/// F8: the list is read only when the call deletes something; a read, a listing or a write (the
/// hook runs before every tool under forced ask) never pays for it.
#[test]
fn the_list_is_read_only_for_a_call_that_deletes() {
    let made = std::env::temp_dir().join(format!("clauseo-no-copy-lazy-{}", std::process::id()));
    crate::write::create_dir_all(&made).unwrap();
    let matter = dunce::canonicalize(&made).unwrap();
    let calls = std::cell::Cell::new(0);
    let read = || {
        calls.set(calls.get() + 1);
        List::decode(&format!("{HEADER}add cloud-only Free.docx\n"))
    };
    let hook = |tool: &str, input: serde_json::Value| {
        serde_json::json!({"tool_name": tool, "tool_input": input, "cwd": matter}).to_string()
    };
    for (tool, input) in [
        ("Read", serde_json::json!({"file_path": "Free.docx"})),
        ("Bash", serde_json::json!({"command": "ls; cat Free.docx"})),
        (
            "Write",
            serde_json::json!({"file_path": "New.docx", "content": "x"}),
        ),
        ("Bash", serde_json::json!({"command": "echo x > New.docx"})),
    ] {
        assert!(refusal(hook(tool, input).as_bytes(), Some(&matter), read).is_none());
    }
    assert_eq!(calls.get(), 0, "never read without a delete");
    let rm = hook("Bash", serde_json::json!({"command": "rm Free.docx"}));
    assert!(refusal(rm.as_bytes(), Some(&matter), read).is_some());
    assert_eq!(calls.get(), 1);
    crate::write::remove_dir_all(&made).unwrap();
}
