//! The `capabilities` check: no capability file grants what the webview must never reach.
//!
//! Each file is parsed (`.json` with `serde_json`, `.toml` with `toml`), so escapes are decoded
//! as Tauri decodes them, and every key and string value is checked: one that contains `remote` or
//! `core:default`, and a string value that starts with a forbidden prefix, fails once each. A
//! `.json5` file passes only when it is plain JSON. The raw text is used only to find the line: the
//! string's own, else its key's, else line 1, the last two marked "(written with escapes)".
//!
//! Capability objects written inline in `app.security.capabilities` of `tauri.conf.json` are
//! walked the same way, at the line of the matching string, else of the `capabilities` key.
//!
//! Only `*.json`, `*.json5` and `*.toml` files are scanned, at any depth: those are the only files
//! Tauri loads from the capabilities folder. Anything else there (a Finder `.DS_Store`, a Vim swap
//! file, a `main.json~` backup) never reaches the app, so it is skipped.

use crate::checks::json_text::{key_line, line_index_of};
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The capability folder, inside the desktop crate's directory.
const DIR: &str = "capabilities";

/// The Tauri config, inside the desktop crate's directory. Its `app.security.capabilities` may
/// hold capability objects inline.
const TAURI_CONF: &str = "tauri.conf.json";

/// The extensions of the capability files Tauri reads.
const EXTENSIONS: [&str; 3] = ["json", "json5", "toml"];

/// Permission prefixes that name a plugin's API. The webview gets none of them.
const FORBIDDEN_PREFIXES: [&str; 5] = ["fs:", "shell:", "http:", "opener:", "dialog:"];

const FIX_CORE: &str = "list only the narrow permissions the window needs; `core:default` also grants event emit and devtools toggling";
const FIX_REMOTE: &str = "remove the remote block, or reword the text: a remote page must never reach the IPC, and `remote` is matched anywhere";
const FIX_PREFIX: &str = "do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission";
const FIX_JSON5: &str = "rename it to .json and write plain JSON: xtask cannot decode json5, so it cannot check the grants";
const FIX_SYNTAX: &str = "fix the syntax: Tauri cannot read the file either";

/// Appended to a message whose string is not found in the raw text, which spells it with escapes.
const ESCAPED: &str = " (written with escapes)";

/// Fails on every forbidden string in a capability file.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "capabilities",
    counts: Noun {
        one: "capability file",
        many: "capability files",
    },
    place: || format!("in the desktop crate's {DIR}/"),
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

/// Counts the capability files read; the inline objects in the Tauri config are not counted.
fn run(ctx: &Context) -> Inspection {
    let desktop = match checks::desktop_dir(ctx) {
        Ok(dir) => dir,
        Err(stopped) => return stopped,
    };
    let rel = checks::join(desktop, DIR);
    let files = match capability_files(&ctx.root.join(&rel)) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Inspection::stopped(checks::violation(
                &rel,
                1,
                format!("the capabilities folder `{rel}` is missing"),
                "restore it with main.json: Tauri reads the window's grants from it, and this check guards them",
            ));
        }
        Err(error) => return Inspection::stopped(checks::unreadable(&rel, &error)),
    };
    let mut read = 0;
    let mut found = Vec::new();
    let conf = checks::join(desktop, TAURI_CONF);
    // A missing or unreadable config is the `tauri-config` check's failure, reported there once.
    if let Ok(Some(text)) = checks::read_repo_file(&ctx.root, &conf) {
        found.extend(inline_violations(&conf, &text));
    }
    for file in &files {
        let path = checks::repo_path(&ctx.root, file);
        match fs::read_to_string(file) {
            Ok(text) => {
                read += 1;
                found.extend(scan(&path, &text));
            }
            Err(error) => found.push(checks::unreadable(&path, &error)),
        }
    }
    Inspection::new(read, found)
}

/// Every file under `dir` with one of `EXTENSIONS`, recursively, sorted by path. Fails when `dir`
/// cannot be listed.
fn capability_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if is_capability_file(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// True when Tauri would load `path` as a capability file: its extension is one of `EXTENSIONS`.
fn is_capability_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| EXTENSIONS.contains(&extension))
}

/// The violations in one capability file. `path` is repo-relative and `text` is the file; the
/// extension of `path` picks the parser.
pub(crate) fn scan(path: &str, text: &str) -> Vec<Violation> {
    let at_one = |message: String, fix: &str| vec![checks::violation(path, 1, message, fix)];
    let extension = Path::new(path).extension().and_then(|ext| ext.to_str());
    let value = if extension == Some("toml") {
        match toml::from_str::<toml::Value>(text).map(serde_json::to_value) {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                return at_one(format!("could not read `{path}`: {error}"), FIX_SYNTAX);
            }
            Err(error) => {
                let line = error
                    .span()
                    .map_or(1, |span| checks::line_at(line_index_of(text, span.start)));
                let message = format!("`{path}` is not valid TOML: {}", error.message());
                return vec![checks::violation(path, line, message, FIX_SYNTAX)];
            }
        }
    } else {
        match serde_json::from_str::<Value>(text) {
            Ok(value) => value,
            Err(_) if extension == Some("json5") => {
                return at_one(
                    "json5 capability files are not supported; use .json".to_owned(),
                    FIX_JSON5,
                );
            }
            Err(error) => {
                let line = u32::try_from(error.line()).unwrap_or(1).max(1);
                let message = format!("`{path}` is not valid JSON: {error}");
                return vec![checks::violation(path, line, message, FIX_SYNTAX)];
            }
        }
    };
    let mut findings = Vec::new();
    walk(&value, None, &mut findings);
    locate(path, text, 0, None, findings)
}

/// The violations in the capability objects written inline in `app.security.capabilities` of the
/// Tauri config at `path`. String elements name capability files and pass. A config that is not
/// valid JSON passes here: `tauri-config` fails on it. Each violation points at the line of its
/// string, searched from the `capabilities` key on, else at that key.
pub(crate) fn inline_violations(path: &str, text: &str) -> Vec<Violation> {
    let keys = ["app", "security", "capabilities"];
    let Ok(conf) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for object in conf
        .pointer("/app/security/capabilities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|element| element.is_object())
    {
        walk(object, None, &mut findings);
    }
    let line = key_line(text, &keys);
    let from: usize = text
        .split_inclusive('\n')
        .take(usize::try_from(line.saturating_sub(1)).unwrap_or(0))
        .map(str::len)
        .sum();
    locate(path, text, from, Some(line), findings)
}

/// A key or string value that trips one or more rules, before it is located in the text.
struct Finding {
    /// The decoded key or string.
    needle: String,
    /// The key that holds a string value; `None` for a key.
    key: Option<String>,
    /// The message and fix of each rule it trips, all reported at the string's one location.
    problems: Vec<(String, &'static str)>,
}

/// Collects a finding for every forbidden key and string value in `value`, at any depth. `key`
/// is the key that holds `value`, if any; array elements keep their array's key.
fn walk(value: &Value, key: Option<&str>, found: &mut Vec<Finding>) {
    match value {
        Value::Object(map) => {
            for (name, child) in map {
                push(found, name, None, problems(name, false));
                walk(child, Some(name), found);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(item, key, found);
            }
        }
        Value::String(text) => push(found, text, key, problems(text, true)),
        _ => {}
    }
}

/// Adds a finding for `needle` when `problems` is not empty.
fn push(
    found: &mut Vec<Finding>,
    needle: &str,
    key: Option<&str>,
    problems: Vec<(String, &'static str)>,
) {
    if !problems.is_empty() {
        found.push(Finding {
            needle: needle.to_owned(),
            key: key.map(str::to_owned),
            problems,
        });
    }
}

/// The messages and fixes for one decoded key or string value. Only a value can grant a
/// permission by its prefix.
fn problems(text: &str, is_value: bool) -> Vec<(String, &'static str)> {
    let mut found = Vec::new();
    if text.contains("remote") {
        found.push(("capability file mentions `remote`".to_owned(), FIX_REMOTE));
    }
    if text.contains("core:default") {
        found.push(("capability grants `core:default`".to_owned(), FIX_CORE));
    }
    if is_value
        && FORBIDDEN_PREFIXES
            .iter()
            .any(|prefix| text.starts_with(prefix))
    {
        found.push((format!("capability grants `{text}`"), FIX_PREFIX));
    }
    found
}

/// The violations for `findings`. Each finding is located once, and all its problems are reported
/// there: at the line in `text` (searched from byte `from` on) of its string, else of its key,
/// else at `fallback` or line 1; the last two say the string is written with escapes. Sorted by
/// line, in walk order within a line.
fn locate(
    path: &str,
    text: &str,
    from: usize,
    fallback: Option<u32>,
    findings: Vec<Finding>,
) -> Vec<Violation> {
    let mut seen = HashMap::new();
    let mut found: Vec<Violation> = Vec::new();
    for finding in findings {
        let (line, suffix) = find(text, from, &finding.needle, &mut seen).map_or_else(
            || {
                let line = finding
                    .key
                    .as_deref()
                    .filter(|_| fallback.is_none())
                    .and_then(|key| find(text, from, key, &mut HashMap::new()))
                    .or(fallback)
                    .unwrap_or(1);
                (line, ESCAPED)
            },
            |line| (line, ""),
        );
        found.extend(
            finding
                .problems
                .into_iter()
                .map(|(message, fix)| checks::violation(path, line, message + suffix, fix)),
        );
    }
    found.sort_by_key(|violation| violation.line);
    found
}

/// The line of the next unclaimed occurrence of `needle` in `text` from byte `from`, spelled as
/// the first of these that occurs at all: quoted with `"`, quoted with `'`, bare (a TOML key).
/// `seen` counts the occurrences already claimed, one per decoded string, so a string written
/// twice points at both of its lines, and a third occurrence of it at none.
fn find(text: &str, from: usize, needle: &str, seen: &mut HashMap<String, usize>) -> Option<u32> {
    let rest = text.get(from..)?;
    let candidate = [
        format!("\"{needle}\""),
        format!("'{needle}'"),
        needle.to_owned(),
    ]
    .into_iter()
    .find(|candidate| rest.contains(candidate.as_str()))?;
    let claimed = seen.get(&candidate).copied().unwrap_or(0);
    let (offset, _) = rest.match_indices(candidate.as_str()).nth(claimed)?;
    seen.insert(candidate, claimed + 1);
    Some(checks::line_at(line_index_of(text, from + offset)))
}

#[cfg(test)]
mod tests;
