//! The `capabilities` check: no capability file grants what the webview must never reach.
//!
//! Each line is checked on its own. A line with `remote`, with `core:default`, and with a
//! permission under a forbidden prefix fails once for each of them.
//!
//! Capability objects written inline in `app.security.capabilities` of `tauri.conf.json` are
//! scanned the same way, at their lines in that file.
//!
//! Only `*.json`, `*.json5` and `*.toml` files are scanned, at any depth: those are the only files
//! Tauri loads from the capabilities folder. Anything else there (a Finder `.DS_Store`, a Vim swap
//! file, a `main.json~` backup) never reaches the app, so it is skipped.

use crate::checks::json_text::{self, key_line};
use crate::checks::{self, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
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

/// Fails on every forbidden string in a capability file.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "capabilities",
    run,
};

fn run(ctx: &Context) -> Vec<Violation> {
    let desktop = match checks::desktop_dir(ctx) {
        Ok(dir) => dir,
        Err(violation) => return vec![violation],
    };
    let rel = checks::join(desktop, DIR);
    let files = match capability_files(&ctx.root.join(&rel)) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return vec![checks::violation(
                &rel,
                1,
                format!("the capabilities folder `{rel}` is missing"),
                "restore it with main.json: Tauri reads the window's grants from it, and this check guards them",
            )];
        }
        Err(error) => return vec![checks::unreadable(&rel, &error)],
    };
    let mut found = Vec::new();
    let conf = checks::join(desktop, TAURI_CONF);
    // A missing or unreadable config is the `tauri-config` check's failure, reported there once.
    if let Ok(Some(text)) = checks::read_repo_file(&ctx.root, &conf) {
        found.extend(inline_violations(&conf, &text));
    }
    for file in &files {
        let path = checks::repo_path(&ctx.root, file);
        match fs::read_to_string(file) {
            Ok(text) => found.extend(scan(&path, &text)),
            Err(error) => found.push(checks::unreadable(&path, &error)),
        }
    }
    found
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

/// The violations in one capability file. `path` is repo-relative and `text` is the file.
pub(crate) fn scan(path: &str, text: &str) -> Vec<Violation> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut problems: Vec<(String, &str)> = Vec::new();
        if line.contains("remote") {
            problems.push(("capability file mentions `remote`".to_owned(), FIX_REMOTE));
        }
        if line.contains("core:default") {
            problems.push(("capability grants `core:default`".to_owned(), FIX_CORE));
        }
        problems.extend(
            forbidden_permissions(line)
                .into_iter()
                .map(|permission| (format!("capability grants `{permission}`"), FIX_PREFIX)),
        );
        found.extend(problems.into_iter().map(|(message, fix)| Violation {
            path: path.to_owned(),
            line: checks::line_at(index),
            message,
            fix: fix.to_owned(),
        }));
    }
    found
}

/// The violations in the capability objects written inline in `app.security.capabilities` of the
/// Tauri config at `path`, each scanned like a capability file, at its own lines. String elements
/// name capability files and pass. A config that is not valid JSON passes here: `tauri-config`
/// fails on it. When an object cannot be located in the text, its violations point at the
/// `capabilities` key.
pub(crate) fn inline_violations(path: &str, text: &str) -> Vec<Violation> {
    let keys = ["app", "security", "capabilities"];
    let Ok(conf) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let objects: Vec<&serde_json::Value> = conf
        .pointer("/app/security/capabilities")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|element| element.is_object())
        .collect();
    let spans =
        json_text::object_elements(text, &keys).filter(|spans| spans.len() == objects.len());
    let Some(spans) = spans else {
        let line = key_line(text, &keys);
        return objects
            .iter()
            .flat_map(|object| {
                scan(
                    path,
                    &serde_json::to_string_pretty(object).unwrap_or_default(),
                )
            })
            .map(|found| Violation { line, ..found })
            .collect();
    };
    let mut found = Vec::new();
    for span in spans {
        let offset = u32::try_from(json_text::line_index_of(text, span.start)).unwrap_or(u32::MAX);
        let object = text.get(span).unwrap_or_default();
        found.extend(scan(path, object).into_iter().map(|violation| Violation {
            line: violation.line.saturating_add(offset),
            ..violation
        }));
    }
    found
}

/// The permission strings on `line` that start with a forbidden prefix, each up to its closing
/// quote. A prefix counts only right after a `"` or a `'`, so prose such as `refs:` does not match.
fn forbidden_permissions(line: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for (start, quote) in line.char_indices().filter(|(_, c)| *c == '"' || *c == '\'') {
        let Some(rest) = line.get(start + quote.len_utf8()..) else {
            continue;
        };
        if FORBIDDEN_PREFIXES
            .iter()
            .any(|prefix| rest.starts_with(prefix))
        {
            let end = rest.find(quote).unwrap_or(rest.len());
            found.push(rest.get(..end).unwrap_or(rest));
        }
    }
    found
}

#[cfg(test)]
mod tests;
