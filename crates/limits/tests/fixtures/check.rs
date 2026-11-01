//! The secret check over a folder of recordings: S8's check 7 as G16 extends it.
//!
//! A file fails on:
//! - an email outside `example.com`: an email shape (a local part, `@`, a dotted domain) whose
//!   domain is not `example.com`, and, in every file but the folder's own top `README.md`, any
//!   `@` not followed by `example.com` and then a byte that ends a domain (so
//!   `x@example.community` and `x@example.com.evil.io` fail). The top `README.md` names npm scopes
//!   (`@anthropic-ai/...`), which are no email shape;
//! - a token shape, `sk-` or `eyJ` followed by 20 or more of `A-Z a-z 0-9 _ -` (S8's regexes
//!   `sk-[A-Za-z0-9_-]{20,}` and `eyJ[A-Za-z0-9_-]{20,}`);
//! - a UUID (`8-4-4-4-12` hex digits), the shape of an account, organisation or install id;
//! - a home path: `/Users/`, `/home/` or `C:\Users\` (in any case, and JSON's `C:\\Users\\`);
//! - its name, when that is `auth.json` (Codex's) or `.credentials.json` (Claude Code's), in any
//!   case;
//! - being a link: a recording is a regular file.
//!
//! A finding names the file, the rule and the line, never the text.

use std::path::{Path, PathBuf};

/// File names no recording may have: the vendors' credential files.
const REFUSED_NAMES: [&str; 2] = ["auth.json", ".credentials.json"];
/// The one domain an email may have.
const EMAIL_DOMAIN: &[u8] = b"example.com";
/// The starts of the token shapes.
const TOKEN_STARTS: [&[u8]; 2] = [b"sk-", b"eyJ"];
/// How many token characters after a start make a token.
pub(crate) const TOKEN_MIN: usize = 20;
/// The starts of a home path, compared in ASCII lower case.
pub(crate) const HOME_STARTS: [&[u8]; 4] =
    [b"/users/", b"/home/", b"c:\\users\\", b"c:\\\\users\\\\"];
/// The file at the top of the folder the bare `@` rule skips.
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
    let readme = root.join(README);
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
                Ok(meta) if meta.is_file() => {
                    let bare_at = path != readme;
                    check(&path, bare_at, &mut report);
                }
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

/// Checks the one file `path`, with every rule: a live run's own recording.
pub(crate) fn check_file(path: &Path) -> Report {
    let mut report = Report::default();
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => check(path, true, &mut report),
        Ok(_) => report
            .findings
            .push(format!("{}: is not a regular file", path.display())),
        Err(err) => report
            .findings
            .push(format!("{}: does not read: {err}", path.display())),
    }
    report
}

/// Checks one regular file's name and bytes; `bare_at` turns on the bare `@` rule.
fn check(path: &Path, bare_at: bool, report: &mut Report) {
    report.files += 1;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if REFUSED_NAMES
        .iter()
        .any(|refused| name.eq_ignore_ascii_case(refused))
    {
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
    for (at, rule) in text_findings(&bytes, bare_at) {
        let line = 1 + bytes.iter().take(at).filter(|byte| **byte == b'\n').count();
        report
            .findings
            .push(format!("{}:{line}: {rule}", path.display()));
    }
}

/// Where `bytes` break a rule, and which.
fn text_findings(bytes: &[u8], bare_at: bool) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for at in 0..bytes.len() {
        let rest = bytes.get(at..).unwrap_or_default();
        if rest.first() == Some(&b'@') {
            if let Some((_, _, domain)) = email_at(bytes, at)
                && !domain.eq_ignore_ascii_case(EMAIL_DOMAIN)
            {
                found.push((at, "an email outside example.com"));
            } else if bare_at && !example_com_follows(rest.get(1..).unwrap_or_default()) {
                found.push((at, "an `@` not followed by example.com"));
            }
        }
        if TOKEN_STARTS.iter().any(|start| {
            rest.starts_with(start)
                && token_run(rest.get(start.len()..).unwrap_or_default()) >= TOKEN_MIN
        }) {
            found.push((
                at,
                "a token shape (`sk-` or `eyJ` and 20 or more token characters)",
            ));
        }
        if starts_uuid(bytes, at) {
            found.push((at, "a UUID (an id that was not redacted)"));
        }
        if home_at(rest) {
            found.push((at, "a home path (/Users/, /home/ or C:\\Users\\)"));
        }
    }
    found
}

/// True when `after` (what follows an `@`) is `example.com` and then a byte that ends a domain:
/// not a letter, a digit or `-`, and not a `.` followed by a letter or a digit.
fn example_com_follows(after: &[u8]) -> bool {
    if !after.starts_with(EMAIL_DOMAIN) {
        return false;
    }
    let next = after.get(EMAIL_DOMAIN.len()).copied();
    let then = after.get(EMAIL_DOMAIN.len() + 1).copied();
    match next {
        Some(byte) if byte.is_ascii_alphanumeric() || byte == b'-' => false,
        Some(b'.') => !then.is_some_and(|byte| byte.is_ascii_alphanumeric()),
        _ => true,
    }
}

/// True for a byte that cannot be part of an email: ASCII whitespace, a control byte, a quote,
/// or one of `<>()[]{},;:/\`. Any other byte, non-ASCII included, can.
pub(crate) fn delimits(byte: u8) -> bool {
    byte.is_ascii_whitespace() || byte.is_ascii_control() || b"\"'`<>()[]{},;:/\\@".contains(&byte)
}

/// The email whose `@` is at `at`: its start, its end (after the domain, without trailing dots)
/// and its domain. `None` when the local part is empty or the domain has no inner dot.
pub(crate) fn email_at(bytes: &[u8], at: usize) -> Option<(usize, usize, &[u8])> {
    let local = bytes
        .get(..at)?
        .iter()
        .rev()
        .take_while(|byte| !delimits(**byte))
        .count();
    let after = bytes.get(at + 1..)?;
    let mut domain_len = after.iter().take_while(|byte| !delimits(**byte)).count();
    while domain_len > 0 && after.get(domain_len - 1) == Some(&b'.') {
        domain_len -= 1;
    }
    let domain = after.get(..domain_len)?;
    let dotted = domain.iter().skip(1).any(|byte| *byte == b'.');
    (local > 0 && dotted).then_some((at - local, at + 1 + domain_len, domain))
}

/// How many token characters (`A-Z a-z 0-9 _ -`) `bytes` starts with.
pub(crate) fn token_run(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_' || **byte == b'-')
        .count()
}

/// True when a UUID starts at `at`, with no hex digit just before it or just after it.
pub(crate) fn starts_uuid(bytes: &[u8], at: usize) -> bool {
    if at > 0 && bytes.get(at - 1).is_some_and(u8::is_ascii_hexdigit) {
        return false;
    }
    let Some(candidate) = bytes.get(at..at + 36) else {
        return false;
    };
    let shape = candidate.iter().enumerate().all(|(i, byte)| match i {
        8 | 13 | 18 | 23 => *byte == b'-',
        _ => byte.is_ascii_hexdigit(),
    });
    shape && !bytes.get(at + 36).is_some_and(u8::is_ascii_hexdigit)
}

/// True when a home path starts at the start of `rest`.
pub(crate) fn home_at(rest: &[u8]) -> bool {
    HOME_STARTS.iter().any(|start| {
        rest.get(..start.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(start))
    })
}
