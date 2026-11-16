//! The plain line shape: what it takes and what it is not.

use super::{Segment, mentions_stetdoc, plain_segments};

fn bash(text: &str) -> Option<Vec<Segment>> {
    plain_segments(text, false)
}

fn pwsh(text: &str) -> Option<Vec<Segment>> {
    plain_segments(text, true)
}

fn words(segments: &[Segment]) -> Vec<Vec<&str>> {
    segments
        .iter()
        .map(|segment| segment.words.iter().map(String::as_str).collect())
        .collect()
}

/// Plain lines split into their commands, quotes off; a pipe marks the command it feeds.
#[test]
fn a_plain_line_splits_into_its_commands() {
    let got =
        bash("stetdoc view 'My File.docx' --find \"x y\" 2>&1 | head -50; ls && pwd").unwrap();
    assert_eq!(
        words(&got),
        [
            vec!["stetdoc", "view", "My File.docx", "--find", "x y"],
            vec!["head", "-50"],
            vec!["ls"],
            vec!["pwd"],
        ]
    );
    assert_eq!(
        got.iter()
            .map(|segment| segment.piped_in)
            .collect::<Vec<_>>(),
        [false, true, false, false]
    );
    let got = pwsh(r"stetdoc view C:\m\a.docx | Select-Object -First 5").unwrap();
    assert_eq!(
        words(&got),
        [
            vec!["stetdoc", "view", r"C:\m\a.docx"],
            vec!["Select-Object", "-First", "5"],
        ]
    );
    let got = bash("stetdoc view /m/RUNNER~1/a.docx\nstetdoc check a.docx || true").unwrap();
    assert_eq!(got.len(), 3);
    assert_eq!(
        words(&pwsh("stetdoc view 'C:\\x y\\a.docx'").unwrap()),
        [vec!["stetdoc", "view", r"C:\x y\a.docx"]]
    );
}

/// Every form the shell would change, or that is no simple command, is not plain.
#[test]
fn what_the_shell_would_change_is_not_plain() {
    for text in [
        "stetdoc view $HOME/a.docx",
        "stetdoc view ~/a.docx",
        "stetdoc view a=~/x",
        "stetdoc view `pwd`/a.docx",
        "stetdoc view $(pwd)/a.docx",
        "stetdoc view a\\ b.docx",
        "stetdoc view \"a\\b.docx\"",
        "stetdoc view \"$x\"",
        "stetdoc view a*.docx",
        "stetdoc view 'a*.docx'",
        "stetdoc view \"/w/\"[b]/a.docx",
        "stetdoc view /w/(b)/a.docx",
        "stetdoc view {a,b}.docx",
        "stetdoc view a.docx > out.txt",
        "stetdoc view < a.docx",
        "stetdoc view <(cat a.docx)",
        "stetdoc view a.docx &",
        "stetdoc view a.docx |& head",
        "stetdoc view a.docx |",
        "| stetdoc view a.docx",
        "stetdoc view a'b'.docx",
        "stetdoc view 'a'b.docx",
        "stetdoc view 'a.docx",
        "stetdoc view a.docx # note",
        "stetdoc view a.docx\r",
        "sh <<EOF\nstetdoc view a.docx\nEOF",
        "2>&1 stetdoc view a.docx",
        "stetdoc view a.docx 2>&1x",
        "stetdoc view !a.docx",
    ] {
        assert_eq!(bash(text), None, "{text}");
    }
    for text in [
        "$a='x'; stetdoc view @a",
        "stetdoc --% view a.docx",
        "stetdoc view (Join-Path /w b.docx)",
        "stetdoc view \"/`w/b.docx\"",
        "stetdoc view @'\na.docx\n'@",
        "& stetdoc view a.docx",
        "stetdoc view a.docx,b.docx",
        "stetdoc view 'it''s.docx'",
        "stetdoc view \u{2018}a.docx\u{2019}",
        "stetdoc view {a}",
        "stetdoc view [a].docx",
        "stetdoc view a?.docx",
    ] {
        assert_eq!(pwsh(text), None, "{text}");
    }
}

/// A line past the word or byte limit is not plain, wherever the stetdoc command stands.
#[test]
fn a_long_line_is_not_plain() {
    let filler = "a ".repeat(1100);
    assert_eq!(
        bash(&format!(
            "stetdoc capabilities {filler}; stetdoc view a.docx"
        )),
        None
    );
    assert_eq!(bash(&format!("echo {filler}; stetdoc view a.docx")), None);
    let segments = "true; ".repeat(40);
    assert_eq!(bash(&format!("{segments}stetdoc view a.docx")), None);
}

/// The name is found in any case, and split by quotes or escape characters.
#[test]
fn stetdoc_is_found_however_it_is_split() {
    for text in [
        "stetdoc view a",
        "STETDOC view a",
        "s't'etdoc view a",
        "st\\etdoc view a",
        "st`etdoc view a",
        "st^etdoc view a",
        "x | xargs \"stetdoc\"",
    ] {
        assert!(mentions_stetdoc(text), "{text}");
    }
    assert!(!mentions_stetdoc("cat notes.docx | head"));
}
