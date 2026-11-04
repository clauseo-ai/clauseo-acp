//! The scan and its own tests: see the crate root.

use std::path::{Path, PathBuf};

/// Claude's credential file, never named.
const CLAUDE_FILE: &str = concat!(".credentials", ".json");

/// Codex's login file, named only by `signin/reuse.rs`.
const CODEX_FILE: &str = concat!("auth", ".json");

/// The one file that may name [`CODEX_FILE`].
const REUSE: &str = "signin/reuse.rs";

/// What `reuse.rs` may never call: an open or a read.
const OPENS: [&str; 4] = ["File::open", "fs::read", "OpenOptions", "open_read"];

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
    for (n, line) in code.lines().enumerate() {
        let at = format!("crates/agents/src/{rel}:{}", n + 1);
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
        }
    }
    found
}

#[test]
fn no_source_file_names_a_credential_file() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = rust_files(&src);
    assert!(
        files.iter().any(|(rel, _)| rel == "lib.rs"),
        "the scan found no lib.rs under {}",
        src.display()
    );
    let broken: Vec<String> = files
        .iter()
        .flat_map(|(rel, source)| violations(rel, &code_only(source)))
        .collect();
    assert!(broken.is_empty(), "{}", broken.join("\n"));
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
        assert_eq!(violations(REUSE, &code_only(call)).len(), 1, "{call}");
    }
    assert_eq!(
        violations(REUSE, &code_only("// File::open is never called")),
        Vec::<String>::new()
    );
}
