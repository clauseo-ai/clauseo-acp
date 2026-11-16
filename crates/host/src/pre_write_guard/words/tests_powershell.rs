//! `PowerShell`'s own quoting in the shared reader (`powershell.rs`, `about_Quoting_Rules`).

use super::{ARRAY_COMMA, Escape, Role, Segments, candidates, powershell_words, shell_segments};

/// The words of `command` read as `PowerShell`, with whether each is unknown, and whether the
/// reading is unsure.
fn read(command: &str) -> (Vec<(String, bool)>, bool) {
    let mut segments = Segments::new(64);
    shell_segments(command, Escape::Backtick, &mut segments);
    let words = segments
        .iter()
        .flat_map(|segment| {
            segment
                .words
                .iter()
                .cloned()
                .zip(segment.unknown.iter().copied())
        })
        .collect();
    (words, segments.unsure())
}

fn words(command: &str) -> Vec<String> {
    read(command).0.into_iter().map(|(word, _)| word).collect()
}

#[test]
fn doubled_quotes_stand_for_one_quote() {
    assert_eq!(
        words(r"Set-Content -LiteralPath 'C:\m\O''Brien & Co Lease\held.txt' -Value x"),
        [
            "Set-Content",
            "-LiteralPath",
            r"C:\m\O'Brien & Co Lease\held.txt",
            "-Value",
            "x"
        ]
    );
    assert_eq!(
        words(r#"echo "say ""hi""" 'it''s'"#),
        ["echo", r#"say "hi""#, "it's"]
    );
    assert_eq!(words("echo ''''"), ["echo", "'"]);
    assert_eq!(words(r#"echo """""#), ["echo", r#"""#]);
    // Bash would read `'O''Brien'` as `OBrien`.
    assert_eq!(
        words_posix("echo 'O''Brien'"),
        ["echo", "OBrien"],
        "the POSIX reading is unchanged"
    );
}

fn words_posix(command: &str) -> Vec<String> {
    let mut segments = Segments::new(64);
    shell_segments(command, Escape::Backslash, &mut segments);
    segments
        .iter()
        .flat_map(|segment| segment.words.clone())
        .collect()
}

#[test]
fn typographic_quotes_are_quotes() {
    // Any single quote closes any other; doubled, the second stands.
    assert_eq!(
        words("echo \u{2018}O\u{2019}\u{2019}Brien Lease\u{2019}"),
        ["echo", "O\u{2019}Brien Lease"]
    );
    assert_eq!(words("echo \u{201a}a b\u{201b}"), ["echo", "a b"]);
    // A doubled quote of either form stands for the second.
    assert_eq!(words("echo 'a\u{2019}'b'"), ["echo", "a'b"]);
    assert_eq!(
        words("echo \u{201c}a \u{201d}\u{201d}b\u{201e}"),
        ["echo", "a \u{201d}b"]
    );
    assert_eq!(words("echo \"a b\u{201d}"), ["echo", "a b"]);
    // Not quotes in Bash.
    assert_eq!(
        words_posix("echo \u{2018}a b\u{2019}"),
        ["echo", "\u{2018}a", "b\u{2019}"]
    );
}

#[test]
fn the_backtick_escapes_in_double_quotes_and_out() {
    assert_eq!(
        words(r#"echo "a`"b `$x ``" Fees` `$500 a`tb "c`nd" `u{48}i"#),
        ["echo", "a\"b $x `", "Fees $500", "a\tb", "c\nd", "Hi"]
    );
    // Inside single quotes nothing is special.
    assert_eq!(words("echo 'a`b $x'"), ["echo", "a`b $x"]);
    // A backtick before a new line joins the lines.
    assert_eq!(words("echo a`\nb"), ["echo", "ab"]);
}

#[test]
fn an_expansion_makes_its_word_unknown() {
    let (got, unsure) =
        read(r#"Set-Content $p "$dir\x" $(Join-Path a "b c") ${env:TEMP} '$lit' "`$esc" $ x$_"#);
    assert!(!unsure);
    assert_eq!(
        got,
        [
            ("Set-Content".to_owned(), false),
            ("$p".to_owned(), true),
            (r"$dir\x".to_owned(), true),
            (r#"$(Join-Path a "b c")"#.to_owned(), true),
            ("${env:TEMP}".to_owned(), true),
            ("$lit".to_owned(), false),
            ("$esc".to_owned(), false),
            ("$".to_owned(), false),
            ("x$_".to_owned(), true),
            // The subexpression's own command, read after the line's.
            ("Join-Path".to_owned(), false),
            ("a".to_owned(), false),
            ("b c".to_owned(), false),
        ]
    );
    let (got, _) = read("Get-Content $env:APPDATA\\clauseo\\x");
    assert_eq!(
        got.last(),
        Some(&(r"$env:APPDATA\clauseo\x".to_owned(), true))
    );
}

#[test]
fn an_unquoted_comma_parts_an_array_and_a_quoted_one_stays() {
    let word = format!("a{ARRAY_COMMA}held.docx");
    assert_eq!(
        words(r"Set-Content -Path 'a','held.docx' -Value x"),
        ["Set-Content", "-Path", word.as_str(), "-Value", "x"]
    );
    assert_eq!(
        words(r"cd 'C:\Clients\Smith, Jones & Co'"),
        ["cd", r"C:\Clients\Smith, Jones & Co"]
    );
    assert_eq!(candidates(&word).collect::<Vec<_>>(), ["a", "held.docx"]);
    assert_eq!(
        powershell_words("Remove-Item a, 'b c',d; ls"),
        [vec!["Remove-Item", "a", "b c", "d"], vec!["ls"]]
    );
    // Bash keeps a comma as it is.
    assert_eq!(words_posix("echo a,b"), ["echo", "a,b"]);
}

#[test]
fn a_here_string_is_one_word() {
    assert_eq!(
        words("Set-Content -Path a.md -Value @'\nIt's \"long\" $x\n'@; ls"),
        [
            "Set-Content",
            "-Path",
            "a.md",
            "-Value",
            "It's \"long\" $x",
            "ls"
        ]
    );
    let (got, unsure) = read("Set-Content a.md @\"\nHi $name\n\"@");
    assert!(!unsure);
    assert_eq!(got.last(), Some(&("Hi $name".to_owned(), true)));
    // `@` anywhere else is itself.
    assert_eq!(
        words("echo user@host @args"),
        ["echo", "user@host", "@args"]
    );
}

#[test]
fn a_reading_it_cannot_finish_is_unsure() {
    for command in [
        "Set-Content 'held.docx",
        "Set-Content \"held.docx",
        "Set-Content $(Get-Item x",
        "Set-Content a @'\nnever closed",
        "echo hi >",
    ] {
        assert!(read(command).1, "{command}");
    }
    let (got, _) = read("Set-Content 'held.docx -Value x");
    assert_eq!(got.last(), Some(&("held.docx -Value x".to_owned(), true)));
    assert!(!read("Set-Content 'a' \"b\" > c").1);
}

#[test]
fn streams_and_targets_of_redirections_are_told_apart() {
    let mut segments = Segments::new(64);
    shell_segments(
        "echo hi > out.txt 2>&1 *> all.txt 3",
        Escape::Backtick,
        &mut segments,
    );
    let segment = segments.iter().next().unwrap();
    assert_eq!(segment.redirects, ["out.txt", "&1", "all.txt"]);
    assert_eq!(
        segment.roles,
        [
            Role::Arg,
            Role::Arg,
            Role::Target,
            Role::Stream,
            Role::Target,
            Role::Stream,
            Role::Target,
            Role::Arg
        ]
    );
}
