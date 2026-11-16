//! The Word blackline through the host when stetdoc's compare marks no change for what changed,
//! or its view does not show it: a link's new address, a comment's new text, a text box's text,
//! and a change in a footnote alone or a header alone, shown in its part. The stand-in stetdoc
//! answers stetdoc 0.9.3's real output (`tests/fixtures/blackline/partial/`): compare's answer,
//! the redline's view and its notes' and headers', its `check --json`, and each version's view;
//! the real stetdoc runs when one is on `PATH`. More in `blackline_parts.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::BlacklineNotice::{
    self, ChangesOutsideText, CommentChanged, HeaderOrFooterNotCompared, MayDiffer, TextBoxChanged,
};
use clauseo_protocol::{
    BlacklineParagraph, BlacklinePart, BlacklinePartKind, BlacklineRun, HeaderSlot, HeaderUse,
    ParagraphChange, RunKind, WordBlackline,
};

use crate::blackline::{
    DOC, blackline, fake_stetdoc, fixture, pair_matter_launching, query, stetdoc_on_path,
    temp_entries,
};
use crate::support::{Dir, FAKE, Scenario};
use crate::write;

/// A fixture of `partial/`.
fn partial(name: &str) -> Vec<u8> {
    fixture(&format!("partial/{name}"))
}

/// The blackline of the pair `<pair>-old.docx`, `<pair>-new.docx` (`old` and `new` when given),
/// through the stand-in stetdoc answering the pair's fixtures: `compare-<pair>.json`,
/// `view-<pair>.json`, `story-notes-<pair>.json`, `story-headers-<pair>.json`,
/// `check-<pair>.json` and `final-<pair>-old.json`, `final-<pair>-new.json` where there are
/// some, and `extra` files (name, bytes) beside it. With the answer, the stand-in's folder and
/// the test's, which holds it until dropped.
pub(crate) fn through_fake(
    name: &str,
    pair: &str,
    sides: Option<(&[u8], &[u8])>,
    extra: &[(&str, &[u8])],
) -> (WordBlackline, PathBuf, Dir) {
    let dir = Dir::new(name, &[(FAKE, Scenario::Bundled("prompt-stream"))]);
    let stetdoc = fake_stetdoc(&dir);
    let fake = stetdoc.parent().unwrap().to_owned();
    let fixtures = crate::blackline::fixtures().join("partial");
    for (from, to) in [
        (format!("compare-{pair}.json"), "compare.json"),
        (format!("view-{pair}.json"), "view.json"),
        (format!("story-notes-{pair}.json"), "view-story-notes.json"),
        (
            format!("story-headers-{pair}.json"),
            "view-story-headers.json",
        ),
        (format!("check-{pair}.json"), "check.json"),
        (format!("final-{pair}-old.json"), "view-old.json"),
        (format!("final-{pair}-new.json"), "view-new.json"),
    ] {
        if fixtures.join(&from).exists() {
            write::write(&fake.join(to), &partial(&from));
        }
    }
    for (file, bytes) in extra {
        write::write(&fake.join(file), bytes);
    }
    let (old, new) = (
        partial(&format!("{pair}-old.docx")),
        partial(&format!("{pair}-new.docx")),
    );
    let (old, new) = sides.unwrap_or((&old, &new));
    let (host, matter, from, _folder) =
        pair_matter_launching(&dir, Some(&stetdoc), Arc::new(dir.launcher()), old, new);
    let answer = blackline(host.query(query(matter, DOC, from, None)));
    assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
    host.shutdown();
    (answer, fake, dir)
}

/// The bank-details pair: only the address behind "the account" changes (acct-111 to acct-999).
/// stetdoc marks nothing; the blackline names the link, and says nothing else. The same file on
/// both sides names nothing, and no version is viewed.
#[test]
fn a_link_whose_address_alone_changed_is_named() {
    let (answer, fake, _dir) = through_fake("blackline_link_named", "link", None, &[]);
    assert_eq!(answer.changed_links, ["the account"]);
    assert_eq!(answer.notices, []);
    assert!(
        answer
            .paragraphs
            .iter()
            .all(|p| p.change == ParagraphChange::Same)
    );
    let args = std::fs::read_to_string(fake.join("args-check.txt")).unwrap();
    assert!(
        args.starts_with("check\n--json\n--author=Clauseo\n"),
        "{args}"
    );
    let args = std::fs::read_to_string(fake.join("args-view-old.txt")).unwrap();
    assert!(
        args.starts_with("view\n--all\n--json\n--mode\nfinal\n"),
        "{args}"
    );

    let new = partial("link-new.docx");
    let (same, fake, _dir) =
        through_fake("blackline_link_same_file", "link", Some((&new, &new)), &[]);
    assert_eq!((same.changed_links.len(), same.notices.len()), (0, 0));
    assert!(!fake.join("args-view-old.txt").exists());
}

#[test]
fn a_comment_whose_text_changed_is_noted() {
    let (answer, ..) = through_fake("blackline_comment", "comment", None, &[]);
    assert_eq!(answer.notices, [CommentChanged]);
}

#[test]
fn a_text_box_whose_text_changed_is_noted() {
    let (answer, ..) = through_fake("blackline_textbox", "textbox", None, &[]);
    assert_eq!(answer.notices, [TextBoxChanged]);
    assert_eq!(answer.paragraphs[1].change, ParagraphChange::Changed);
}

/// A change in a footnote alone, or a header alone, is shown in its part, with its inserted and
/// deleted text, and nothing is said to be outside what the blackline shows: by stetdoc's list
/// of ids, or by the summary's count when there is no list. When the story's view fails, the
/// part is not shown and the notice says so, by the list and by the count.
#[test]
fn a_footnote_or_header_change_alone_is_shown_in_its_part() {
    for (pair, kind, before) in [
        (
            "footnote",
            BlacklinePartKind::Footnote { number: 1 },
            "Note ",
        ),
        ("header", header_on(&[1]), "Header "),
    ] {
        let (answer, fake, _dir) = through_fake(&format!("blackline_{pair}_only"), pair, None, &[]);
        assert_eq!(answer.notices, [], "{pair}");
        assert_eq!(answer.parts, [changed_part(kind, before)], "{pair}");
        assert!(
            answer
                .paragraphs
                .iter()
                .all(|p| p.change == ParagraphChange::Same)
        );
        let args = std::fs::read_to_string(fake.join("args-view-story-notes.txt")).unwrap();
        assert!(
            args.starts_with("view\n--all\n--json\n--story\nnotes\n"),
            "{args}"
        );
        let (by_summary, ..) = through_fake(
            &format!("blackline_{pair}_summary"),
            pair,
            None,
            &[("check.json", b"not a list")],
        );
        assert_eq!(by_summary.notices, [], "{pair}");
        for (check, label) in [(None, "list"), (Some(&b"not a list"[..]), "count")] {
            let mut extra: Vec<(&str, &[u8])> = vec![("view-story-mode.txt", b"exit 2")];
            extra.extend(check.map(|check| ("check.json", check)));
            let (unshown, ..) = through_fake(
                &format!("blackline_{pair}_unshown_{label}"),
                pair,
                None,
                &extra,
            );
            // A header's text no shown part holds also says it was not compared: true, as no
            // part shows it.
            let expected: &[BlacklineNotice] = if pair == "header" {
                &[ChangesOutsideText, HeaderOrFooterNotCompared]
            } else {
                &[ChangesOutsideText]
            };
            assert_eq!(unshown.notices, expected, "{pair} {label}");
            assert_eq!(unshown.parts, [], "{pair} {label}");
        }
    }
}

/// A header on the first kind of page of `sections`.
pub(crate) fn header_on(sections: &[u32]) -> BlacklinePartKind {
    BlacklinePartKind::Header {
        uses: vec![HeaderUse {
            slot: HeaderSlot::Default,
            sections: sections.to_vec(),
        }],
    }
}

/// The part `kind` of the footnote and header pairs: one paragraph, `before` then "one year"
/// deleted and "two years" inserted.
pub(crate) fn changed_part(kind: BlacklinePartKind, before: &str) -> BlacklinePart {
    let run = |kind, text: &str| BlacklineRun {
        kind,
        text: text.to_owned(),
        object: None,
    };
    BlacklinePart {
        kind,
        paragraphs: vec![BlacklineParagraph {
            change: ParagraphChange::Changed,
            runs: vec![
                run(RunKind::Same, before),
                run(RunKind::Del, "one year"),
                run(RunKind::Ins, "two years"),
                run(RunKind::Same, "."),
            ],
            hidden: false,
        }],
    }
}

/// A version's view that does not read: the links and comments can't be compared, so the
/// blackline, still shown, may differ.
#[test]
fn versions_that_can_not_be_compared_may_differ() {
    let (answer, ..) = through_fake(
        "blackline_sides_unread",
        "link",
        None,
        &[("view-new.json", b"{}")],
    );
    assert_eq!(answer.notices, [MayDiffer]);
    assert_eq!(answer.changed_links, Vec::<String>::new());
}

/// With a real stetdoc on `PATH`: the link, comment, text box, footnote and header pairs.
#[test]
fn the_real_stetdoc_names_what_its_compare_does_not_mark() {
    let Some(stetdoc) = stetdoc_on_path() else {
        eprintln!("skipped: no stetdoc on PATH");
        return;
    };
    let run = |pair: &str| {
        let dir = Dir::new(
            &format!("blackline_real_crossed_{pair}"),
            &[(FAKE, Scenario::Bundled("prompt-stream"))],
        );
        let launcher = PassthroughLauncher::new(
            SpawnOptions::new()
                .home(dir.home().join("home"))
                .clear_env(),
        );
        let (old, new) = (
            partial(&format!("{pair}-old.docx")),
            partial(&format!("{pair}-new.docx")),
        );
        let (host, matter, from, _folder) =
            pair_matter_launching(&dir, Some(&stetdoc), Arc::new(launcher), &old, &new);
        let answer = blackline(host.query(query(matter, DOC, from, None)));
        assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
        host.shutdown();
        answer
    };
    let link = run("link");
    assert_eq!(
        (link.changed_links, link.notices),
        (vec!["the account".to_owned()], vec![])
    );
    assert_eq!(run("comment").notices, [CommentChanged]);
    assert_eq!(run("textbox").notices, [TextBoxChanged]);
    let footnote = run("footnote");
    assert_eq!(footnote.notices, []);
    assert_eq!(
        footnote.parts,
        [changed_part(
            BlacklinePartKind::Footnote { number: 1 },
            "Note "
        )]
    );
    let header = run("header");
    assert_eq!(header.notices, []);
    assert_eq!(header.parts, [changed_part(header_on(&[1]), "Header ")]);
}
