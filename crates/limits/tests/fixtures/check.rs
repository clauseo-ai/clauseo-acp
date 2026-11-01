//! The secret check over a folder of recordings: S8's check 7 as G16 extends it.
//!
//! A file fails on an `@` not followed by `example.com` (an email that was not redacted), on a
//! token shape, `sk-` or `eyJ` followed by 20 or more of `A-Z a-z 0-9 _ -` (S8's regexes
//! `sk-[A-Za-z0-9_-]{20,}` and `eyJ[A-Za-z0-9_-]{20,}`), and on its name when that is
//! `auth.json` (Codex's) or `.credentials.json` (Claude Code's). A link fails too: a recording is
//! a regular file. `README.md` is exempt from the `@` rule only, since it names npm scopes
//! (`@anthropic-ai/...`). A finding names the file, the rule and the line, never the text.

use std::path::{Path, PathBuf};

/// File names no recording may have: the vendors' credential files.
const REFUSED_NAMES: [&str; 2] = ["auth.json", ".credentials.json"];
/// The one domain an email may have.
const EMAIL_DOMAIN: &[u8] = b"example.com";
/// The starts of the token shapes.
const TOKEN_STARTS: [&[u8]; 2] = [b"sk-", b"eyJ"];
/// How many token characters after a start make a token.
const TOKEN_MIN: usize = 20;
/// The file the `@` rule skips.
const README: &str = "README.md";

/// What the check found under a folder.
#[derive(Debug, Default)]
pub(crate) struct Report {
    /// How many files it read.
    pub(crate) files: usize,
    /// One line per failure: the file, the rule and where.
    pub(crate) findings: Vec<String>,
}

/// Checks every file under `root`, recursively. A folder that does not read is a finding.
pub(crate) fn check_dir(root: &Path) -> Report {
    let mut report = Report::default();
    let mut todo: Vec<PathBuf> = vec![root.to_owned()];
    while let Some(dir) = todo.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) => {
                report
                    .findings
                    .push(format!("{}: does not read: {err}", dir.display()));
                continue;
            }
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for path in paths {
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => todo.push(path),
                Ok(meta) if meta.is_file() => check_file(&path, &mut report),
                Ok(_) => report
                    .findings
                    .push(format!("{}: is a link, not a regular file", path.display())),
                Err(err) => report
                    .findings
                    .push(format!("{}: does not read: {err}", path.display())),
            }
        }
    }
    report
}

/// Checks one regular file's name and bytes.
fn check_file(path: &Path, report: &mut Report) {
    report.files += 1;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if REFUSED_NAMES.contains(&name.as_str()) {
        let shown = path.display();
        report
            .findings
            .push(format!("{shown}: a credential file's name ({name})"));
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            let shown = path.display();
            report
                .findings
                .push(format!("{shown}: does not read: {err}"));
            return;
        }
    };
    for (at, rule) in text_findings(&bytes, name != README) {
        let line = 1 + bytes.iter().take(at).filter(|byte| **byte == b'\n').count();
        report
            .findings
            .push(format!("{}:{line}: {rule}", path.display()));
    }
}

/// Where `bytes` break a rule, and which: an `@` outside `example.com` when `emails` is set,
/// and each token shape.
fn text_findings(bytes: &[u8], emails: bool) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for at in 0..bytes.len() {
        let rest = bytes.get(at..).unwrap_or_default();
        if emails
            && rest.first() == Some(&b'@')
            && !rest.get(1..).unwrap_or_default().starts_with(EMAIL_DOMAIN)
        {
            found.push((at, "an `@` not followed by example.com"));
        }
        for start in TOKEN_STARTS {
            if rest.starts_with(start)
                && token_run(rest.get(start.len()..).unwrap_or_default()) >= TOKEN_MIN
            {
                found.push((
                    at,
                    "a token shape (`sk-` or `eyJ` and 20 or more token characters)",
                ));
            }
        }
    }
    found
}

/// How many token characters (`A-Z a-z 0-9 _ -`) `bytes` starts with.
pub(crate) fn token_run(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_' || **byte == b'-')
        .count()
}
