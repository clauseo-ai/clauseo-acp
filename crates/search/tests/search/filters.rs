//! Scope, archived, kinds, and the titles each hit carries.

use clauseo_search::{EntryKind, Kind, Mode, Query};

use crate::common::{TestDir, answer, at, count, find, index, matter, query, seq, thread};

fn ask(index: &clauseo_search::SearchIndex, change: impl FnOnce(&mut Query)) -> usize {
    let mut query = query("lease ", Mode::Ranked);
    change(&mut query);
    index.query(&query).unwrap().hits.len()
}

#[test]
fn a_matter_scope_keeps_that_matter_only() {
    let dir = TestDir::new("scope");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "the lease ends");
    answer(&mut index, 2, 2, 1, "the lease starts");
    let page = index
        .query(&Query {
            matter: Some(matter(2)),
            ..query("lease ", Mode::Ranked)
        })
        .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].matter, matter(2));
    assert_eq!(ask(&index, |q| q.matter = Some(matter(9))), 0);
    assert_eq!(ask(&index, |_| {}), 2);
}

#[test]
fn archived_rows_are_hidden_unless_asked_for_and_follow_later_puts() {
    let dir = TestDir::new("archived");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "the lease ends");
    answer(&mut index, 1, 2, 1, "the lease starts");
    index.set_archived(matter(1), thread(1), true).unwrap();
    // An entry put after archiving is archived too.
    answer(&mut index, 1, 1, 2, "a second lease");
    assert_eq!(ask(&index, |_| {}), 1);
    assert_eq!(ask(&index, |q| q.include_archived = true), 3);
    for mode in [Mode::Typing, Mode::Ranked] {
        let hidden = index.query(&query("lease", mode)).unwrap();
        assert!(
            hidden.hits.iter().all(|hit| hit.thread == Some(thread(2))),
            "{mode:?}"
        );
    }
    index.set_archived(matter(1), thread(1), false).unwrap();
    assert_eq!(ask(&index, |_| {}), 3);
}

#[test]
fn archiving_a_conversation_with_no_rows_yet_holds_for_later_puts() {
    let dir = TestDir::new("archived-empty");
    let mut index = index(&dir);
    index.set_archived(matter(1), thread(1), true).unwrap();
    answer(&mut index, 1, 1, 1, "the lease ends");
    assert_eq!(ask(&index, |_| {}), 0);
    assert_eq!(ask(&index, |q| q.include_archived = true), 1);
}

#[test]
fn kinds_filter_and_an_empty_list_matches_nothing() {
    let dir = TestDir::new("kinds");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "the lease ends");
    index
        .put_entry(
            matter(1),
            thread(1),
            seq(2),
            EntryKind::ToolTitle,
            at(2),
            "Read lease.docx",
        )
        .unwrap();
    index
        .put_file_names(matter(1), at(3), ["Lease deed.pdf"])
        .unwrap();
    assert_eq!(ask(&index, |_| {}), 3);
    assert_eq!(ask(&index, |q| q.kinds = Some(vec![Kind::ToolTitle])), 1);
    assert_eq!(
        ask(&index, |q| q.kinds =
            Some(vec![Kind::FileName, Kind::Answer])),
        2
    );
    assert_eq!(ask(&index, |q| q.kinds = Some(Vec::new())), 0);
}

#[test]
fn each_hit_carries_its_current_matter_and_thread_titles() {
    let dir = TestDir::new("titles");
    let mut index = index(&dir);
    index
        .put_matter_title(matter(1), at(0), "Sharma v Gupta")
        .unwrap();
    index
        .put_thread_title(matter(1), thread(1), at(0), "Draft notice")
        .unwrap();
    answer(&mut index, 1, 1, 1, "the lease ends");
    index
        .put_file_names(matter(1), at(1), ["Plaint.pdf"])
        .unwrap();
    let hit = &find(&index, "lease ", Mode::Ranked).hits[0];
    assert_eq!(hit.matter_title.as_deref(), Some("Sharma v Gupta"));
    assert_eq!(hit.thread_title.as_deref(), Some("Draft notice"));
    let file = &find(&index, "plaint ", Mode::Ranked).hits[0];
    assert_eq!(file.matter_title.as_deref(), Some("Sharma v Gupta"));
    assert_eq!(file.thread_title, None);
    assert_eq!(file.path.as_deref(), Some("Plaint.pdf"));

    index
        .put_thread_title(matter(1), thread(1), at(5), "Reply to notice")
        .unwrap();
    index
        .put_matter_title(matter(1), at(5), "Sharma v Gupta (2024)")
        .unwrap();
    let hit = &find(&index, "lease ", Mode::Ranked).hits[0];
    assert_eq!(hit.thread_title.as_deref(), Some("Reply to notice"));
    assert_eq!(hit.matter_title.as_deref(), Some("Sharma v Gupta (2024)"));
    // The old titles are no longer found; the new ones are, as titles.
    assert_eq!(count(&index, "draft ", Mode::Ranked), 0);
    let title = &find(&index, "reply ", Mode::Ranked).hits[0];
    assert_eq!(title.kind, Kind::ThreadTitle);
}
