//! What the stderr tail hides: the home folder, the values of the spec's secret variables, words
//! starting `sk-` and the word after `Bearer ` (in any case).

use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

/// What a secret becomes.
const REDACTED: &str = "[redacted]";
/// A variable whose name holds one of these, in any case, holds a secret.
const SECRET_NAMES: [&str; 4] = ["KEY", "TOKEN", "SECRET", "PASSWORD"];
/// A shorter value is not redacted: it would hide common words, and guessing it is easy anyway.
const MIN_SECRET: usize = 8;

/// The rules for one agent, fixed when it starts.
#[derive(Debug)]
pub(crate) struct Redactor {
    home: Option<String>,
    /// Longest first, so a secret that holds another goes whole.
    secrets: Vec<String>,
}

impl Redactor {
    /// `home` becomes `~`. The values of `env`'s variables whose names hold `KEY`, `TOKEN`,
    /// `SECRET` or `PASSWORD`, at least 8 bytes long, become `[redacted]`.
    pub(crate) fn new(home: Option<&Path>, env: &BTreeMap<OsString, OsString>) -> Redactor {
        let home = home
            .and_then(Path::to_str)
            .filter(|home| !home.is_empty())
            .map(str::to_owned);
        let mut secrets: Vec<String> = env
            .iter()
            .filter(|(name, value)| is_secret_name(name) && value.len() >= MIN_SECRET)
            .map(|(_, value)| value.to_string_lossy().into_owned())
            .collect();
        secrets.sort_unstable_by_key(|secret| Reverse(secret.len()));
        secrets.dedup();
        Redactor { home, secrets }
    }

    /// `text` with every rule applied: secret values first, then the home folder, then `sk-`
    /// words and the word after `Bearer ` (in any case: `authorization: bearer ..` too).
    pub(crate) fn redact(&self, text: &str) -> String {
        let mut text = text.to_owned();
        for secret in &self.secrets {
            if text.contains(secret.as_str()) {
                text = text.replace(secret.as_str(), REDACTED);
            }
        }
        if let Some(home) = &self.home
            && text.contains(home.as_str())
        {
            text = text.replace(home.as_str(), "~");
        }
        let text = replace_runs(&text, "sk-", Marker::Starts, is_key_char);
        replace_runs(&text, "bearer ", Marker::Precedes, is_token68_char)
    }
}

fn is_secret_name(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_uppercase();
    SECRET_NAMES.iter().any(|secret| name.contains(secret))
}

/// Whether a marker is part of what [`replace_runs`] hides.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Marker {
    /// The marker starts a word, and the word goes (`sk-`). A marker inside a word is left.
    Starts,
    /// The marker, matched in any ASCII case, stays, and the run after it goes (`Bearer `).
    Precedes,
}

/// `text` with each `marker` and the run of `run` chars after it replaced, as `how` says.
/// A `Precedes` marker is matched in any ASCII case, so give it in lower case.
fn replace_runs(text: &str, marker: &str, how: Marker, run: fn(char) -> bool) -> String {
    // Lowering ASCII keeps every byte where it was, so offsets found in `haystack` hold in `text`.
    let haystack = match how {
        Marker::Starts => Cow::Borrowed(text),
        Marker::Precedes => Cow::Owned(text.to_ascii_lowercase()),
    };
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut from = 0;
    while let Some(found) = haystack.get(from..).and_then(|rest| rest.find(marker)) {
        let found = from + found;
        let after = found + marker.len();
        let end = text
            .get(after..)
            .and_then(|rest| rest.find(|c: char| !run(c)))
            .map_or(text.len(), |len| after + len);
        let starts_word = text
            .get(..found)
            .and_then(|before| before.chars().next_back())
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
        let hidden = match how {
            Marker::Starts => starts_word.then_some(found),
            Marker::Precedes => (end > after).then_some(after),
        };
        if let Some(start) = hidden {
            out.push_str(text.get(copied..start).unwrap_or_default());
            out.push_str(REDACTED);
            copied = end;
        }
        from = end.max(after);
    }
    out.push_str(text.get(copied..).unwrap_or_default());
    out
}

/// A char of an API key: letters, digits, `_` and `-`.
fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// A char of an HTTP bearer token (RFC 6750's `b64token`).
fn is_token68_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '+' | '/' | '=')
}
