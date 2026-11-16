//! The story views read: stetdoc 0.9.3's real `--story notes` and `--story headers` output on
//! the redlines of the `partial/` pairs, the parts put in order, and the views that do not read.

use clauseo_protocol::BlacklineNotice::{ChangesOutsideText, PartNotShown};
use clauseo_protocol::ParagraphChange::Changed;
use clauseo_protocol::RunKind::{Del, Ins, Same};
use clauseo_protocol::{BlacklinePart, BlacklinePartKind, HeaderSlot, HeaderUse};

use super::super::tests::para;
use super::super::{Read, ViewFailure, read_view};
use super::{Story, StoryRead, add_parts, read_story};

/// A fixture of `tests/fixtures/blackline/partial/`, as text.
macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/blackline/partial/",
            $name
        ))
    };
}

/// `json` read as `story`.
fn read(json: &str, story: Story) -> StoryRead {
    read_story(json, story).unwrap()
}

/// A part of `kind` with one paragraph that reads `before`, `one`/`two`, `after`.
fn part(kind: BlacklinePartKind, before: &str, old: &str, new: &str) -> BlacklinePart {
    BlacklinePart {
        kind,
        paragraphs: vec![para(
            Changed,
            &[(Same, before), (Del, old), (Ins, new), (Same, ".")],
        )],
    }
}

/// A header's or footer's one use.
fn used(slot: HeaderSlot, sections: &[u32]) -> Vec<HeaderUse> {
    vec![HeaderUse {
        slot,
        sections: sections.to_vec(),
    }]
}

#[test]
fn a_footnote_that_changed_is_a_part_with_its_text() {
    let notes = read(fixture!("story-notes-footnote.json"), Story::Notes);
    assert_eq!(
        notes.parts,
        [part(
            BlacklinePartKind::Footnote { number: 1 },
            "Note ",
            "one year",
            "two years"
        )]
    );
    assert_eq!(notes.ids.into_iter().collect::<Vec<_>>(), [2, 3]);
    assert_eq!(notes.notices, []);
    // Its header did not change: no part, no notice.
    let headers = read(fixture!("story-headers-footnote.json"), Story::Headers);
    assert_eq!((headers.parts, headers.notices), (vec![], vec![]));
}

#[test]
fn a_header_that_changed_is_a_part_with_its_text() {
    let headers = read(fixture!("story-headers-header.json"), Story::Headers);
    let header = BlacklinePartKind::Header {
        uses: used(HeaderSlot::Default, &[1]),
    };
    assert_eq!(
        headers.parts,
        [part(header, "Header ", "one year", "two years")]
    );
    assert_eq!(headers.ids.into_iter().collect::<Vec<_>>(), [2, 3]);
    let notes = read(fixture!("story-notes-header.json"), Story::Notes);
    assert_eq!(notes.parts, []);
}

/// Two sections: a footnote and an endnote, a header on both sections and a first-page footer
/// on the second, all changed; then put in order with the body (`view-stories.json`).
#[test]
fn notes_headers_and_footers_of_two_sections() {
    let notes = read(fixture!("story-notes-stories.json"), Story::Notes);
    let headers = read(fixture!("story-headers-stories.json"), Story::Headers);
    let header = BlacklinePartKind::Header {
        uses: used(HeaderSlot::Default, &[1, 2]),
    };
    let footer = BlacklinePartKind::Footer {
        uses: used(HeaderSlot::FirstPage, &[2]),
    };
    let expected = vec![
        part(
            BlacklinePartKind::Footnote { number: 1 },
            "Footnote ",
            "one",
            "two",
        ),
        part(
            BlacklinePartKind::Endnote { number: 1 },
            "Endnote ",
            "one",
            "two",
        ),
        part(header, "Header ", "one", "two"),
        part(footer, "First footer ", "one", "two"),
    ];
    let mut body: Read = read_view(fixture!("view-stories.json")).unwrap();
    // The body names none of the 8 changes its summary counts.
    assert_eq!(body.blackline.notices, [ChangesOutsideText]);
    add_parts(&mut body, vec![headers, notes]);
    assert_eq!(body.blackline.parts, expected);
    assert_eq!(body.ids.len(), 8);
    assert_eq!(body.blackline.notices, []);
}

/// A header in the newer version only: stetdoc's compare marks nothing in it, so it shows no
/// change and is left out (compare's warning says it, `receipt.rs`); a story with nothing in
/// it gives nothing.
#[test]
fn a_part_with_no_change_is_left_out() {
    let headers = read(fixture!("story-headers-unmatched.json"), Story::Headers);
    assert_eq!((headers.parts, headers.notices), (vec![], vec![]));
    for (json, story) in [
        (fixture!("story-notes-cellfmt-body.json"), Story::Notes),
        (fixture!("story-headers-cellfmt-body.json"), Story::Headers),
    ] {
        assert_eq!(read(json, story), StoryRead::default());
    }
}

/// A part cut short by stetdoc (a page that does not show every block) says part of it is not
/// shown.
#[test]
fn a_story_cut_short_is_a_part_not_shown() {
    let mut json: serde_json::Value =
        serde_json::from_str(fixture!("story-notes-footnote.json")).unwrap();
    json["position"]["total"] = serde_json::json!(2);
    let notes = read(&json.to_string(), Story::Notes);
    assert_eq!(notes.notices, [PartNotShown]);
    assert_eq!(notes.parts.len(), 1);
}

/// A view of another story, the body's or one that is not a redline, is not read.
#[test]
fn a_view_of_another_story_does_not_read() {
    let notes = fixture!("story-notes-footnote.json");
    assert!(matches!(
        read_story(notes, Story::Headers),
        Err(ViewFailure::NotAView(_))
    ));
    assert!(matches!(
        read_story(fixture!("view-stories.json"), Story::Notes),
        Err(ViewFailure::NotAView(_))
    ));
    let final_mode = notes.replace("\"mode\": \"redline\"", "\"mode\": \"final\"");
    assert!(read_story(&final_mode, Story::Notes).is_err());
    assert!(read_story("{}", Story::Notes).is_err());
}
