//! The Word blackline through the host when stetdoc's output is not all plain: a table inside a
//! table, hidden text and a line the reader can't read give the blackline with its notices, and
//! stetdoc's own refusals (a compare that exits 5 over a table inside a table) their own codes,
//! never `blackline.failed`. The stand-in stetdoc answers stetdoc 0.9.3's real output
//! (`tests/fixtures/blackline/partial/`); the real stetdoc runs when one is on `PATH`.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{BlacklineNotice, ErrorCode, ParagraphChange, RunKind};

use crate::blackline::{
    DOC, blackline, fake_stetdoc, files_of, fixture, lease_matter, pair_matter_launching, query,
    refused, stetdoc_on_path, temp_entries,
};
use crate::support::{Dir, FAKE, Scenario};
use crate::write;

/// A fixture of `partial/`.
fn partial(name: &str) -> Vec<u8> {
    fixture(&format!("partial/{name}"))
}

/// The stand-in stetdoc, its view `partial/<view>`.
fn fake_with_view(dir: &Dir, view: &str) -> PathBuf {
    let stetdoc = fake_stetdoc(dir);
    write::write(&stetdoc.parent().unwrap().join("view.json"), &partial(view));
    stetdoc
}

/// A fresh test folder named `name`.
fn dir(name: &str) -> Dir {
    Dir::new(name, &[(FAKE, Scenario::Bundled("prompt-stream"))])
}

#[test]
fn a_table_inside_a_table_is_blacklined() {
    let dir = dir("blackline_nested");
    let stetdoc = fake_with_view(&dir, "view-nested-inner.json");
    let (host, matter, old, _folder) = lease_matter(&dir, Some(&stetdoc));
    let answer = blackline(host.query(query(matter, DOC, old, None)));
    assert_eq!(answer.notices, []);
    assert_eq!(answer.paragraphs.len(), 6);
    let inner = &answer.paragraphs[2];
    assert_eq!(inner.change, ParagraphChange::Changed);
    let kinds: Vec<RunKind> = inner.runs.iter().map(|run| run.kind).collect();
    assert_eq!(
        kinds,
        [RunKind::Same, RunKind::Del, RunKind::Ins, RunKind::Same]
    );
    assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
    host.shutdown();
}

/// stetdoc's `hidden_text` warning is a notice over the blackline, not a failure.
#[test]
fn a_view_that_warns_is_blacklined_with_a_notice() {
    let dir = dir("blackline_hidden");
    let stetdoc = fake_with_view(&dir, "view-hidden.json");
    let (host, matter, old, _folder) = lease_matter(&dir, Some(&stetdoc));
    let answer = blackline(host.query(query(matter, DOC, old, None)));
    assert_eq!(answer.notices, [BlacklineNotice::HiddenText]);
    assert_eq!(answer.paragraphs.len(), 2);
    host.shutdown();
}

/// A line the reader can't read is left out; the rest comes, with the notice.
#[test]
fn a_line_that_does_not_read_is_left_out_with_a_notice() {
    let dir = dir("blackline_part");
    let stetdoc = fake_stetdoc(&dir);
    let view = String::from_utf8(partial("view-nested.json")).unwrap();
    let view = view.replace("p5 Outer right.", "p5 Outer {zz right.");
    write::write(
        &stetdoc.parent().unwrap().join("view.json"),
        view.as_bytes(),
    );
    let (host, matter, old, _folder) = lease_matter(&dir, Some(&stetdoc));
    let answer = blackline(host.query(query(matter, DOC, old, None)));
    assert_eq!(answer.notices, [BlacklineNotice::PartNotShown]);
    // The paragraph left out has a place of its own, where it stood.
    assert_eq!(answer.paragraphs.len(), 6);
    assert_eq!(answer.paragraphs[4].change, ParagraphChange::NotShown);
    host.shutdown();
}

/// A footnote and a header changed with the body: the blackline shows the body and says the
/// other changes are not shown.
#[test]
fn changes_outside_the_main_text_are_noted() {
    let dir = dir("blackline_outside");
    let stetdoc = fake_with_view(&dir, "view-notes.json");
    let (host, matter, old, _folder) = lease_matter(&dir, Some(&stetdoc));
    let answer = blackline(host.query(query(matter, DOC, old, None)));
    assert_eq!(answer.notices, [BlacklineNotice::ChangesOutsideText]);
    assert_eq!(answer.paragraphs.len(), 1);
    host.shutdown();
}

/// stetdoc 0.9.3's compare exits 5 with `validation_failed` when an inner table that ends its
/// cell is removed (`partial/compare-inner-last.json`, its real answer): the caller gets
/// `blackline.nested_table`, with the fixed message, and nothing is left behind.
#[test]
fn a_compare_refused_over_a_table_inside_a_table_says_so() {
    let dir = dir("blackline_inner_last");
    let stetdoc = fake_stetdoc(&dir);
    let fake = stetdoc.parent().unwrap();
    write::write(&fake.join("compare-exit.txt"), b"5");
    write::write(
        &fake.join("compare-out.json"),
        &partial("compare-inner-last.json"),
    );
    let (host, matter, old, folder) = lease_matter(&dir, Some(&stetdoc));
    let before = files_of(&folder);
    let error = host.query(query(matter, DOC, old, None)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::BlacklineNestedTable, "{error}");
    assert!(!error.retryable());
    assert_eq!(
        error.message(),
        "the Word blackline failed; the detail is in the host's log"
    );
    assert!(!fake.join("args-view.txt").exists());
    assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
    assert_eq!(files_of(&folder), before);

    // The same exit with stetdoc's size limit.
    let limit = br#"{"error":{"code":"limit_exceeded","message":"x is larger than the limit"}}"#;
    write::write(&fake.join("compare-out.json"), limit);
    refused(
        host.query(query(matter, DOC, old, None)),
        ErrorCode::BlacklineTooLarge,
    );
    host.shutdown();
}

/// With a real stetdoc on `PATH`: the pairs with a table inside a table and hidden text are
/// blacklined; the pair whose inner table ends its cell and is removed is refused as
/// `blackline.nested_table` by stetdoc 0.9.3, and blacklined by a stetdoc that mends it.
#[test]
fn the_real_stetdoc_blacklines_nested_tables_and_hidden_text() {
    let Some(stetdoc) = stetdoc_on_path() else {
        eprintln!("skipped: no stetdoc on PATH");
        return;
    };
    let run = |name: &str| {
        let dir = dir(&format!("blackline_real_{}", name.replace('-', "_")));
        let launcher = PassthroughLauncher::new(
            SpawnOptions::new()
                .home(dir.home().join("home"))
                .clear_env(),
        );
        let (old, new) = (
            partial(&format!("{name}-old.docx")),
            partial(&format!("{name}-new.docx")),
        );
        let (host, matter, from, _folder) =
            pair_matter_launching(&dir, Some(&stetdoc), Arc::new(launcher), &old, &new);
        let answer = host.query(query(matter, DOC, from, None));
        assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
        host.shutdown();
        answer
    };
    let inner = blackline(run("nested-inner"));
    assert_eq!(
        inner.paragraphs[2].change,
        ParagraphChange::Changed,
        "{inner:?}"
    );
    assert_eq!(inner.notices, []);
    let hidden = blackline(run("hidden-text"));
    assert_eq!(hidden.notices, [BlacklineNotice::HiddenText]);
    match run("inner-last") {
        Ok(_) => {}
        Err(error) => assert_eq!(error.code(), ErrorCode::BlacklineNestedTable, "{error}"),
    }
}
