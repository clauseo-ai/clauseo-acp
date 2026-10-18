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

/// The capability files, at the repo root.
const DIR: &str = "crates/desktop/capabilities";

/// The extensions of the capability files Tauri reads.
const EXTENSIONS: [&str; 3] = ["json", "json5", "toml"];

/// Permission prefixes that name a plugin's API. The webview gets none of them.
const FORBIDDEN_PREFIXES: [&str; 5] = ["fs:", "shell:", "http:", "opener:", "dialog:"];

const FIX_CORE: &str = "list only the narrow permissions the window needs; `core:default` also grants event emit and devtools toggling";
const FIX_REMOTE: &str = "remove the `remote` block: a remote page must never reach the IPC";
const FIX_PREFIX: &str = "do it in Rust behind a validated command; the webview gets no fs, shell, http, opener or dialog permission";

/// Fails on every forbidden string in a capability file.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "capabilities",
    run,
};

fn run(ctx: &Context) -> Vec<Violation> {
    let dir = ctx.root.join(DIR);
    let files = match capability_files(&dir) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => return vec![checks::unreadable(DIR, &error)],
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
            problems.push(("capability allows `remote` URLs".to_owned(), FIX_REMOTE));
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
mod tests {
    use super::*;
    use crate::scratch::Scratch;
    use crate::workspace::Context;

    const PATH: &str = "crates/desktop/capabilities/main.json";

    const CLEAN: &str = r#"{
  "identifier": "main",
  "description": "Grants nothing: refs: and prose are fine.",
  "windows": ["main"],
  "permissions": []
}"#;

    fn violation(line: u32, message: &str, fix: &str) -> Violation {
        Violation {
            path: PATH.to_owned(),
            line,
            message: message.to_owned(),
            fix: fix.to_owned(),
        }
    }

    #[test]
    fn a_capability_with_an_empty_list_passes() {
        assert_eq!(scan(PATH, CLEAN), Vec::new());
    }

    #[test]
    fn core_default_fails_at_its_line() {
        let text = "{\n  \"permissions\": [\n    \"core:default\"\n  ]\n}";
        assert_eq!(
            scan(PATH, text),
            vec![violation(3, "capability grants `core:default`", FIX_CORE)]
        );
    }

    #[test]
    fn remote_fails_at_its_line() {
        let text = "{\n  \"remote\": { \"urls\": [\"https://*\"] }\n}";
        assert_eq!(
            scan(PATH, text),
            vec![violation(2, "capability allows `remote` URLs", FIX_REMOTE)]
        );
    }

    #[test]
    fn a_quoted_fs_permission_fails_and_names_the_whole_string() {
        let text = "  \"permissions\": [\"fs:allow-read\"]";
        assert_eq!(
            scan(PATH, text),
            vec![violation(
                1,
                "capability grants `fs:allow-read`",
                FIX_PREFIX
            )]
        );
    }

    #[test]
    fn an_identifier_object_with_a_prefix_fails_and_single_quotes_count() {
        let object = "{ \"identifier\": \"fs:scope\" }";
        assert_eq!(
            scan(PATH, object)[0].message,
            "capability grants `fs:scope`"
        );
        let single = "permissions = ['shell:allow-open']";
        assert_eq!(
            scan(PATH, single)[0].message,
            "capability grants `shell:allow-open`"
        );
    }

    #[test]
    fn a_line_with_core_default_and_a_prefix_fails_twice() {
        let text = "[\"core:default\", \"dialog:allow-ask\"]";
        let messages: Vec<String> = scan(PATH, text).into_iter().map(|v| v.message).collect();
        assert_eq!(
            messages,
            [
                "capability grants `core:default`",
                "capability grants `dialog:allow-ask`"
            ]
        );
    }

    #[test]
    fn the_check_is_registered_under_its_name() {
        assert!(
            crate::checks::ALL
                .iter()
                .any(|check| check.name == "capabilities")
        );
    }

    #[test]
    fn a_ds_store_and_a_backup_file_are_ignored() {
        let dir = Scratch::new("capabilities-ignored");
        let caps = dir.join(DIR);
        fs::create_dir_all(&caps).expect("create the capability dir");
        fs::write(caps.join("main.json"), CLEAN).expect("write main.json");
        fs::write(
            caps.join(".DS_Store"),
            b"\x00\x00\x00\x01Bud1\xff\xfe\x00remote",
        )
        .expect("write .DS_Store");
        fs::write(caps.join("main.json~"), "[\"core:default\"]").expect("write main.json~");
        let ctx = Context {
            root: dir.to_path_buf(),
            members: Vec::new(),
        };
        assert_eq!(run(&ctx), Vec::new());
    }

    #[test]
    fn a_missing_directory_passes() {
        let dir = Scratch::new("capabilities-missing");
        let ctx = Context {
            root: dir.to_path_buf(),
            members: Vec::new(),
        };
        assert_eq!(run(&ctx), Vec::new());
    }

    #[test]
    fn run_reads_nested_capability_files_and_reports_their_repo_path() {
        let dir = Scratch::new("capabilities-run");
        let nested = dir.join("crates/desktop/capabilities/extra");
        fs::create_dir_all(&nested).expect("create the capability dirs");
        fs::write(dir.join("crates/desktop/capabilities/main.json"), CLEAN)
            .expect("write main.json");
        fs::write(
            nested.join("local.toml"),
            "permissions = [\"fs:allow-read\"]\n",
        )
        .expect("write local.toml");
        let ctx = Context {
            root: dir.to_path_buf(),
            members: Vec::new(),
        };
        assert_eq!(
            run(&ctx),
            vec![violation(
                1,
                "capability grants `fs:allow-read`",
                FIX_PREFIX
            )]
            .into_iter()
            .map(|v| Violation {
                path: "crates/desktop/capabilities/extra/local.toml".to_owned(),
                ..v
            })
            .collect::<Vec<_>>()
        );
    }
}
