//! The reader against rows observed on a real Windows PowerShell 5.1 and PowerShell 7.4 (the
//! VM, 16 Nov 2026; both gave the same result unless a test says otherwise).

use super::{Escape, Role, Segments, powershell_words, shell_segments};

/// The segments of `command` read as `PowerShell`, each its words.
fn segments(command: &str) -> Vec<Vec<String>> {
    let mut read = Segments::new(64);
    shell_segments(command, Escape::Backtick, &mut read);
    read.iter().map(|segment| segment.words.clone()).collect()
}

/// The words of `command`'s first segment, each with whether it is unknown.
fn first(command: &str) -> Vec<(String, bool)> {
    let mut read = Segments::new(64);
    shell_segments(command, Escape::Backtick, &mut read);
    read.iter()
        .next()
        .map(|segment| {
            segment
                .words
                .iter()
                .cloned()
                .zip(segment.unknown.iter().copied())
                .collect()
        })
        .unwrap_or_default()
}

/// Rows 1, 1b, 2 and 3: CR, LF and CRLF are each one new line, for a backtick's continuation, a
/// comment's end and a statement's end.
#[test]
fn cr_lf_and_crlf_are_each_one_new_line() {
    // Row 1b: a backtick right before CRLF joins the lines: one statement.
    assert_eq!(
        segments("Remove-Item `\r\n-LiteralPath .\\t.txt"),
        [vec!["Remove-Item", "-LiteralPath", ".\\t.txt"]]
    );
    assert_eq!(
        segments("Remove-Item `\r-LiteralPath .\\t.txt"),
        [vec!["Remove-Item", "-LiteralPath", ".\\t.txt"]]
    );
    // Row 1: backtick, blank, CRLF: the backtick escapes the blank; two statements.
    assert_eq!(
        segments("Remove-Item ` \r\n-LiteralPath .\\t.txt"),
        [vec!["Remove-Item", " "], vec!["-LiteralPath", ".\\t.txt"]]
    );
    // Row 2: a lone CR ends a comment.
    assert_eq!(
        segments("# c\rRemove-Item .\\t.txt"),
        [vec!["Remove-Item", ".\\t.txt"]]
    );
    // Row 3: a lone CR ends a statement.
    assert_eq!(
        segments("'a.txt' | Out-Null\rRemove-Item .\\t.txt"),
        [
            vec!["a.txt"],
            vec!["Out-Null"],
            vec!["Remove-Item", ".\\t.txt"]
        ]
    );
    // A here-string's lines may end in CR or CRLF too.
    assert_eq!(
        segments("Set-Content a.md @'\r\nIt's\r\n'@\rls"),
        [vec!["Set-Content", "a.md", "It's"], vec!["ls"]]
    );
    // Bash is unchanged: a CR is a blank there.
    let mut read = Segments::new(64);
    shell_segments("echo a\rb", Escape::Backslash, &mut read);
    assert_eq!(read.iter().count(), 1);
}

/// Rows 4a and 4b (L1): `'abc'.ToUpper()` is one argument and the method runs, so the word is
/// unknown; `'a'b` is two arguments.
#[test]
fn what_follows_a_closing_quote() {
    assert_eq!(
        first("Write-Output 'abc'.ToUpper()"),
        [
            ("Write-Output".to_owned(), false),
            ("abc.ToUpper".to_owned(), true)
        ]
    );
    assert_eq!(
        first(r"Set-Content 'C:\m'[0] x"),
        [
            ("Set-Content".to_owned(), false),
            (r"C:\m[0]".to_owned(), true),
            ("x".to_owned(), false)
        ]
    );
    assert_eq!(
        segments("Write-Output 'a'b \"c\"d"),
        [vec!["Write-Output", "a", "b", "c", "d"]]
    );
    // A comma, a blank or an operator after the quote is no bare text.
    assert_eq!(segments("echo 'a';ls"), [vec!["echo", "a"], vec!["ls"]]);
}

/// Row 5 (L2): `> (Join-Path …)` writes where the expression leads: the target is unknown, and
/// the expression's words are no target.
#[test]
fn a_parenthesised_redirection_target_is_unknown() {
    let mut read = Segments::new(64);
    shell_segments(
        "'x' > (Join-Path $PWD 'r.txt')",
        Escape::Backtick,
        &mut read,
    );
    let all: Vec<_> = read.iter().collect();
    let target = all.first().unwrap();
    assert_eq!(target.redirects, ["("]);
    assert_eq!(target.roles, [Role::Arg, Role::Target]);
    assert_eq!(target.unknown, [false, true]);
    let inner = all.get(1).unwrap();
    assert!(inner.roles.iter().all(|role| *role == Role::Arg));
    assert!(inner.redirects.is_empty());
}

/// Row 8 (L5): `'a'’b'` is one argument, the doubled pair keeping its second quote (`’`).
#[test]
fn a_mixed_doubled_quote_keeps_the_second() {
    assert_eq!(
        segments("Write-Output 'a'\u{2019}b'"),
        [vec!["Write-Output", "a\u{2019}b"]]
    );
}

/// Row 9 (L4): Windows PowerShell 5.1 reads `"`u{43}"` as `u{43}`, PowerShell 7.4 as `C`. The
/// reader reads it as 7 does, always the larger reading.
#[test]
fn the_unicode_escape_reads_as_powershell_7() {
    assert_eq!(segments("echo \"`u{43}\""), [vec!["echo", "C"]]);
}

/// L7: a path inside a here-string of more than one line is a word of its own for the readers
/// that take [`powershell_words`].
#[test]
fn a_path_inside_a_here_string_is_a_word_too() {
    let words = powershell_words("$s = @'\nType C:\\Data\\x.txt\nnow\n'@");
    assert!(
        words.iter().flatten().any(|word| word == r"C:\Data\x.txt"),
        "{words:?}"
    );
}
