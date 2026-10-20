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
//!
//! The app's permission files are held to the same rules, since a capability can grant a set
//! of them by its bare name (`"all"`) and a set may bundle other plugins' permissions
//! (tauri-utils 2.10.2 `acl/resolved.rs:447-459`). tauri-build 2.7.3 reads them from the
//! crate's `permissions/**/*` (`acl.rs:304-320`); tauri-utils keeps the `*.json` and `*.toml`
//! ones (`acl/build.rs:36`, case-sensitive, hidden files too, as glob 0.3 matches them) and skips
//! a file whose folder is named `schemas` (`acl/build.rs:88`). Moving them with
//! `permissions_path_pattern` is banned in clippy.toml's `disallowed-methods`, as
//! `capabilities_path_pattern` is.

mod fix;
pub(crate) use fix::Fix;

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

/// The app's permission folder, inside the desktop crate's directory.
const PERMISSIONS_DIR: &str = "permissions";

/// A folder of this name under `PERMISSIONS_DIR` holds schemas, which Tauri skips.
const SCHEMAS_DIR: &str = "schemas";

/// The Tauri config, inside the desktop crate's directory. Its `app.security.capabilities` may
/// hold capability objects inline.
const TAURI_CONF: &str = "tauri.conf.json";

/// The extensions of the capability files Tauri reads.
const EXTENSIONS: [&str; 3] = ["json", "json5", "toml"];

/// The extensions of the permission files Tauri reads.
const PERMISSION_EXTENSIONS: [&str; 2] = ["json", "toml"];

/// Permission prefixes that name a plugin's API. The webview gets none of them.
const FORBIDDEN_PREFIXES: [&str; 5] = ["fs:", "shell:", "http:", "opener:", "dialog:"];

/// Appended to a message whose string is not found in the raw text, which spells it with escapes.
const ESCAPED: &str = " (written with escapes)";

/// Fails on every forbidden string in a capability file.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "capabilities",
    counts: Noun {
        one: "capability or permission file",
        many: "capability and permission files",
    },
    place: || format!("in the desktop crate's {DIR}/ and {PERMISSIONS_DIR}/"),
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

/// Counts the capability and permission files read; the inline objects in the Tauri config are
/// not counted. Permission files beside a capabilities folder with no capability file fail that
/// folder, so they cannot stand in for the capabilities this check guards.
fn run(ctx: &Context) -> Inspection {
    let desktop = match checks::desktop_dir(ctx) {
        Ok(dir) => dir,
        Err(stopped) => return stopped,
    };
    let rel = checks::join(desktop, DIR);
    let files = match files_in(&ctx.root.join(&rel), is_capability_file) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Inspection::stopped(checks::violation(
                &rel,
                1,
                format!("the capabilities folder `{rel}` is missing"),
                Fix::FolderMissing,
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
    let permissions_rel = checks::join(desktop, PERMISSIONS_DIR);
    let permissions = match files_in(&ctx.root.join(&permissions_rel), is_permission_file) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            found.push(checks::unreadable(&permissions_rel, &error));
            Vec::new()
        }
    };
    if read == 0 && !permissions.is_empty() {
        found.push(checks::violation(
            &rel,
            1,
            format!("the capabilities folder `{rel}` holds no capability file"),
            Fix::NoCapabilityFile,
        ));
    }
    for file in &permissions {
        let path = checks::repo_path(&ctx.root, file);
        match fs::read_to_string(file) {
            Ok(text) => {
                read += 1;
                found.extend(scan_as(Kind::Permission, &path, &text));
            }
            Err(error) => found.push(checks::unreadable(&path, &error)),
        }
    }
    Inspection::new(read, found)
}

/// Every file under `dir` that `keep` accepts, recursively, sorted by path. Fails when `dir`
/// cannot be listed.
fn files_in(dir: &Path, keep: fn(&Path) -> bool) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if keep(&path) {
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

/// True when Tauri would load `path` as an app permission file: its extension is one of
/// `PERMISSION_EXTENSIONS`, and its folder is not named `SCHEMAS_DIR`.
fn is_permission_file(path: &Path) -> bool {
    let extension = path.extension().and_then(|extension| extension.to_str());
    let folder = path.parent().and_then(Path::file_name);
    extension.is_some_and(|extension| PERMISSION_EXTENSIONS.contains(&extension))
        && folder.is_none_or(|folder| folder != SCHEMAS_DIR)
}

/// The kind of file a string is in, which its messages name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A capability file, or a capability written inline in the Tauri config.
    Capability,
    /// An app permission file, whose sets and defaults a capability grants by name.
    Permission,
}

/// The violations in one capability file. `path` is repo-relative and `text` is the file; the
/// extension of `path` picks the parser.
pub(crate) fn scan(path: &str, text: &str) -> Vec<Violation> {
    scan_as(Kind::Capability, path, text)
}

/// The violations in one file of `kind`, as `scan` finds them.
fn scan_as(kind: Kind, path: &str, text: &str) -> Vec<Violation> {
    let at_one = |message: String, fix: Fix| vec![checks::violation(path, 1, message, fix)];
    let extension = Path::new(path).extension().and_then(|ext| ext.to_str());
    let value = if extension == Some("toml") {
        match toml::from_str::<toml::Value>(text).map(serde_json::to_value) {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                return at_one(format!("could not read `{path}`: {error}"), Fix::Syntax);
            }
            Err(error) => {
                let line = error
                    .span()
                    .map_or(1, |span| checks::line_at(line_index_of(text, span.start)));
                let message = format!("`{path}` is not valid TOML: {}", error.message());
                return vec![checks::violation(path, line, message, Fix::Syntax)];
            }
        }
    } else {
        match serde_json::from_str::<Value>(text) {
            Ok(value) => value,
            Err(_) if extension == Some("json5") => {
                return at_one(
                    "json5 capability files are not supported; use .json".to_owned(),
                    Fix::Json5,
                );
            }
            Err(error) => {
                let line = u32::try_from(error.line()).unwrap_or(1).max(1);
                let message = format!("`{path}` is not valid JSON: {error}");
                return vec![checks::violation(path, line, message, Fix::Syntax)];
            }
        }
    };
    let mut findings = Vec::new();
    walk(kind, &value, None, &mut findings);
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
        walk(Kind::Capability, object, None, &mut findings);
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
    problems: Vec<(String, Fix)>,
}

/// Collects a finding for every forbidden key and string value in `value`, at any depth. `key`
/// is the key that holds `value`, if any; array elements keep their array's key.
fn walk(kind: Kind, value: &Value, key: Option<&str>, found: &mut Vec<Finding>) {
    match value {
        Value::Object(map) => {
            for (name, child) in map {
                push(found, name, None, problems(kind, name, false));
                walk(kind, child, Some(name), found);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(kind, item, key, found);
            }
        }
        Value::String(text) => push(found, text, key, problems(kind, text, true)),
        _ => {}
    }
}

/// Adds a finding for `needle` when `problems` is not empty.
fn push(found: &mut Vec<Finding>, needle: &str, key: Option<&str>, problems: Vec<(String, Fix)>) {
    if !problems.is_empty() {
        found.push(Finding {
            needle: needle.to_owned(),
            key: key.map(str::to_owned),
            problems,
        });
    }
}

/// The messages and fixes for one decoded key or string value in a file of `kind`. Only a value
/// can grant a permission by its prefix. A permission file names what a capability grants
/// through it.
fn problems(kind: Kind, text: &str, is_value: bool) -> Vec<(String, Fix)> {
    let (file, grants, remote_fix) = match kind {
        Kind::Capability => ("capability file", "capability grants", Fix::Remote),
        Kind::Permission => (
            "permission file",
            "permission file names",
            Fix::RemotePermission,
        ),
    };
    let mut found = Vec::new();
    if text.contains("remote") {
        found.push((format!("{file} mentions `remote`"), remote_fix));
    }
    if text.contains("core:default") {
        found.push((format!("{grants} `core:default`"), Fix::CoreDefault));
    }
    if is_value
        && FORBIDDEN_PREFIXES
            .iter()
            .any(|prefix| text.starts_with(prefix))
    {
        found.push((format!("{grants} `{text}`"), Fix::ForbiddenPrefix));
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
