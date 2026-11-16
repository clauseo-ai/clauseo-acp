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

/// In PowerShell a word holding `"`, or a quoted word with a space that ends in `\`, is not
/// plain; without the space, or in Bash, the same words are.
#[test]
fn a_powershell_word_with_a_double_quote_is_not_plain() {
    assert_eq!(
        pwsh("stetdoc view --find 'q q\"q C:\\w\\b\\secret.docx'"),
        None
    );
    for text in [
        "stetdoc view 'a\"b.docx'",
        "stetdoc view --find 'x y\\' C:\\w\\b\\secret.docx",
        "stetdoc view \"C:\\x y\\\" a.docx",
        "stetdoc view 'C:\\x\ty\\'",
    ] {
        assert_eq!(pwsh(text), None, "{text}");
    }
    assert_eq!(
        words(&pwsh("stetdoc view --find 'xy\\' a.docx").unwrap()),
        [vec!["stetdoc", "view", "--find", "xy\\", "a.docx"]]
    );
    assert_eq!(
        words(&bash("stetdoc view --find 'q q\"q' a.docx").unwrap()),
        [vec!["stetdoc", "view", "--find", "q q\"q", "a.docx"]]
    );
}

/// A bare word that starts with `=` or holds `==` is not plain, in a command's place or an
/// argument's; a quoted one, or one with a single `=` inside, is.
#[test]
fn a_word_starting_with_an_equals_sign_is_not_plain() {
    for text in [
        "=stetdoc view a.docx",
        "stetdoc view =ls",
        "stetdoc view a.docx | =head",
        "stetdoc view --find==ls a.docx",
        "stetdoc view x==ls",
    ] {
        assert_eq!(bash(text), None, "{text}");
        assert_eq!(pwsh(text), None, "{text}");
    }
    assert_eq!(
        words(&bash("stetdoc view --find=x '=y' a.docx").unwrap()),
        [vec!["stetdoc", "view", "--find=x", "=y", "a.docx"]]
    );
}

/// What Windows PowerShell 5.1 hands a program for the single-quoted word holding `"` that the
/// shape refuses, as the argv-echo example (`examples/argv_echo.rs`) prints it. 5.1 wraps the
/// word in double quotes as it is, so the program reads three arguments, the last the path. The
/// shape is refused whatever 5.1 does; this records that it splits, so a change shows.
#[cfg(windows)]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "runs powershell.exe once to record how it passes a word to a program"
)]
fn windows_powershell_splits_a_word_with_a_double_quote() {
    use base64::Engine as _;

    let exe = std::env::current_exe().unwrap();
    let echo = exe.parent().unwrap().parent().unwrap().join("examples");
    let echo = echo.join("argv_echo.exe");
    assert!(
        echo.is_file(),
        "build the host's examples: {}",
        echo.display()
    );
    let word = r#"'q q"q C:\w\b\secret.docx'"#;
    assert_eq!(pwsh(&format!("stetdoc view --find {word}")), None);
    // Encoded, so the line reaches PowerShell as written, with no quoting of its own around it.
    let script = format!("& '{}' --find {word}", echo.display());
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&output.stdout);
    println!("powershell.exe handed the program:\n{said}");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let args: Vec<&str> = said
        .lines()
        .filter_map(|line| line.trim_end().strip_prefix('[')?.strip_suffix(']'))
        .collect();
    assert_eq!(args, ["--find", "q qq", r"C:\w\b\secret.docx"], "{said}");
}
