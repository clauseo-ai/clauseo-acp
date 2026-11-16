//! The body's ids against stetdoc's own list (`check --json`), on stetdoc 0.9.3's real output:
//! a change in a footnote alone and in a header alone, the lease (its footnote changed), and a
//! document that holds a cell's and a section's format change the view never marks; and the
//! footnote and header shown in their parts (`story.rs`).

use clauseo_protocol::BlacklineNotice::{CellFormatChanged, ChangesOutsideText, MayDiffer};

use super::super::story::{Story, add_parts, read_story};
use super::super::{Read, read_view};
use super::{Change, apply, read_check};

/// A fixture of `tests/fixtures/blackline/`, as text.
macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/blackline/",
            $name
        ))
    };
}

/// The view `view` read, then checked against the list `check`.
fn checked(view: &str, check: &str) -> Read {
    let mut read = read_view(view).unwrap();
    apply(&mut read, &read_check(check).unwrap());
    read
}

/// A change in a footnote alone, or a header alone: the body is unchanged and the notice that
/// changes outside the main text aren't shown comes, from the list.
#[test]
fn a_footnote_or_header_change_alone_is_noted() {
    for (view, check) in [
        (
            fixture!("partial/view-footnote.json"),
            fixture!("partial/check-footnote.json"),
        ),
        (
            fixture!("partial/view-header.json"),
            fixture!("partial/check-header.json"),
        ),
    ] {
        let read = checked(view, check);
        assert_eq!(read.blackline.notices, [ChangesOutsideText]);
        assert!(read.ids.is_empty());
    }
}

/// The same changes once the part that holds each is read: its ids are shown, and nothing is
/// said to be outside what the blackline shows.
#[test]
fn a_footnote_or_header_change_shown_in_its_part_is_covered() {
    for (view, story, kind, check) in [
        (
            fixture!("partial/view-footnote.json"),
            fixture!("partial/story-notes-footnote.json"),
            Story::Notes,
            fixture!("partial/check-footnote.json"),
        ),
        (
            fixture!("partial/view-header.json"),
            fixture!("partial/story-headers-header.json"),
            Story::Headers,
            fixture!("partial/check-header.json"),
        ),
    ] {
        let mut read = read_view(view).unwrap();
        add_parts(&mut read, vec![read_story(story, kind).unwrap()]);
        apply(&mut read, &read_check(check).unwrap());
        assert_eq!(read.blackline.notices, [], "{kind:?}");
        assert_eq!(read.blackline.parts.len(), 1, "{kind:?}");
    }
}

/// The lease: the list's ids 2 to 17, the body names 2 to 15, and the footnote's 16 and 17 are
/// outside it.
#[test]
fn the_lease_s_footnote_is_outside_the_main_text() {
    let read = checked(fixture!("view.json"), fixture!("check.json"));
    assert_eq!(read.blackline.notices, [ChangesOutsideText]);
    assert_eq!(read.ids.len(), 14);
}

/// The summary counts a move as one change and the view names its two ids, so the count alone
/// can't tell; the list says which are missing: the cell's format change (a notice of its own)
/// and the last section's (may differ), and nothing is outside the main text.
#[test]
fn a_cell_s_format_change_is_named_and_the_summary_s_guess_replaced() {
    let view = fixture!("partial/view-props.json");
    let summary = read_view(view).unwrap();
    assert!(summary.blackline.notices.contains(&ChangesOutsideText));
    let read = checked(view, fixture!("partial/check-props.json"));
    assert_eq!(read.blackline.notices, [CellFormatChanged, MayDiffer]);
}

/// A change the body names that the list does not hold: the two disagree, so may differ. And a
/// page cut short names fewer: no notice about the rest.
#[test]
fn a_disagreement_may_differ_and_a_cut_page_says_nothing_more() {
    let view = fixture!("view.json");
    let only = |ids: &[u64], kind: &str| Change {
        ids: ids.to_vec(),
        kind: kind.to_owned(),
    };
    let mut read = read_view(view).unwrap();
    apply(&mut read, &[only(&[2, 3], "paragraph_deleted")]);
    assert_eq!(read.blackline.notices, [MayDiffer]);

    let mut cut = read_view(view).unwrap();
    cut.blackline.truncated = true;
    let mut all: Vec<Change> = read_check(fixture!("check.json")).unwrap();
    all.push(only(&[99], "cell_format"));
    apply(&mut cut, &all);
    assert_eq!(cut.blackline.notices, [CellFormatChanged]);
}

#[test]
fn a_check_that_is_not_a_list_does_not_read() {
    assert!(read_check("{}").is_err());
    assert!(read_check("not json").is_err());
    assert_eq!(read_check(r#"{"existing": []}"#).unwrap(), []);
}
