//! Puts replace the row with the same key; deletes remove rows from what a query finds.

use clauseo_search::{EntryKind, Kind, Mode};

use crate::common::{TestDir, answer, at, count, find, index, matter, seq, snippet, thread};

#[test]
fn a_put_replaces_the_entry_with_the_same_thread_and_entry() {
    let dir = TestDir::new("replace");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "the first draft");
    answer(&mut index, 1, 1, 1, "the second version");
    assert_eq!(count(&index, "draft ", Mode::Ranked), 0);
    assert_eq!(find(&index, "version ", Mode::Ranked).hits.len(), 1);
    index
        .put_entry(
            matter(1),
            thread(1),
            seq(1),
            EntryKind::Prompt,
            at(1),
            "a prompt now",
        )
        .unwrap();
    let page = find(&index, "prompt ", Mode::Ranked);
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].kind, Kind::Prompt);
    assert_eq!(count(&index, "version ", Mode::Ranked), 0);
}

#[test]
fn delete_thread_removes_its_rows_and_title() {
    let dir = TestDir::new("delete-thread");
    let mut index = index(&dir);
    index
        .put_thread_title(matter(1), thread(1), at(0), "Bail matter")
        .unwrap();
    answer(&mut index, 1, 1, 1, "bail granted");
    answer(&mut index, 1, 2, 1, "bail refused");
    index.delete_thread(thread(1)).unwrap();
    let page = find(&index, "bail ", Mode::Ranked);
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].thread, Some(thread(2)));
    assert_eq!(page.hits[0].thread_title, None);
    assert_eq!(count(&index, "granted", Mode::Typing), 0);
}

#[test]
fn delete_matter_removes_its_rows_threads_and_names() {
    let dir = TestDir::new("delete-matter");
    let mut index = index(&dir);
    index
        .put_matter_title(matter(1), at(0), "Cheque bounce")
        .unwrap();
    index
        .put_file_names(matter(1), at(0), ["Cheque copy.pdf"])
        .unwrap();
    answer(&mut index, 1, 1, 1, "the cheque was dishonoured");
    answer(&mut index, 2, 2, 1, "another cheque");
    index.delete_matter(matter(1)).unwrap();
    let page = find(&index, "cheque ", Mode::Ranked);
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].matter, matter(2));
    assert_eq!(count(&index, "dishonoured ", Mode::Ranked), 0);
    assert_eq!(count(&index, "bounce", Mode::Typing), 0);
}

#[test]
fn delete_entries_removes_an_undone_steps_rows_only() {
    let dir = TestDir::new("delete-entries");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "lease kept");
    answer(&mut index, 1, 1, 5, "lease undone");
    index
        .put_entry(
            matter(1),
            thread(1),
            seq(6),
            EntryKind::ToolTitle,
            at(6),
            "lease tool",
        )
        .unwrap();
    answer(&mut index, 1, 2, 5, "lease other thread");
    index.delete_entries(thread(1), [seq(5), seq(6)]).unwrap();
    let mut left: Vec<String> = find(&index, "lease ", Mode::Ranked)
        .hits
        .iter()
        .map(snippet)
        .collect();
    left.sort();
    assert_eq!(left, vec!["lease kept", "lease other thread"]);
}

#[test]
fn put_file_names_replaces_the_matters_names() {
    let dir = TestDir::new("file-names");
    let mut index = index(&dir);
    index
        .put_file_names(matter(1), at(0), ["Plaint.pdf", "Affidavit.docx"])
        .unwrap();
    index
        .put_file_names(matter(2), at(0), ["Plaint copy.pdf"])
        .unwrap();
    index
        .put_file_names(matter(1), at(1), ["drafts/Reply.docx"])
        .unwrap();
    assert_eq!(count(&index, "affidavit ", Mode::Ranked), 0);
    let plaint = find(&index, "plaint ", Mode::Ranked);
    assert_eq!(plaint.hits.len(), 1);
    assert_eq!(plaint.hits[0].matter, matter(2));
    let reply = find(&index, "reply ", Mode::Ranked);
    assert_eq!(reply.hits[0].kind, Kind::FileName);
    assert_eq!(reply.hits[0].path.as_deref(), Some("drafts/Reply.docx"));
    // A folder name in the path is a word too.
    assert_eq!(find(&index, "drafts ", Mode::Ranked).hits.len(), 1);
}

#[test]
fn snippet_markers_in_stored_text_are_stripped() {
    let dir = TestDir::new("markers");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "a \u{2}forged\u{3} mark");
    let hit = &find(&index, "mark ", Mode::Ranked).hits[0];
    assert_eq!(snippet(hit), "a forged mark");
    assert_eq!(crate::common::marked(hit), vec!["mark"]);
}
