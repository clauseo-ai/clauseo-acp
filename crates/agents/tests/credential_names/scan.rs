//! The scan and its own tests: see the crate root.

use std::path::{Path, PathBuf};

/// Claude's credential file, never named.
const CLAUDE_FILE: &str = concat!(".credentials", ".json");

/// Codex's login file, named only by `signin/reuse.rs`.
const CODEX_FILE: &str = concat!("auth", ".json");

/// The one file that may name [`CODEX_FILE`].
const REUSE: &str = "signin/reuse.rs";

/// What `reuse.rs` may never call: an open or a read. Matched as text anywhere in a line.
const OPENS: [&str; 7] = [
    "File::open",
    "fs::read",
    "OpenOptions",
    "open_read",
    "read_small",
    "write::read",
    "options(",
];

/// The literal `"auth"`, which with `json` or `with_extension` beside it can build Codex's
/// file's name in pieces.
const AUTH_LITERAL: &str = "\"auth\"";

/// Claude's `--json` flag, which sits beside its `auth` subcommand and builds no file name.
const JSON_FLAG: &str = "\"--json\"";

/// Whether `word` stands in `line` as a whole identifier (`File`, not `LOGIN_FILE` or
/// `Files`).
fn has_word(line: &str, word: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    line.match_indices(word).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + word.len()..].chars().next();
        !before.is_some_and(ident) && !after.is_some_and(ident)
    })
}

/// Whether `line` calls a function or method whose name starts with `prefix` (`read_` matches
/// `read_to_string(` and `read_dir(`), or named exactly `name` (`open(`).
fn calls(line: &str, prefix: &str, exact: bool) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    line.match_indices(prefix).any(|(at, _)| {
        if line[..at].chars().next_back().is_some_and(ident) {
            return false;
        }
        let rest = &line[at + prefix.len()..];
        let name_end = if exact {
            0
        } else {
            rest.find(|c: char| !ident(c)).unwrap_or(rest.len())
        };
        rest[name_end..].trim_start().starts_with('(')
    })
}

/// Whether `line` holds an escaped dot, `\x2e` or `\u{2e}` (any case, any leading zeros),
/// which can spell a file name's dot without the dot. Comments are blanked by now, so only a
/// literal can hold one.
fn escaped_dot(line: &str) -> bool {
    let lower = line.to_lowercase();
    if lower.contains("\\x2e") {
        return true;
    }
    lower.match_indices("\\u{").any(|(at, _)| {
        let digits = &lower[at + 3..];
        let end = digits.find('}').unwrap_or(digits.len());
        digits[..end].trim_start_matches('0') == "2e"
    })
}

/// Whether `line` (Claude's `--json` flag left out) says `json` or `with_extension`.
fn builds_extension(line: &str) -> bool {
    let line = line.replace(JSON_FLAG, "");
    line.contains("json") || line.contains("with_extension")
}

/// Every `.rs` file under `dir`, as (path relative to `dir` with `/`, contents).
fn rust_files(dir: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path: PathBuf = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let rel = path
                    .strip_prefix(dir)
                    .unwrap()
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                found.push((rel, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    found.sort();
    found
}

/// `source` with every comment blanked out (line, doc and nested block comments), keeping
/// string and character literals and line breaks.
fn code_only(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        let here = chars[i];
        let next = chars.get(i + 1).copied();
        if here == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if here == '/' && next == Some('*') {
            let mut depth = 0_usize;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if chars[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
            }
        } else if here == 'r' && matches!(next, Some('"' | '#')) && raw_start(&chars, i) {
            i = copy_raw(&chars, i, &mut out);
        } else if here == '"' {
            i = copy_string(&chars, i, &mut out);
        } else if here == '\'' {
            i = copy_char(&chars, i, &mut out);
        } else {
            out.push(here);
            i += 1;
        }
    }
    out
}

/// Whether a raw string starts at `i` (`r"`, `r#"`, …), and not inside an identifier.
fn raw_start(chars: &[char], i: usize) -> bool {
    let after_ident = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
    let mut j = i + 1;
    while chars.get(j) == Some(&'#') {
        j += 1;
    }
    !after_ident && chars.get(j) == Some(&'"')
}

/// Copies the raw string starting at `i`; returns the index after it.
fn copy_raw(chars: &[char], mut i: usize, out: &mut String) -> usize {
    out.push(chars[i]);
    i += 1;
    let mut hashes = 0;
    while chars[i] == '#' {
        hashes += 1;
        out.push('#');
        i += 1;
    }
    out.push('"');
    i += 1;
    while i < chars.len() {
        if chars[i] == '"' && (1..=hashes).all(|k| chars.get(i + k) == Some(&'#')) {
            out.push('"');
            out.extend(std::iter::repeat_n('#', hashes));
            return i + 1 + hashes;
        }
        out.push(chars[i]);
        i += 1;
    }
    i
}

/// Copies the string starting at `i`; returns the index after it.
fn copy_string(chars: &[char], mut i: usize, out: &mut String) -> usize {
    out.push('"');
    i += 1;
    while i < chars.len() {
        out.push(chars[i]);
        match chars[i] {
            '\\' => {
                if let Some(&escaped) = chars.get(i + 1) {
                    out.push(escaped);
                }
                i += 2;
            }
            '"' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

/// Copies a character literal at `i`, or just the `'` of a lifetime or label; returns the index
/// after what it copied.
fn copy_char(chars: &[char], i: usize, out: &mut String) -> usize {
    let literal_end = match chars.get(i + 1) {
        Some('\\') => (i + 3..chars.len().min(i + 12)).find(|&j| chars[j] == '\''),
        Some(_) if chars.get(i + 2) == Some(&'\'') => Some(i + 2),
        _ => None,
    };
    let end = literal_end.unwrap_or(i);
    out.extend(&chars[i..=end]);
    end + 1
}

/// Every rule `code` (one file, comments blanked) breaks, as messages.
fn violations(rel: &str, code: &str) -> Vec<String> {
    let mut found = Vec::new();
    let lines: Vec<&str> = code.lines().collect();
    for (n, line) in lines.iter().copied().enumerate() {
        let at = format!("crates/agents/src/{rel}:{}", n + 1);
        if escaped_dot(line) {
            found.push(format!("{at} spells a dot with an escape"));
        }
        let next = lines.get(n + 1).copied().unwrap_or_default();
        if line.contains(AUTH_LITERAL) && (builds_extension(line) || builds_extension(next)) {
            found.push(format!("{at} builds a file name from \"auth\" in pieces"));
        }
        if line.contains(CLAUDE_FILE) {
            found.push(format!("{at} names Claude's credential file"));
        }
        if rel != REUSE && line.contains(CODEX_FILE) {
            found.push(format!("{at} names Codex's login file outside {REUSE}"));
        }
        if rel == REUSE {
            for call in OPENS.iter().filter(|call| line.contains(*call)) {
                found.push(format!("{at} calls {call}"));
            }
            if has_word(line, "File") {
                found.push(format!("{at} names File"));
            }
            if calls(line, "read_", false) {
                found.push(format!("{at} calls a read_ function"));
            }
            if calls(line, "open", true) {
                found.push(format!("{at} calls open("));
            }
        }
    }
    found
}

/// Every rule the files break, as messages.
fn scan(files: &[(String, String)]) -> Vec<String> {
    files
        .iter()
        .flat_map(|(rel, source)| violations(rel, &code_only(source)))
        .collect()
}

/// Every `.rs` file under `crates/agents/src`.
fn src_files() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = rust_files(&src);
    assert!(
        files.iter().any(|(rel, _)| rel == "lib.rs"),
        "the scan found no lib.rs under {}",
        src.display()
    );
    files
}

#[test]
fn no_source_file_names_a_credential_file() {
    let broken = scan(&src_files());
    assert!(broken.is_empty(), "{}", broken.join("\n"));
}

/// A copy of `src` with `probe` added to the end of `rel` (made if missing): what the scan
/// finds in it that it does not find in `src` itself.
fn probe(rel: &str, probe: &str) -> Vec<String> {
    let mut files = src_files();
    match files.iter_mut().find(|(name, _)| name == rel) {
        Some((_, source)) => {
            source.push('\n');
            source.push_str(probe);
            source.push('\n');
        }
        None => files.push((rel.to_owned(), probe.to_owned())),
    }
    scan(&files)
}

/// The S18 security review's ways around the scan, each planted in a copy of `src`: each is
/// caught. The scan stays defence in depth, not proof (decision 0046).
#[test]
fn the_scan_catches_the_review_s_ways_around_it() {
    let in_reuse = [
        "fn f(p: &Path) { let _ = crate::write::read_small(p, 10); }",
        "fn f(p: &Path) { let _ = write::read(p); }",
        "fn f(p: &Path) { let _ = std::fs::read_dir(p); }",
        "use std::fs::File as F;",
        "fn f(p: &Path) { let _ = F::options().read(true); }",
        "fn f(p: &Path) { let _ = std::fs::File::open(p); }",
        "fn f(o: &mut Opts, p: &Path) { let _ = o.open(p); }",
        "fn f() -> std::io::Result<File> { todo!() }",
    ];
    for code in in_reuse {
        assert!(!probe(REUSE, code).is_empty(), "{code} in {REUSE}");
    }
    let anywhere = [
        "const P: &str = \"auth\\x2ejson\";",
        "const P: &str = \"auth\\u{2e}json\";",
        "const P: &str = \"auth\\x2Ejson\";",
        "fn f(h: &Path) -> PathBuf { h.join(\"auth\").with_extension(\"json\") }",
        "fn f(h: &Path) -> PathBuf {\n    h.join(\"auth\")\n        .with_extension(\"json\")\n}",
        "const P: &str = concat!(\"auth\", \".json\");",
        "const P: &str = concat!(\n    \"auth\",\n    \".json\"\n);",
    ];
    for rel in ["signin/claude.rs", REUSE, "probe.rs"] {
        for code in anywhere {
            assert!(!probe(rel, code).is_empty(), "{code} in {rel}");
        }
    }
}

/// What the new rules leave alone: Claude's `auth` subcommand beside its `--json` flag, an
/// `auth` word inside a comment, and reuse.rs's own stat, link and remove calls.
#[test]
fn the_new_rules_leave_ordinary_code_alone() {
    let fine = [
        (
            "signin/claude.rs",
            "const S: [&str; 4] = [\"--cli\", \"auth\", \"status\", \"--json\"];",
        ),
        ("signin/claude.rs", "// \"auth\" then \"json\"\nlet x = 1;"),
        (
            REUSE,
            "fn f(p: &Path) { let _ = std::fs::symlink_metadata(p); }",
        ),
        (
            REUSE,
            "fn f(a: &Path, b: &Path) { let _ = crate::write::hard_link(a, b); }",
        ),
        (
            REUSE,
            "fn f(p: &Path) -> bool { home_file(p).exists() && LOGIN_FILE.is_empty() }",
        ),
    ];
    for (rel, code) in fine {
        assert_eq!(probe(rel, code), Vec::<String>::new(), "{code} in {rel}");
    }
}

#[test]
fn the_scan_skips_comments_and_reads_literals() {
    let file = format!("\"{CODEX_FILE}\"");
    let cases = [
        (format!("// the {CODEX_FILE} link\nlet x = 1;"), 0),
        (format!("/// see {CLAUDE_FILE}\nfn f() {{}}"), 0),
        (
            format!("/* outer /* {CODEX_FILE} */ still */ let y = 2;"),
            0,
        ),
        (format!("let p = home.join({file});"), 1),
        (
            format!("let u = \"https://a.example/x\"; let p = {file};"),
            1,
        ),
        (format!("let p = r#\"{CLAUDE_FILE}\"#;"), 1),
        (
            format!("fn f<'a>(x: &'a str) -> char {{ '\"' }} const P: &str = {file};"),
            1,
        ),
        (format!("let q = \"// not a comment {CODEX_FILE}\";"), 1),
        (format!("let c = '/'; let p = {file};"), 1),
    ];
    for (source, want) in cases {
        assert_eq!(
            violations("signin/claude.rs", &code_only(&source)).len(),
            want,
            "{source}"
        );
    }
}

#[test]
fn reuse_may_name_the_codex_file_but_never_open_it() {
    let link = format!("write::hard_link(&user.join(\"{CODEX_FILE}\"), &home)?;");
    assert_eq!(violations(REUSE, &code_only(&link)), Vec::<String>::new());
    let claude = format!("let p = \"{CLAUDE_FILE}\";");
    assert_eq!(violations(REUSE, &code_only(&claude)).len(), 1);
    for call in [
        "File::open(&p)",
        "std::fs::read(&p)",
        "fs::read_to_string(&p)",
        "OpenOptions::new().read(true)",
        "write::open_read(&p)",
    ] {
        assert!(!violations(REUSE, &code_only(call)).is_empty(), "{call}");
    }
    assert_eq!(
        violations(REUSE, &code_only("// File::open is never called")),
        Vec::<String>::new()
    );
}
