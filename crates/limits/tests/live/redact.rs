//! Redaction of a live run's raw stdout before it is written under `target/g16/`.
//!
//! Each line that is JSON is walked: an account, organisation, user, workspace or team id (a key
//! such as `accountId`, `organization_uuid`, or `id` inside an `account` object), and any string
//! shaped like a UUID, becomes `redacted-<n>`, the same `n` for the same value within one run.
//! A host name (`serverName`, `hostname`) becomes `redacted-host`, and a local path (a key
//! such as `codexHome`, `cwd`, or one ending in `path` or `dir`) becomes `redacted-path`:
//! codex app-server reports both. In every other string, and in a line that is not JSON, each
//! email becomes `test@example.com` and each token shape (`fixtures/check.rs`) becomes
//! `redacted-token`.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::check::token_run;

/// The email every email becomes.
const EMAIL: &str = "test@example.com";
/// What a token shape becomes.
const TOKEN: &str = "redacted-token";
/// Key starts that name whose id a key holds.
const SUBJECTS: [&str; 6] = [
    "account",
    "org",
    "organization",
    "user",
    "workspace",
    "team",
];
/// Bare keys that hold this machine's name.
const HOST_KEYS: [&str; 2] = ["servername", "hostname"];
/// Bare keys that hold a local path, besides those ending in `path` or `dir`.
const PATH_KEYS: [&str; 3] = ["codexhome", "cwd", "home"];

/// Redacts one run's lines, numbering the ids it replaces.
#[derive(Debug, Default)]
pub(crate) struct Redactor {
    /// Each id seen, with its number.
    ids: BTreeMap<String, usize>,
}

impl Redactor {
    /// `line`, redacted.
    pub(crate) fn line(&mut self, line: &str) -> String {
        match serde_json::from_str::<Value>(line) {
            Ok(mut value) => {
                self.value(&mut value, false, false);
                value.to_string()
            }
            Err(_) => text(line),
        }
    }

    /// Redacts `value` in place. `id` says its key names an id; `subject` that the object
    /// holding it is an account, organisation, user, workspace or team.
    fn value(&mut self, value: &mut Value, id: bool, subject: bool) {
        match value {
            Value::Object(map) => {
                for (key, child) in map.iter_mut() {
                    let bare = bare(key);
                    if let Some(with) = machine(&bare)
                        && child.is_string()
                    {
                        *child = Value::String(with.to_owned());
                        continue;
                    }
                    let id = is_id_key(&bare) || (subject && (bare == "id" || bare == "uuid"));
                    let subject = SUBJECTS.iter().any(|start| bare.starts_with(start));
                    self.value(child, id, subject);
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.value(item, id, subject);
                }
            }
            Value::String(string) if id || is_uuid(string) => *string = self.id(string),
            Value::String(string) => *string = text(string),
            Value::Number(number) if id => *value = Value::String(self.id(&number.to_string())),
            _ => {}
        }
    }

    /// `redacted-<n>` for `raw`.
    fn id(&mut self, raw: &str) -> String {
        let next = self.ids.len() + 1;
        let n = *self.ids.entry(raw.to_owned()).or_insert(next);
        format!("redacted-{n}")
    }
}

/// `key` in lower case without `_` and `-`.
fn bare(key: &str) -> String {
    key.chars()
        .filter(|c| *c != '_' && *c != '-')
        .collect::<String>()
        .to_ascii_lowercase()
}

/// What replaces a string under a bare key that names this machine or a local path.
fn machine(bare: &str) -> Option<&'static str> {
    if HOST_KEYS.contains(&bare) {
        Some("redacted-host")
    } else if PATH_KEYS.contains(&bare) || bare.ends_with("path") || bare.ends_with("dir") {
        Some("redacted-path")
    } else {
        None
    }
}

/// True for a bare key that names a subject's id: `accountid`, `organizationuuid`, `userid`.
fn is_id_key(bare: &str) -> bool {
    SUBJECTS.iter().any(|start| bare.starts_with(start))
        && (bare.ends_with("id") || bare.ends_with("uuid"))
}

/// True for `8-4-4-4-12` hex digits.
fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.iter().map(|part| part.len()).eq([8, 4, 4, 4, 12])
        && parts
            .iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// `text` with each email and token shape replaced.
pub(crate) fn text(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut at = 0;
    while at < bytes.len() {
        if let Some((start, end, with)) = match_at(bytes, at)
            && start >= copied
        {
            out.push_str(text.get(copied..start).unwrap_or_default());
            out.push_str(with);
            copied = end;
            at = end;
        } else {
            at += 1;
        }
    }
    out.push_str(text.get(copied..).unwrap_or_default());
    out
}

/// An email whose `@` is at `at`, or a token shape that starts at `at`: its span and what
/// replaces it. Spans end and start on ASCII bytes, so they are char boundaries.
fn match_at(bytes: &[u8], at: usize) -> Option<(usize, usize, &'static str)> {
    let rest = bytes.get(at..)?;
    for start in [&b"sk-"[..], b"eyJ"] {
        if rest.starts_with(start) {
            let run = token_run(rest.get(start.len()..)?);
            if run >= 20 {
                return Some((at, at + start.len() + run, TOKEN));
            }
        }
    }
    if rest.first() != Some(&b'@') {
        return None;
    }
    let local = |byte: &u8| byte.is_ascii_alphanumeric() || b"._%+-".contains(byte);
    let domain = |byte: &u8| byte.is_ascii_alphanumeric() || b".-".contains(byte);
    let before = bytes
        .get(..at)?
        .iter()
        .rev()
        .take_while(|b| local(b))
        .count();
    let after: Vec<u8> = rest.get(1..)?.iter().copied().take_while(domain).collect();
    let dotted = after.contains(&b'.') && !after.starts_with(b".");
    (before > 0 && dotted).then_some((at - before, at + 1 + after.len(), EMAIL))
}
