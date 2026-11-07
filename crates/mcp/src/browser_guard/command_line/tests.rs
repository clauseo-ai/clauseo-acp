use super::{command_line, current_dir, env_block, is_batch};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// The command line for `program` and `args`, without its NUL, as text.
fn line(program: &str, args: &[&str]) -> String {
    let args: Vec<Vec<u16>> = args.iter().map(|arg| wide(arg)).collect();
    let mut line = command_line(&wide(program), &args).unwrap();
    assert_eq!(line.pop(), Some(0));
    String::from_utf16(&line).unwrap()
}

/// The arguments after the program, as the C runtime and `CommandLineToArgvW` split them (the
/// rules since 2008): a space or tab outside quotes ends an argument, 2n backslashes then `"`
/// are n backslashes and a quote that opens or closes, 2n + 1 then `"` are n backslashes and a
/// literal quote, and other backslashes are literal.
fn split(line: &str) -> Vec<String> {
    let rest = line
        .strip_prefix('"')
        .and_then(|rest| rest.split_once('"'))
        .map(|(_, rest)| rest)
        .unwrap();
    let mut args = Vec::new();
    let mut chars = rest.chars().peekable();
    loop {
        while chars.next_if(|c| *c == ' ' || *c == '\t').is_some() {}
        if chars.peek().is_none() {
            return args;
        }
        let (mut arg, mut quoted) = (String::new(), false);
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    let mut count = 1;
                    while chars.next_if_eq(&'\\').is_some() {
                        count += 1;
                    }
                    if chars.peek() == Some(&'"') {
                        arg.push_str(&"\\".repeat(count / 2));
                        if count % 2 == 1 {
                            arg.push('"');
                            chars.next();
                        }
                    } else {
                        arg.push_str(&"\\".repeat(count));
                    }
                }
                '"' => quoted = !quoted,
                ' ' | '\t' if !quoted => break,
                c => arg.push(c),
            }
        }
        args.push(arg);
    }
}

#[test]
fn the_command_line_matches_std() {
    // std's own cases (`library/std/src/sys/process/windows/tests.rs`, `test_make_command_line`).
    assert_eq!(line("prog", &["aaa", "bbb", "ccc"]), "\"prog\" aaa bbb ccc");
    assert_eq!(line("prog", &[r"C:\"]), r#""prog" C:\"#);
    assert_eq!(line("prog", &[r"2slashes\\"]), r#""prog" 2slashes\\"#);
    assert_eq!(line("prog", &[r" C:\"]), r#""prog" " C:\\""#);
    assert_eq!(line("prog", &[r" 2slashes\\"]), r#""prog" " 2slashes\\\\""#);
    assert_eq!(
        line(r"C:\Program Files\blah\blah.exe", &["aaa", "v*"]),
        r#""C:\Program Files\blah\blah.exe" aaa v*"#
    );
    assert_eq!(
        line(r"C:\Program Files\test", &["aa\"bb"]),
        r#""C:\Program Files\test" aa\"bb"#
    );
    assert_eq!(line("echo", &["a b c"]), "\"echo\" \"a b c\"");
    assert_eq!(
        line("echo", &["\" \\\" \\", "\\"]),
        "\"echo\" \"\\\" \\\\\\\" \\\\\" \\"
    );
    assert_eq!(line("\u{3c0}\u{42f}", &[]), "\"\u{3c0}\u{42f}\"");
    // An empty argument stays one.
    assert_eq!(line("prog", &["", "a"]), "\"prog\" \"\" a");
}

#[test]
fn every_argument_reads_back_unchanged() {
    let args = [
        "",
        " ",
        "plain",
        "two words",
        "tab\there",
        "\"",
        "\"\"",
        r"\",
        r"\\",
        r#"\""#,
        r#"a\\"b"#,
        r"ends\ ",
        r"ends\\",
        r"--user-data-dir=C:\Users\a b\chrome-profile-1",
        "--proxy-bypass-list=<-loopback>",
        "\u{e9}\u{3c0} \u{1f600}",
    ];
    assert_eq!(split(&line(r"C:\p\agent-browser.exe", &args)), args);
}

#[test]
fn a_nul_or_a_quote_in_the_program_is_refused() {
    let quote = command_line(&wide("a\"b"), &[]).unwrap_err();
    assert_eq!(quote.kind(), std::io::ErrorKind::InvalidInput);
    assert!(command_line(&wide("a\0b"), &[]).is_err());
    assert!(command_line(&wide("a"), &[wide("x\0y")]).is_err());
}

fn block(vars: &[(&str, &str)]) -> Vec<String> {
    let vars = vars
        .iter()
        .map(|(name, value)| ((*name).to_owned(), wide(value)))
        .collect();
    let block = env_block(vars).unwrap();
    let text = String::from_utf16(&block).unwrap();
    let body = text.strip_suffix("\0\0").expect("two NULs end the block");
    body.split('\0').map(str::to_owned).collect()
}

#[test]
fn the_environment_block_is_sorted_without_case_and_ends_in_two_nuls() {
    assert_eq!(
        block(&[("b", "2"), ("A", "1"), ("PATH", "C:\\x"), ("c", "")]),
        ["A=1", "b=2", "c=", "PATH=C:\\x"]
    );
    // Names that differ only in case are one variable: the first spelling, the last value.
    assert_eq!(block(&[("PATH", "first"), ("Path", "last")]), ["PATH=last"]);
    // No variables: two NULs, and nothing inherited.
    assert_eq!(env_block(Vec::new()).unwrap(), [0, 0]);
    // A value may hold `=`; the name's leading `=` is Windows' drive variables' own.
    assert_eq!(
        block(&[("=C:", "C:\\"), ("K", "a=b")]),
        ["=C:=C:\\", "K=a=b"]
    );
}

#[test]
fn a_bad_variable_is_refused() {
    for (name, value) in [("", "v"), ("A=B", "v"), ("A\0", "v"), ("A", "v\0")] {
        let vars = vec![(name.to_owned(), wide(value))];
        assert!(env_block(vars).is_err(), "{name:?}={value:?}");
    }
}

fn dir(path: &str) -> String {
    let mut out = current_dir(&wide(path)).unwrap();
    assert_eq!(out.pop(), Some(0));
    String::from_utf16(&out).unwrap()
}

#[test]
fn a_verbatim_folder_loses_its_prefix_only_when_it_reads_the_same() {
    assert_eq!(dir(r"C:\Users\a b\run"), r"C:\Users\a b\run");
    assert_eq!(dir(r"\\?\C:\Users\a b\run"), r"C:\Users\a b\run");
    assert_eq!(dir(r"\\?\C:\"), r"C:\");
    assert_eq!(dir(r"\\?\C:\run\"), r"C:\run\");
    assert_eq!(dir(r"\\?\UNC\server\share\run"), r"\\server\share\run");
    // Each of these reads otherwise without the prefix, so it stays.
    for kept in [
        r"\\?\C:\a\..\b",
        r"\\?\C:\a\.\b",
        r"\\?\C:\dot.",
        r"\\?\C:\space ",
        r"\\?\C:\a\\b",
        r"\\?\C:\a/b",
        r"\\?\C:",
        r"\\?\UNC\server",
        r"\\?\Volume{1}\a",
    ] {
        assert_eq!(dir(kept), kept);
    }
    assert!(current_dir(&wide("C:\\a\0")).is_err());
}

#[test]
fn names_fold_one_unit_at_a_time_and_sort_by_utf16() {
    // Windows' uppercase is one-to-one: `ß` has no one-unit uppercase, so these are two names.
    assert_eq!(
        block(&[("STRAßE", "a"), ("STRASSE", "b")]),
        ["STRASSE=b", "STRAßE=a"]
    );
    // A one-unit uppercase merges, outside ASCII too: the first spelling, the last value.
    assert_eq!(block(&[("\u{e4}x", "1"), ("\u{c4}X", "2")]), ["\u{e4}x=2"]);
    // UTF-16 order: a surrogate pair (U+10000, 0xD800 first) sorts before U+E000, which UTF-8
    // order would put first.
    assert_eq!(
        block(&[("\u{e000}", "bmp"), ("\u{10000}", "astral")]),
        ["\u{10000}=astral", "\u{e000}=bmp"]
    );
}

#[test]
fn a_name_with_a_non_ascii_first_letter_and_an_equals_is_refused() {
    for name in ["\u{e9}=X", "\u{1f600}=", "\u{e9}a=b"] {
        assert!(
            env_block(vec![(name.to_owned(), wide("v"))]).is_err(),
            "{name}"
        );
    }
    assert!(env_block(vec![("\u{e9}".to_owned(), wide("v"))]).is_ok());
}

#[test]
fn batch_files_are_named_whatever_their_case_or_trailing_dots() {
    for batch in [
        r"C:\a\run.bat",
        r"C:\a\run.CMD",
        r"C:\a\run.Bat. . ",
        r"C:\a\run.cmd...",
        r"C:\a b\x.y\run.bat:stream",
        "C:/a/run.bat",
    ] {
        assert!(is_batch(&wide(batch)), "{batch}");
    }
    for program in [
        r"C:\a\chrome.exe",
        r"C:\a\bat",
        r"C:\a.bat\run.exe",
        r"C:\a\run.bat.exe",
        r"C:\a\run.batch",
        r"C:\a\run",
    ] {
        assert!(!is_batch(&wide(program)), "{program}");
    }
}
