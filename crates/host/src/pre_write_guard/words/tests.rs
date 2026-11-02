use super::{Escape, Segments, candidates, shell_segments};

fn segments(command: &str, limit: usize, escape: Escape) -> Vec<Vec<String>> {
    let mut segments = Segments::new(limit);
    shell_segments(command, escape, &mut segments);
    segments
        .iter()
        .map(|segment| {
            let mut words = segment.words.clone();
            if !segment.command {
                words.insert(0, "(body)".to_owned());
            }
            words
        })
        .collect()
}

fn split(command: &str, limit: usize, escape: Escape) -> Vec<String> {
    segments(command, limit, escape).concat()
}

#[test]
fn posix_words_split_the_simple_shell_way() {
    let posix = Escape::Backslash;
    assert_eq!(
        split(r#"cp  'a b' "c \"d\" \$e \x" f\ g '' h"#, 64, posix),
        ["cp", "a b", r#"c "d" $e \x"#, "f g", "", "h"]
    );
    assert_eq!(split("a 'b c", 64, posix), ["a", "b c"]);
    assert_eq!(split("a b c d", 2, posix), ["a", "b"]);
    assert_eq!(split("trailing\\", 64, posix), ["trailing"]);
    assert_eq!(split("  \t\n ", 64, posix), Vec::<String>::new());
}

#[test]
fn commands_split_into_segments_at_unquoted_operators() {
    let posix = Escape::Backslash;
    assert_eq!(
        segments(
            "mkdir -p D && cd D;echo 'a;b' \"c && d\" >|x | tee y||z\nw 2>&1 & v |& u",
            64,
            posix
        ),
        [
            vec!["mkdir", "-p", "D"],
            vec!["cd", "D"],
            vec!["echo", "a;b", "c && d", "x"],
            vec!["tee", "y"],
            vec!["z"],
            vec!["w", "2", "1"],
            vec!["v"],
            vec!["u"],
        ]
    );
    assert_eq!(
        segments("(cd D && x) ; { cd E; } &>log", 64, posix),
        [
            vec!["cd", "D"],
            vec!["x"],
            vec!["{", "cd", "E"],
            vec!["}", "log"]
        ]
    );
    assert_eq!(
        segments("a\\;b ; ;; c", 64, posix),
        [vec!["a;b"], vec!["c"]]
    );
    assert_eq!(segments("a; b; c", 2, posix), [vec!["a"], vec!["b"]]);
    assert_eq!(
        segments(
            "Set-Location D; Set-Content ..\\C.docx hi; & { sl E }",
            64,
            Escape::Backtick
        ),
        [
            vec!["Set-Location", "D"],
            vec!["Set-Content", r"..\C.docx", "hi"],
            vec!["{", "sl", "E", "}"]
        ]
    );
}

#[test]
fn redirections_comments_continuations_and_ansi_c_quotes_are_read() {
    let posix = Escape::Backslash;
    assert_eq!(
        split(">Contract.docx&true", 64, posix),
        ["Contract.docx", "true"]
    );
    assert_eq!(
        segments("cat>Contract.docx<x", 64, posix),
        [vec!["cat", "Contract.docx", "x"]]
    );
    assert_eq!(
        segments("cd D # ; cd /tmp\necho a#b", 64, posix),
        [vec!["cd", "D"], vec!["echo", "a#b"]]
    );
    assert_eq!(
        segments("echo hi > Con\\\ntract.docx \"a\\\nb\"", 64, posix),
        [vec!["echo", "hi", "Contract.docx", "ab"]]
    );
    assert_eq!(
        split(
            r"echo $'Con\x74ract.docx' $'a\'b\n' x$'y' $'\164\u0074\U00000074\x' $'\q'",
            64,
            posix
        ),
        ["echo", "Contract.docx", "a'b\n", "xy", "tttx", "q"]
    );
    // PowerShell has no ANSI-C quotes, and its backtick joins lines.
    assert_eq!(
        split("Set-Content $'a' Con`\ntract.docx", 64, Escape::Backtick),
        ["Set-Content", "$a", "Contract.docx"]
    );
}

#[test]
fn heredoc_bodies_are_not_commands() {
    let posix = Escape::Backslash;
    assert_eq!(
        segments(
            "cat <<'EOF' > x\ncd /tmp\nEOF\necho hi; cat <<-END\n\tcd /\n\tEND\nls <<<here",
            64,
            posix
        ),
        [
            vec!["cat", "x"],
            vec!["(body)", "cd", "/tmp"],
            vec!["echo", "hi"],
            vec!["cat"],
            vec!["(body)", "cd", "/"],
            vec!["ls", "here"],
        ]
    );
    // An unclosed heredoc runs to the end.
    assert_eq!(
        segments("cat <<E\ncd x", 64, posix),
        [vec!["cat"], vec!["(body)", "cd", "x"]]
    );
}
#[test]
fn powershell_words_keep_backslashes_and_escape_with_backticks() {
    let ps = Escape::Backtick;
    assert_eq!(
        split(
            r#"Set-Content C:\m\Contract.docx "a `"b`" \x" a` b"#,
            64,
            ps
        ),
        ["Set-Content", r"C:\m\Contract.docx", r#"a "b" \x"#, "a b"]
    );
}

#[test]
fn candidates_drop_shell_punctuation_around_a_name() {
    fn all(word: &str) -> Vec<&str> {
        candidates(word).collect()
    }
    assert_eq!(all("Contract.docx;"), ["Contract.docx"]);
    assert_eq!(all("Contract.docx&&"), ["Contract.docx"]);
    assert_eq!(all(">Contract.docx"), [">Contract.docx", "Contract.docx"]);
    assert_eq!(all(">>Contract.docx"), [">>Contract.docx", "Contract.docx"]);
    assert_eq!(all(">|Contract.docx"), [">|Contract.docx", "Contract.docx"]);
    assert_eq!(all("2>Contract.docx"), ["2>Contract.docx", "Contract.docx"]);
    assert_eq!(
        all("--out=Contract.docx"),
        ["--out=Contract.docx", "Contract.docx"]
    );
    assert_eq!(
        all("-Path:Contract.docx"),
        ["-Path:Contract.docx", "Contract.docx"]
    );
    assert_eq!(
        all(r#"-LiteralPath:"C:\m\Contract.docx""#),
        [
            r#"-LiteralPath:"C:\m\Contract.docx""#,
            r"C:\m\Contract.docx"
        ]
    );
    assert_eq!(all("a:b"), ["a:b"]);
    assert_eq!(all("-Force:"), ["-Force:"]);
    assert_eq!(all(";"), Vec::<&str>::new());
}

#[test]
fn powershell_block_comments_are_dropped() {
    let ps = Escape::Backtick;
    assert_eq!(
        split("<# note #> Set-Content Contract.docx hi", 64, ps),
        ["Set-Content", "Contract.docx", "hi"]
    );
    assert_eq!(
        split("Get-Item a <# x\n# y #>b", 64, ps),
        ["Get-Item", "a", "b"]
    );
    // POSIX shells have no block comments: the `#` after `<` starts a line comment.
    assert_eq!(split("cat <#x", 64, Escape::Backslash), ["cat"]);
}

#[test]
fn output_redirections_are_noted() {
    let redirects = |command: &str| -> Vec<Vec<String>> {
        let mut segments = Segments::new(64);
        shell_segments(command, Escape::Backslash, &mut segments);
        segments
            .iter()
            .map(|segment| segment.redirects.clone())
            .collect()
    };
    assert_eq!(
        redirects("cat a >b 2>&1 >>c; x &>d 2>/dev/null >|e <f"),
        [vec!["b", "&1", "c"], vec!["d", "/dev/null", "e"]]
    );
}
