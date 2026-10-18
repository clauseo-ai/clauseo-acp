//! The `capabilities` check: no capability file grants what the webview must never reach.
//!
//! Each line is checked on its own. A line with `remote`, with `core:default`, and with a
//! permission under a forbidden prefix fails once for each of them.
//!
//! Only `*.json`, `*.json5` and `*.toml` files are scanned, at any depth: those are the only files
//! Tauri loads from the capabilities folder. Anything else there (a Finder `.DS_Store`, a Vim swap
//! file, a `main.json~` backup) never reaches the app, so it is skipped.

use crate::checks::{self, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The capability folder, inside the desktop crate's directory.
const DIR: &str = "capabilities";

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
