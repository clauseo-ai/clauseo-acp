//! Cursors: every match once, in order, across pages; a cursor belongs to one query.

use std::collections::HashSet;

use clauseo_search::{Cursor, Error, Mode, Query, SearchIndex};

use crate::common::{TestDir, answer, index, keys, query};

fn thirty(index: &mut SearchIndex) {
    for t in 1..=30 {
        answer(index, 1, t, t, &format!("rent notice number {t}"));
    }
}

/// Every page of `first`, seven hits at a time, calling `between` after the first page.
fn all_pages(
    index: &mut SearchIndex,
    first: Query,
    between: impl FnOnce(&mut SearchIndex),
) -> Vec<String> {
    let mut seen = Vec::new();
    let mut query = Query { limit: 7, ..first };
    let mut between = Some(between);
    loop {
        let page = index.query(&query).unwrap();
        assert!(page.hits.len() <= 7);
        seen.extend(keys(&page).into_iter().map(|key| format!("{key:?}")));
        if let Some(between) = between.take() {
            between(index);
        }
        match page.next {
            // Through text and back, as the app will send it.
            Some(cursor) => query.cursor = Some(Cursor::decode(&cursor.encode()).unwrap()),
            None => break,
        }
    }
    seen
}

#[test]
fn typing_pages_newest_first_through_every_match_once() {
    let dir = TestDir::new("paging-typing");
    let mut index = index(&dir);
    thirty(&mut index);
    let seen = all_pages(&mut index, query("rent noti", Mode::Typing), |_| {});
    assert_eq!(seen.len(), 30);
    assert_eq!(seen.iter().collect::<HashSet<_>>().len(), 30);
    let newest: Vec<String> = (1..=30)
        .rev()
        .map(|t| format!("{:?}", (Some(crate::common::thread(t)), Some(t))))
        .collect();
    assert_eq!(seen, newest);
}

#[test]
fn ranked_pages_are_stable_while_rows_arrive() {
    let dir = TestDir::new("paging-ranked");
    let mut index = index(&dir);
    thirty(&mut index);
    let quiet = all_pages(&mut index, query("rent notice ", Mode::Ranked), |_| {});
    assert_eq!(quiet.len(), 30);
    assert_eq!(quiet.iter().collect::<HashSet<_>>().len(), 30);
    // Rows put after the first page do not shift the later pages.
    let busy = all_pages(&mut index, query("rent notice ", Mode::Ranked), |index| {
        for t in 31..=40 {
            answer(index, 1, t, t, "rent notice again");
        }
    });
    assert_eq!(busy, quiet);
}

#[test]
fn a_cursor_used_with_another_query_starts_over() {
    let dir = TestDir::new("paging-cursor");
    let mut index = index(&dir);
    thirty(&mut index);
    let first = Query {
        limit: 5,
        ..query("rent ", Mode::Ranked)
    };
    let cursor = index.query(&first).unwrap().next;
    assert!(cursor.is_some());
    for other in [
        Query {
            limit: 5,
            ..query("notice ", Mode::Ranked)
        },
        Query {
            include_archived: true,
            ..first.clone()
        },
        Query {
            mode: Mode::Typing,
            ..first.clone()
        },
    ] {
        let fresh = index.query(&other).unwrap();
        let with_cursor = index
            .query(&Query {
                cursor,
                ..other.clone()
            })
            .unwrap();
        assert_eq!(with_cursor, fresh, "{other:?}");
    }
    // With its own query, the cursor gives the second page.
    let second = index
        .query(&Query {
            cursor,
            ..first.clone()
        })
        .unwrap();
    assert_ne!(keys(&second), keys(&index.query(&first).unwrap()));
}

#[test]
fn garbled_cursor_text_is_an_error_not_a_panic() {
    for text in ["", "AAAA", "not a cursor", "\u{0}", &"_".repeat(10_000)] {
        assert!(
            matches!(Cursor::decode(text), Err(Error::Cursor)),
            "{text:.20}"
        );
    }
}
