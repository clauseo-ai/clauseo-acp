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

/// Row 9 (L4): Windows PowerShell 5.1 reads ``"`u{43}"`` as `u{43}`, PowerShell 7.4 as `C`. Both
/// readings are kept, 7's then 5.1's (a role of its own, [`Role::Alt`], or a redirection's target
/// when the word is one), so a held `Contracte.docx` spelled ``Contract`e.docx`` is still named
/// (5.1 writes `Contracte.docx`, 7 a name with ESC in it).
#[test]
fn a_7_only_escape_is_read_both_ways() {
    assert_eq!(segments("echo \"`u{43}\""), [vec!["echo", "C", "u{43}"]]);
    assert_eq!(
        segments("Set-Content Contract`e.docx x"),
        [vec![
            "Set-Content",
            "Contract\u{1b}.docx",
            "Contracte.docx",
            "x"
        ]]
    );
    let mut read = Segments::new(64);
    shell_segments(
        "Get-Content x > Contract`e.docx",
        Escape::Backtick,
        &mut read,
    );
    let segment = read.iter().next().unwrap();
    assert_eq!(segment.redirects, ["Contract\u{1b}.docx", "Contracte.docx"]);
    assert_eq!(
        segment.roles,
        [Role::Arg, Role::Arg, Role::Target, Role::Target]
    );
    let mut read = Segments::new(64);
    shell_segments("Remove-Item Contract`e.docx", Escape::Backtick, &mut read);
    let segment = read.iter().next().unwrap();
    assert_eq!(segment.roles, [Role::Arg, Role::Arg, Role::Alt]);
    // Every other escape reads the same in both.
    assert_eq!(segments("echo a`tb"), [vec!["echo", "a\tb"]]);
}

/// Rows 1 and 2 (observed on Windows PowerShell 5.1 and PowerShell 7.4, VM, 16 Nov 2026): `#`
/// right after a closing quote starts a comment; quotes of the two kinds side by side are two
/// arguments; two single quotes inside single quotes are one quote, one argument.
#[test]
fn what_follows_a_closing_quote_is_a_comment_or_another_word() {
    assert_eq!(
        segments("Write-Output 'a'#; Write-Output b"),
        [vec!["Write-Output", "a"]]
    );
    assert_eq!(
        segments("Write-Output \"a\"#; Write-Output b"),
        [vec!["Write-Output", "a"]]
    );
    assert_eq!(
        segments("Write-Output 'a'\"b\""),
        [vec!["Write-Output", "a", "b"]]
    );
    assert_eq!(
        segments("Write-Output \"a\"'b'"),
        [vec!["Write-Output", "a", "b"]]
    );
    assert_eq!(
        segments("Write-Output 'a''b'"),
        [vec!["Write-Output", "a'b"]]
    );
}

/// L7: a path inside a here-string, of one line or more, is a word of its own for the readers
/// that take [`powershell_words`].
#[test]
fn a_path_inside_a_here_string_is_a_word_too() {
    for line in [
        "$s = @'\nType C:\\Data\\x.txt\nnow\n'@",
        "$s = @'\nType C:\\Data\\x.txt now\n'@",
        "$s = @\"\nType C:\\Data\\x.txt\n\"@",
    ] {
        let words = powershell_words(line);
        assert!(
            words.iter().flatten().any(|word| word == r"C:\Data\x.txt"),
            "{line}: {words:?}"
        );
    }
    // A quoted word with a blank is no here-string: it stays whole.
    assert_eq!(
        powershell_words(r"Get-Content 'C:\Matters\O''Brien & Sons\brief.txt'"),
        [vec!["Get-Content", r"C:\Matters\O'Brien & Sons\brief.txt"]]
    );
}
