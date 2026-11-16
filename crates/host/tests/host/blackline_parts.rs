//! The Word blackline through the host for its notes, headers and footers, what compare says
//! beside the redline, and table formatting: the footnotes, endnotes, headers and footers of two
//! sections, each shown in its part; a header in one version only beside a body change, which
//! compare warns of and does not mark; a block compare could not mark up; and a cell's shading
//! beside a body change, which compare does not mark and Clauseo's own comparison of the tables
//! names. The stand-in stetdoc answers stetdoc 0.9.3's real output
//! (`tests/fixtures/blackline/partial/`, `make_stories.py`'s pairs); the real stetdoc runs when
//! one is on `PATH`.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::BlacklineNotice::{CellFormatChanged, HeaderOrFooterNotCompared, MayDiffer};
use clauseo_protocol::{
    BlacklinePartKind, ChangedTable, HeaderSlot, HeaderUse, ParagraphChange, TableFormat,
    TablePlace, WordBlackline,
};

use crate::blackline::{
    DOC, blackline, fixture, pair_matter_launching, query, stetdoc_on_path, temp_entries,
};
use crate::blackline_crossed::through_fake;
use crate::support::{Dir, FAKE, Scenario};

/// The body's first table, its shading changed.
fn shaded() -> ChangedTable {
    ChangedTable {
        place: TablePlace::Body,
        number: 1,
        formats: vec![TableFormat::Shading],
    }
}

/// Whether `answer`'s body shows a change.
fn body_changed(answer: &WordBlackline) -> bool {
    answer
        .paragraphs
        .iter()
        .any(|p| p.change == ParagraphChange::Changed)
}

/// The kinds of `answer`'s parts.
fn kinds(answer: &WordBlackline) -> Vec<BlacklinePartKind> {
    answer.parts.iter().map(|part| part.kind.clone()).collect()
}

/// The parts of the two-section pair, in their order: the footnote, the endnote, the header on
/// both sections, the first-page footer on the second.
fn two_sections() -> Vec<BlacklinePartKind> {
    vec![
        BlacklinePartKind::Footnote { number: 1 },
        BlacklinePartKind::Endnote { number: 1 },
        BlacklinePartKind::Header {
            uses: vec![HeaderUse {
                slot: HeaderSlot::Default,
                sections: vec![1, 2],
            }],
        },
        BlacklinePartKind::Footer {
            uses: vec![HeaderUse {
                slot: HeaderSlot::FirstPage,
                sections: vec![2],
            }],
        },
    ]
}

#[test]
fn notes_headers_and_footers_of_two_sections_are_shown_in_order() {
    let (answer, ..) = through_fake("blackline_parts_two_sections", "stories", None, &[]);
    assert_eq!(kinds(&answer), two_sections());
    assert!(answer.parts.iter().all(|part| part.paragraphs.len() == 1));
    assert_eq!(answer.notices, []);
}

/// A header in the newer version only, beside a body change: compare warns and marks nothing
/// for it, and the notice is said even though the body shows a change.
#[test]
fn a_header_in_one_version_beside_a_body_change_is_noticed() {
    let (answer, ..) = through_fake("blackline_parts_unmatched", "unmatched", None, &[]);
    assert!(body_changed(&answer));
    assert_eq!(answer.parts, []);
    assert_eq!(answer.notices, [HeaderOrFooterNotCompared]);
}

/// A header in the newer version only that compare does not warn of, when the headers' view
/// fails: stetdoc's list of changes names nothing in it (it holds no marked change), so the
/// blackline still says, from the header's text, that it was not compared.
#[test]
fn a_header_compare_is_silent_on_is_noticed_when_the_headers_view_fails() {
    let (answer, ..) = through_fake(
        "blackline_parts_unmatched_unviewed",
        "unmatched",
        None,
        &[
            ("compare.json", br#"{"receipt":{"warnings":[]}}"#),
            ("view-story-mode.txt", b"exit 2"),
        ],
    );
    assert!(body_changed(&answer));
    assert_eq!(answer.notices, [HeaderOrFooterNotCompared]);
}

/// compare's answer that lists a block it could not mark up, or that does not read, may differ.
#[test]
fn what_compare_could_not_mark_up_may_differ() {
    let (answer, ..) = through_fake("blackline_parts_excluded", "excluded", None, &[]);
    assert_eq!(answer.notices, [MayDiffer]);
    let (unread, ..) = through_fake(
        "blackline_parts_compare_unread",
        "unmatched",
        None,
        &[("compare.json", b"{}")],
    );
    // The header the newer version adds is still caught, by its text, with no warning to read.
    assert_eq!(unread.notices, [HeaderOrFooterNotCompared, MayDiffer]);
}

/// A cell's shading changed beside a body change: compare marks only the body's; the table is
/// named, with the table notice.
#[test]
fn a_cells_shading_beside_a_body_change_is_named() {
    let (answer, ..) = through_fake("blackline_parts_cellfmt", "cellfmt-body", None, &[]);
    assert!(body_changed(&answer));
    assert_eq!(answer.notices, [CellFormatChanged]);
    assert_eq!(answer.changed_tables, [shaded()]);
}

/// The blackline of the pair `<pair>-old.docx`, `<pair>-new.docx` of `partial/` through the
/// real stetdoc `stetdoc`.
fn through_real(stetdoc: &std::path::Path, pair: &str) -> WordBlackline {
    let dir = Dir::new(
        &format!("blackline_real_parts_{pair}"),
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let launcher = PassthroughLauncher::new(
        SpawnOptions::new()
            .home(dir.home().join("home"))
            .clear_env(),
    );
    let (old, new) = (
        fixture(&format!("partial/{pair}-old.docx")),
        fixture(&format!("partial/{pair}-new.docx")),
    );
    let (host, matter, from, _folder) =
        pair_matter_launching(&dir, Some(stetdoc), Arc::new(launcher), &old, &new);
    let answer = blackline(host.query(query(matter, DOC, from, None)));
    assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
    host.shutdown();
    answer
}

/// With a real stetdoc on `PATH` (0.9.3, the pin): the same pairs, and a cell's shading alone,
/// which stetdoc marks nothing for, named by the table comparison.
#[test]
fn the_real_stetdoc_shows_the_parts_and_names_what_compare_misses() {
    let Some(stetdoc) = stetdoc_on_path() else {
        eprintln!("skipped: no stetdoc on PATH");
        return;
    };
    let stories = through_real(&stetdoc, "stories");
    assert_eq!((kinds(&stories), stories.notices), (two_sections(), vec![]));
    let unmatched = through_real(&stetdoc, "unmatched");
    assert!(body_changed(&unmatched));
    assert_eq!(unmatched.notices, [HeaderOrFooterNotCompared]);
    assert_eq!(through_real(&stetdoc, "excluded").notices, [MayDiffer]);
    for pair in ["cellfmt-body", "cellfmt"] {
        let answer = through_real(&stetdoc, pair);
        assert_eq!(answer.notices, [CellFormatChanged], "{pair}");
        assert_eq!(answer.changed_tables, [shaded()], "{pair}");
    }
    // A cover section inserted first, with its own header: the old header, whose table's
    // shading changed, is on section 2 now and pairs with itself. compare can't mark up the
    // inserted paragraph that ends the cover section, which may differ.
    // Its header is in the newer version only and stetdoc's compare pairs it with nothing, nor
    // warns of it: the blackline says it was not compared.
    let cover = through_real(&stetdoc, "cover");
    assert_eq!(
        cover.notices,
        [HeaderOrFooterNotCompared, CellFormatChanged, MayDiffer]
    );
    let on_section_2 = ChangedTable {
        place: TablePlace::Header {
            uses: vec![HeaderUse {
                slot: HeaderSlot::Default,
                sections: vec![2],
            }],
        },
        ..shaded()
    };
    assert_eq!(cover.changed_tables, [on_section_2]);
    // A section added in the newer version (an existing paragraph now ends it) with a header of
    // its own, beside a body change: stetdoc's compare neither compares that header nor warns,
    // and its view shows the header unchanged.
    let added = through_real(&stetdoc, "added");
    assert!(body_changed(&added));
    assert_eq!(
        (added.parts, added.notices),
        (vec![], vec![HeaderOrFooterNotCompared])
    );
}
