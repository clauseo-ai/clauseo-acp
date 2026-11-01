//! Redaction of a live run's raw stdout before it is written under `target/g16/`.
//!
//! First, each of the run's own local values (the login folder, the install, node's path, the
//! temp folder and `$HOME`, longest first) is replaced wherever it appears with `redacted-path`.
//! Then each line that is JSON is walked, keys included: an account, organisation, user,
//! workspace or team id (a key such as `accountId`, `organization_uuid`, or `id` inside an
//! `account` object) becomes `redacted-<n>`, the same `n` for the same value within one run; a
//! host name (`serverName`, `hostname`) becomes `redacted-host`; and a local path (a key such as
//! `codexHome`, `cwd`, or one ending in `path` or `dir`) becomes `redacted-path`, since codex
//! app-server reports both. In every other string and key, and in a line that is not JSON:
//! each email (any run of non-delimiter bytes, `@`, and a dotted domain;
//! `fixtures/check.rs`'s `delimits`) becomes `test@example.com`, each UUID `redacted-<n>`, each
//! token shape `redacted-token`, and each home path (`/Users/...`, `/home/...`, `C:\Users\...`, up
//! to the next delimiter) `redacted-path`.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::check::{TOKEN_MIN, email_at, home_at, starts_uuid, token_run};

/// The email every email becomes.
const EMAIL: &str = "test@example.com";
/// What a token shape becomes.
const TOKEN: &str = "redacted-token";
/// What a local path becomes.
const PATH: &str = "redacted-path";
/// What a host name becomes.
const HOST: &str = "redacted-host";
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
    /// The run's own local values, longest first.
    known: Vec<String>,
    /// Each id seen, with its number.
    ids: BTreeMap<String, usize>,
}

impl Redactor {
    /// A redactor that also replaces each of `known` (the run's local paths) literally. Empty
    /// values are dropped.
    pub(crate) fn new(known: impl IntoIterator<Item = String>) -> Redactor {
        let mut known: Vec<String> = known
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect();
        known.sort_by_key(|value| std::cmp::Reverse(value.len()));
        known.dedup();
        Redactor {
            known,
            ids: BTreeMap::new(),
        }
    }

    /// `line`, redacted.
    pub(crate) fn line(&mut self, line: &str) -> String {
        let line = self.literal(line);
        match serde_json::from_str::<Value>(&line) {
            Ok(mut value) => {
                self.value(&mut value, false, false);
                value.to_string()
            }
            Err(_) => self.text(&line),
        }
    }

    /// `text` with each known local value replaced.
    fn literal(&self, text: &str) -> String {
        self.known.iter().fold(text.to_owned(), |text, known| {
            text.replace(known.as_str(), PATH)
        })
    }

    /// Redacts `value` in place. `id` says its key names an id; `subject` that the object
    /// holding it is an account, organisation, user, workspace or team.
    fn value(&mut self, value: &mut Value, id: bool, subject: bool) {
        match value {
            Value::Object(map) => {
                let mut redacted = Map::new();
                for (key, mut child) in std::mem::take(map) {
                    let bare = bare(&key);
                    match machine(&bare) {
                        Some(with) if child.is_string() => child = Value::String(with.to_owned()),
                        _ => {
                            let id =
                                is_id_key(&bare) || (subject && (bare == "id" || bare == "uuid"));
                            let subject = SUBJECTS.iter().any(|start| bare.starts_with(start));
                            self.value(&mut child, id, subject);
                        }
                    }
                    redacted.insert(self.text(&key), child);
                }
                *map = redacted;
            }
            Value::Array(items) => {
                for item in items {
                    self.value(item, id, subject);
                }
            }
            Value::String(string) if id => *string = self.id(string),
            Value::String(string) => *string = self.text(string),
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

    /// `text` with each known value, email, UUID, token shape and home path replaced. Every
    /// span starts and ends at an ASCII byte or an end, so it is on char boundaries.
    pub(crate) fn text(&mut self, text: &str) -> String {
        let text = self.literal(text);
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        let mut at = 0;
        while at < bytes.len() {
            match self.match_at(bytes, at) {
                Some((start, end, with)) if start >= copied => {
                    out.push_str(text.get(copied..start).unwrap_or_default());
                    out.push_str(&with);
                    copied = end;
                    at = end;
                }
                _ => at += 1,
            }
        }
        out.push_str(text.get(copied..).unwrap_or_default());
        out
    }

    /// What starts at `at` (or, for an email, has its `@` there): its span and what replaces it.
    fn match_at(&mut self, bytes: &[u8], at: usize) -> Option<(usize, usize, String)> {
        let rest = bytes.get(at..)?;
        for start in [&b"sk-"[..], b"eyJ"] {
            if rest.starts_with(start) {
                let run = token_run(rest.get(start.len()..)?);
                if run >= TOKEN_MIN {
                    return Some((at, at + start.len() + run, TOKEN.to_owned()));
                }
            }
        }
        if starts_uuid(bytes, at) {
            let raw = String::from_utf8_lossy(rest.get(..36)?).into_owned();
            return Some((at, at + 36, self.id(&raw)));
        }
        if home_at(rest) {
            let len = rest
                .iter()
                .take_while(|byte| !byte.is_ascii_whitespace() && !b"\"'`<>,;".contains(byte))
                .count();
            return Some((at, at + len, PATH.to_owned()));
        }
        if rest.first() == Some(&b'@') {
            let (start, end, _) = email_at(bytes, at)?;
            return Some((start, end, EMAIL.to_owned()));
        }
        None
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
        Some(HOST)
    } else if PATH_KEYS.contains(&bare) || bare.ends_with("path") || bare.ends_with("dir") {
        Some(PATH)
    } else {
        None
    }
}

/// True for a bare key that names a subject's id: `accountid`, `organizationuuid`, `userid`.
fn is_id_key(bare: &str) -> bool {
    SUBJECTS.iter().any(|start| bare.starts_with(start))
        && (bare.ends_with("id") || bare.ends_with("uuid"))
}
