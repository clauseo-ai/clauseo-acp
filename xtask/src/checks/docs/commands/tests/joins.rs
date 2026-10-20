//! Tests of the lines a code block joins, and what the joined text gives.

use super::*;

/// The lines of a block with info word `info`, numbered from 1, split at each `\n` of `text`.
fn block(info: &str, text: &str) -> Vec<CodeLine> {
    let numbered = |(index, line): (usize, &str)| CodeLine {
        line: 1 + u32::try_from(index).unwrap_or(u32::MAX),
        text: line.to_owned(),
        info: info.to_owned(),
    };
    text.split('\n').enumerate().map(numbered).collect()
}

/// What `read_texts` reads from `code`, one `line: text` per line, joined with `\n`.
fn reads(code: &[CodeLine]) -> String {
    let texts: Vec<String> = read_texts(code)
        .iter()
        .map(|(at, t)| format!("{at}: {t}"))
        .collect();
    texts.join("\n")
}

#[test]
fn a_continued_line_is_joined_with_the_lines_after_it_in_its_block() {
    // (info word, lines, what `read_texts` reads, the lines `problems` reports), one row a line.
    #[rustfmt::skip]
    let rows: &[(&str, &str, &str, &[u32])] = &[
        ("sh", "cargo xtask \\\nfrobnicate", "1: cargo xtask frobnicate", &[1]),
        ("sh", "cargo xtask check \\\n--quik", "1: cargo xtask check --quik", &[1]),
        ("console", "$ cargo xtask check \\\n> --quik", "1: cargo xtask check --quik", &[1]),
        ("powershell", "cargo xtask check `\n--quik", "1: cargo xtask check --quik", &[1]),
        ("cmd", "cargo xtask check ^\n--quik", "1: cargo xtask check --quik", &[1]),
        ("sh", "cargo xtask \\\ncheck \\\n--quick", "1: cargo xtask check --quick", &[]),
        ("sh", "cargo xtask check \\\n--quick", "1: cargo xtask check --quick", &[]),
        ("sh", "cargo xtask frobnicate \\", "1: cargo xtask frobnicate", &[1]),
        ("sh", "cargo xtask check\\  ", "1: cargo xtask check", &[]),
        ("console", "$ cargo xtask check \\\n--quick", "1: cargo xtask check --quick", &[]),
        ("ps1-session", "PS> cargo xtask check `\n> --quick", "1: cargo xtask check --quick", &[]),
        ("pwsh-session", "PS> cargo xtask check `\n>> --quik", "1: cargo xtask check --quik", &[1]),
        ("powershell", "cargo xtask check \\\n--quick", "1: cargo xtask check \\\n2: --quick", &[1]),
        ("sh", "cargo xtask check ^\n--quick", "1: cargo xtask check ^\n2: --quick", &[1]),
        ("console", "cargo xtask check \\\n--quick", "", &[]),
        ("text", "cargo xtask check \\\n--quick", "", &[]),
        ("sh", "cargo xtask check \\\\\n--quick", "1: cargo xtask check \\\\\n2: --quick", &[1]),
        ("powershell", "cargo xtask check ``\n--quick", "1: cargo xtask check ``\n2: --quick", &[]),
        ("cmd", "cargo xtask check ^^\n--quick", "1: cargo xtask check ^^\n2: --quick", &[1]),
        ("sh", "# x \\\ncargo xtask frobnicate", "1: # x \\\n2: cargo xtask frobnicate", &[2]),
        ("sh", "cargo xtask check # done \\\ncargo xtask frobnicate", "1: cargo xtask check # done \\\n2: cargo xtask frobnicate", &[2]),
        ("sh", "echo '# x' \\\ncargo xtask check", "1: echo '# x' cargo xtask check", &[]),
    ];
    for (info, text, read, reported) in rows {
        let scan = Scan {
            code: block(info, text),
            ..Scan::default()
        };
        assert_eq!(reads(&scan.code), *read, "{info} {text:?}");
        let found = problems("a.md", &scan);
        let lines: Vec<u32> = found.iter().map(|violation| violation.line).collect();
        assert_eq!(lines, *reported, "{info} {text:?}");
    }
    // Lines that are not next to each other are two lines, whatever lies between them.
    let mut apart = block("sh", "cargo xtask check \\\n--quick");
    apart[1].line = 3;
    assert_eq!(reads(&apart), "1: cargo xtask check\n3: --quick");
}
