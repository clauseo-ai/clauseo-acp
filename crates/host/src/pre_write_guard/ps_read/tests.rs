//! The grammar and the rewrite (`parse.rs`), pure text: every normal case is read and rewritten
//! exactly, every hostile one refused. The lexer's properties are checked with proptest.

use proptest::prelude::*;

use super::parse::{
    Count, Kind, Line, PIN, PathText, Select, Source, Statement, parse, quoted, rewrite,
};

/// Line (b) of the laptop, verbatim but for the user's names.
const LINE_B: &str = r#"Get-ChildItem -Recurse -File "C:\Users\Ann\Matters\Smith" | Select-Object FullName, Length; Get-ChildItem "C:\Users\Ann\AppData\Local\ai.clauseo.desktop\agent-state\m-1\notes""#;

fn win(line: &str) -> Option<Line> {
    parse(line, true)
}

fn path(text: &str, absolute: bool) -> PathText {
    PathText {
        text: text.to_owned(),
        absolute,
    }
}

fn list(path: Option<PathText>) -> Source {
    Source::List {
        path,
        recurse: false,
        kind: None,
        force: false,
        name: false,
    }
}

/// `line` rewritten with `paths` standing for the canonical ones, each quoted.
fn rewritten(line: &str, paths: &[&str]) -> String {
    let quoted: Vec<String> = paths.iter().map(|path| quoted(path).unwrap()).collect();
    rewrite(&win(line).unwrap(), &quoted).unwrap()
}

#[test]
fn the_pin_is_the_fixed_import_of_the_management_module_from_pshome() {
    assert_eq!(
        PIN,
        r#"Import-Module -Name "$PSHOME\Modules\Microsoft.PowerShell.Management" -Force; "#
    );
}

#[test]
fn line_b_is_read_verbatim_and_rewritten() {
    let line = win(LINE_B).unwrap();
    assert_eq!(
        line.statements,
        vec![
            Statement {
                source: Source::List {
                    path: Some(path(r"C:\Users\Ann\Matters\Smith", true)),
                    recurse: true,
                    kind: Some(Kind::File),
                    force: false,
                    name: false,
                },
                select: Some(Select {
                    properties: vec!["FullName".to_owned(), "Length".to_owned()],
                    first: None,
                }),
            },
            Statement {
                source: list(Some(path(
                    r"C:\Users\Ann\AppData\Local\ai.clauseo.desktop\agent-state\m-1\notes",
                    true
                ))),
                select: None,
            },
        ]
    );
    let m = r"C:\Users\Ann\Matters\Smith";
    let notes = r"C:\Users\Ann\AppData\Local\ai.clauseo.desktop\agent-state\m-1\notes";
    assert_eq!(
        rewritten(LINE_B, &[m, notes]),
        format!(
            "{PIN}Microsoft.PowerShell.Management\\Get-ChildItem -LiteralPath '{m}' -Recurse \
             -File | Microsoft.PowerShell.Utility\\Select-Object -Property FullName, Length; \
             Microsoft.PowerShell.Management\\Get-ChildItem -LiteralPath '{notes}'"
        )
    );
}

#[test]
fn the_usual_reads_are_rewritten_exactly() {
    let m = r"C:\m";
    let gci = r"Microsoft.PowerShell.Management\Get-ChildItem -LiteralPath";
    let gc = r"Microsoft.PowerShell.Management\Get-Content -LiteralPath";
    let select = r"Microsoft.PowerShell.Utility\Select-Object";
    for (line, paths, want) in [
        ("Get-ChildItem", vec![m], format!("{gci} 'C:\\m'")),
        ("gci .", vec![m], format!("{gci} 'C:\\m'")),
        ("ls -Force", vec![m], format!("{gci} 'C:\\m' -Force")),
        (
            "dir 'C:\\m\\Drafts' -Name -Directory",
            vec![r"C:\m\Drafts"],
            format!("{gci} 'C:\\m\\Drafts' -Directory -Name"),
        ),
        (
            "GCI -RECURSE\t-path C:/m/Drafts",
            vec![r"C:\m\Drafts"],
            format!("{gci} 'C:\\m\\Drafts' -Recurse"),
        ),
        (
            "gc '.\\Contract (final).docx' -TotalCount 40",
            vec![r"C:\m\Contract (final).docx"],
            format!("{gc} 'C:\\m\\Contract (final).docx' -TotalCount 40"),
        ),
        (
            "Get-Content -LiteralPath 'C:\\m\\memo [v2].md' -Raw",
            vec![r"C:\m\memo [v2].md"],
            format!("{gc} 'C:\\m\\memo [v2].md' -Raw"),
        ),
        (
            "type \"C:\\m\\O'Brien; #1.txt\" -Last 3",
            vec![r"C:\m\O'Brien; #1.txt"],
            format!("{gc} 'C:\\m\\O''Brien; #1.txt' -Tail 3"),
        ),
        (
            "cat .\\a.txt -Head 007 | select -First 5",
            vec![r"C:\m\a.txt"],
            format!("{gc} 'C:\\m\\a.txt' -TotalCount 7 | {select} -First 5"),
        ),
        (
            "gci|Select-Object -Property Name,Length -First 2",
            vec![m],
            format!("{gci} 'C:\\m' | {select} -Property Name, Length -First 2"),
        ),
        (
            "  gci . ;gc .\\README.md  ",
            vec![m, r"C:\m\README.md"],
            format!("{gci} 'C:\\m'; {gc} 'C:\\m\\README.md'"),
        ),
    ] {
        assert_eq!(rewritten(line, &paths), format!("{PIN}{want}"), "{line}");
    }
}

#[test]
fn unix_rules_take_slash_paths_only() {
    let line = parse("gc /m/a/x.txt", false).unwrap();
    assert_eq!(
        line.statements[0].source.path(),
        Some(&path("/m/a/x.txt", true))
    );
    for line in [r"gc C:\m\x.txt", r"gc a\b", "gc //srv/s"] {
        assert_eq!(parse(line, false), None, "{line}");
    }
}

/// Lines the grammar refuses by their text: hostile ones and ones it does not cover.
const HOSTILE: &[&str] = &[
    // Number-like bare words.
    "gc 1kb",
    "gc .5",
    "gc 1s",
    "gc 0x1F",
    "gc 1e3",
    "gc 1d",
    "gc 2024-notes.txt",
    // Joints, empty statements and stray pipes.
    r#"gc 'a'"b""#,
    r#"gc a"b""#,
    "gc 'a'b",
    "gc -LiteralPath:x",
    "gci;;gci",
    "gci;",
    "gci |",
    "| gci",
    ";gci",
    "gci -",
    "gci --% x",
    "gc -Path\"x\"",
    // Line ends and invisible characters.
    "gc a.txt\rgc b",
    "gc a.txt\ngc b",
    "gc a\u{0}",
    "gc a.txt\u{85}rm x",
    "gc a.txt\u{2028}rm x",
    "gc 'a\u{2029}b'",
    "gc 'a\u{200B}b'",
    "gc \"a\tb\"",
    "gc 'a\u{a0}b'",
    // Commands outside the table, aliases included.
    "gcb",
    "gi .",
    "Get-Item .",
    "sls x a.txt",
    "Select-String x a.txt",
    "ii a.txt",
    "iex a",
    "gci | Where-Object Name",
    "gci | ? Name",
    "gci | ft",
    "gci | Format-Table",
    "gci | Group-Object Name",
    "gci | Sort-Object Name",
    "gci | Measure-Object",
    "Microsoft.PowerShell.Management\\Get-Content a.txt",
    "gc.exe a.txt",
    "'gci'",
    "gc a.txt | iex",
    "gc a.txt | gc",
    "select Name",
    // `..`, `.`, roots and other places.
    r"gc ..\x",
    r"gc .\..\x",
    r"gc C:\m\..\x",
    r"gc C:\m\.\x",
    r"gc a\\b",
    r"gc C:\m\",
    r"gc \\?\C:\d\state.sqlite",
    r"gc \\srv\share\x",
    r"gc \x",
    "gc //srv/share/x",
    "gci env:",
    r"gci HKLM:\Software",
    "gci Registry::HKEY_USERS",
    r"gci Microsoft.PowerShell.Core\FileSystem::C:\m",
    r"gc ~\x",
    "gc '~'",
    "gc C:x.txt",
    "gc C:",
    "gc a.txt:s",
    r"gc 'C:\m\a.txt::$DATA'",
    // A second command chained or piped.
    r"gc .\a.txt; Remove-Item .\a.txt",
    r"gc .\a.txt | Out-File .\b.txt",
    "gc a.txt > b.txt",
    "gc a.txt 2>&1",
    "gc a && rm x",
    "gc a || rm x",
    "gc a & rm x",
    "gc a.txt # c",
    "gc a.txt `\nrm x",
    "gc a.txt `",
    // Expansion and the call operators.
    "gc $env:USERPROFILE\\x",
    "gc \"C:\\m\\$(whoami).txt\"",
    "gc \"a`\"b\"",
    r#"gc "a""b""#,
    "gc 'a''b'",
    "& gc a",
    ". .\\x.ps1",
    "gc @a",
    "gc (x)",
    "gc {x}",
    "gc [x]",
    "gc a,b",
    "gci a, b",
    // Smart quotes and dashes.
    "gc ‘.\\a’; rm x",
    "gc “a”",
    "gc 'a’b'",
    "gc \"a„b\"",
    "gc –Path a",
    "gc —Path a",
    // Parameters outside the table, twice, or with bad values.
    "gc a -Wait",
    "gc a -Stream s",
    "gc a -Credential u",
    "gc a -Encoding UTF8",
    "gci -FollowSymlink",
    "gci -Filter x",
    "gci -Include x",
    "gci -Depth 2",
    "gci -s",
    "gci -Hidden",
    "gci -r",
    "gci -rec",
    "gci -ea 0",
    "gci -Recurse -Recurse",
    "gci -r -Recurse",
    "gci -File -Directory",
    "gci -File -File",
    "gci -Path a b",
    "gci a b",
    "gc",
    "gc a b",
    "gc a -TotalCount 1 -Raw",
    "gc a -TotalCount 1 -Tail 1",
    "gc a -TotalCount 1kb",
    "gc a -TotalCount 12345678",
    "gc a -TotalCount -1",
    "gc a -TotalCount '5'",
    "gc a -Raw -Raw",
    "gci -Name x -Name",
    // Globs.
    "gci *.docx",
    "gci C:\\m\\*",
    "gc 'C:\\m\\a?.txt'",
    "gc 'C:\\m\\[ab].txt'",
    "gc -LiteralPath 'C:\\m\\a*'",
    // Select-Object outside the list.
    "gci | Select-Object *",
    "gci | Select-Object @{n='x';e={1}}",
    "gci | Select-Object",
    "gci | Select-Object -First 1kb",
    "gci | Select-Object Name | Select-Object Name",
    "gci | Select-Object -ExpandProperty Name",
    "gci | Select-Object 'Name'",
    "gci | Select-Object Name,",
    "gci | Select-Object ,Name",
    "gci | Select-Object 1a",
    "gci | Select-Object Name -Property Length",
    "gci | Select-Object -First 1 -First 2",
    "gci | Select-Object -Last 1",
    "gci | Select-Object Na-me",
    // Too many statements.
    "gci; gci; gci; gci; gci",
];

/// Each hostile line, and each line the grammar does not cover, is refused by its text.
#[test]
fn hostile_and_unknown_lines_are_refused() {
    let long = format!("gc C:\\m\\{}", "a".repeat(5000));
    for line in HOSTILE.iter().copied().chain([long.as_str()]) {
        assert_eq!(win(line), None, "{line:?}");
    }
    let props = (0..17)
        .map(|n| format!("P{n}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(win(&format!("gci | select {props}")), None);
    assert_eq!(win(&format!("gci | select {}", "P".repeat(65))), None);
    assert!(win("gci; gci; gci; gci").is_some());
}

#[test]
fn a_quoted_path_is_single_quoted_with_quotes_doubled_and_odd_characters_refused() {
    assert_eq!(quoted(r"C:\m\O'Brien").as_deref(), Some(r"'C:\m\O''Brien'"));
    assert_eq!(
        quoted(r#"C:\m\"x" $y `z"#).as_deref(),
        Some(r#"'C:\m\"x" $y `z'"#)
    );
    for odd in [
        "C:\\m\\a\u{2019}b",
        "C:\\m\\a\nb",
        "C:\\m\\a\u{85}",
        "C:\\m\\a\u{202E}b",
    ] {
        assert_eq!(quoted(odd), None, "{odd:?}");
    }
    assert_eq!(Count::Total(1), Count::Total(1));
}

/// A line whose statements each start with the source and its words.
fn seeds() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        LINE_B.to_owned(),
        "gci".to_owned(),
        r"gc .\a.txt -Tail 3".to_owned(),
        r"Get-Content -LiteralPath 'C:\m\memo [v2].md' -Raw".to_owned(),
        r"gci C:\m -Recurse -File | select Name, Length -First 3".to_owned(),
        r#"type "C:\m\O'Brien.txt""#.to_owned(),
    ])
}

/// The characters PowerShell gives a meaning outside quotes that the grammar never accepts
/// there (an independent list, not the lexer's).
const NEVER_BARE: &str = "$`(){}[]<>#=%!^~+&@\"'\r\n\u{0}\u{85}\u{2028}\u{2029}*?";

/// The unquoted characters of `line` (quotes taken off naively: each `'…'` and `"…"`).
fn unquoted(line: &str) -> String {
    let mut out = String::new();
    let mut quote = None;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None => out.push(c),
        }
    }
    out
}

proptest! {
    /// The lexer never panics, and what it accepts holds no unquoted character outside the
    /// grammar, and its rewrite starts with the pin and holds the pin's text once.
    #[test]
    fn anything_accepted_is_plain(line in "[ -~\t\r\n\u{85}\u{2028}‘’“”–]{0,60}") {
        if let Some(parsed) = win(&line) {
            let bare = unquoted(&line);
            prop_assert!(!bare.chars().any(|c| NEVER_BARE.contains(c)), "{line:?}");
            let paths = vec!["'C:\\m'".to_owned(); parsed.statements.len()];
            let command = rewrite(&parsed, &paths).unwrap();
            prop_assert!(command.starts_with(PIN));
            prop_assert_eq!(command.matches("Import-Module").count(), 1);
            prop_assert!(!command.strip_prefix(PIN).unwrap_or_default().contains(['$', '`', '"']));
        }
    }

    /// Mutations of accepted seeds: never a panic, and what is accepted is still plain.
    #[test]
    fn mutated_seeds_stay_plain(seed in seeds(), at in 0usize..200, c in any::<char>()) {
        let mut chars: Vec<char> = seed.chars().collect();
        let at = at % (chars.len() + 1);
        chars.insert(at, c);
        let line: String = chars.into_iter().collect();
        if win(&line).is_some() {
            prop_assert!(!unquoted(&line).chars().any(|c| NEVER_BARE.contains(c)), "{line:?}");
        }
    }

    /// Anything appended that runs a second command, redirects, comments, escapes, expands or
    /// ends the line refuses the whole line.
    #[test]
    fn a_chained_tail_refuses(
        seed in seeds(),
        tail in prop::sample::select(vec![
            "; Remove-Item a", "| Out-File b", " > f", " `", " $x", " # c", " && rm x",
            "\nrm x", "\r", ";", " |", ", b", " 2>&1", " | iex", "; & x", " -Wait",
        ]),
    ) {
        let line = format!("{seed}{tail}");
        prop_assert_eq!(win(&line), None, "{:?}", line);
    }
}
