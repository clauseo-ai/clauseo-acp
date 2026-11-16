//! The Word blackline through the host when stetdoc's compare marks no change for what changed,
//! or its view does not show it: a link's new address, a comment's new text, a text box's text,
//! and a change in a footnote alone or a header alone. The stand-in stetdoc answers stetdoc
//! 0.9.3's real output (`tests/fixtures/blackline/partial/`): the redline's view, its `check
//! --json`, and each version's view; the real stetdoc runs when one is on `PATH`.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::BlacklineNotice::{
    ChangesOutsideText, CommentChanged, MayDiffer, TextBoxChanged,
};
use clauseo_protocol::{ParagraphChange, WordBlackline};

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
/// through the stand-in stetdoc answering the pair's fixtures: `view-<pair>.json`,
/// `check-<pair>.json` and `final-<pair>-old.json`, `final-<pair>-new.json` where there are
/// some, and `extra` files (name, bytes) beside it. The test keeps the `Dir` while it reads the
/// stand-in's folder: dropping it deletes the folder.
fn through_fake(
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
        (format!("view-{pair}.json"), "view.json"),
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
    let (same, fake, _same_dir) =
        through_fake("blackline_link_same_file", "link", Some((&new, &new)), &[]);
    assert_eq!((same.changed_links.len(), same.notices.len()), (0, 0));
    assert!(!fake.join("args-view-old.txt").exists());
}

#[test]
fn a_comment_whose_text_changed_is_noted() {
    let (answer, _, _) = through_fake("blackline_comment", "comment", None, &[]);
    assert_eq!(answer.notices, [CommentChanged]);
}

#[test]
fn a_text_box_whose_text_changed_is_noted() {
    let (answer, _, _) = through_fake("blackline_textbox", "textbox", None, &[]);
    assert_eq!(answer.notices, [TextBoxChanged]);
    assert_eq!(answer.paragraphs[1].change, ParagraphChange::Changed);
}

/// Until the notes and headers are shown, a change in a footnote alone, or a header alone, gets
/// the notice that changes outside the main text aren't shown: by stetdoc's list of ids, and by
/// the summary's count when there is no list.
#[test]
fn a_footnote_or_header_change_alone_is_noted() {
    for pair in ["footnote", "header"] {
        let (answer, _, _) = through_fake(&format!("blackline_{pair}_only"), pair, None, &[]);
        assert_eq!(answer.notices, [ChangesOutsideText], "{pair}");
        assert!(
            answer
                .paragraphs
                .iter()
                .all(|p| p.change == ParagraphChange::Same)
        );
        let (by_summary, _, _) = through_fake(
            &format!("blackline_{pair}_summary"),
            pair,
            None,
            &[("check.json", b"not a list")],
        );
        assert_eq!(by_summary.notices, [ChangesOutsideText], "{pair}");
    }
}

/// A version's view that does not read: the links and comments can't be compared, so the
/// blackline, still shown, may differ.
#[test]
fn versions_that_can_not_be_compared_may_differ() {
    let (answer, _, _) = through_fake(
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
    assert_eq!(run("footnote").notices, [ChangesOutsideText]);
    assert_eq!(run("header").notices, [ChangesOutsideText]);
}
