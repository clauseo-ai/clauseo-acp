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
/// The marker of a bearer token, matched in any case.
const BEARER: &str = "bearer ";
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
        replace_runs(&text, BEARER, Marker::Precedes, is_token68_char)
    }

    /// `text`, the start of a window that does not hold stderr's start, without what may be the
    /// end of a secret whose start was cut off, which [`redact`] would no longer know.
    ///
    /// At a line start (`mid_line` false) that is the longest end of a secret value that
    /// follows one of its `\n`s, as in a PEM key. Mid-line it is whichever leaves out more of the
    /// longest end of a secret value and [`without_token_end`] (the end of an `sk-` key or a
    /// bearer token), and then again the longest end of a value, for a value that began inside
    /// what that left out. A value may hold any char, so only its own end is left out for it.
    ///
    /// [`redact`]: Redactor::redact
    pub(crate) fn without_cut_secret<'t>(&self, text: &'t str, mid_line: bool) -> &'t str {
        if !mid_line {
            return self.without_value_end(text, true);
        }
        let rest = [self.without_value_end(text, false), without_token_end(text)]
            .into_iter()
            .min_by_key(|rest| rest.len())
            .unwrap_or(text);
        self.without_value_end(rest, false)
    }

    /// `text` without the longest end of a secret value it starts with: any end, or with
    /// `after_newline` only one that follows a `\n` of the value.
    fn without_value_end<'t>(&self, text: &'t str, after_newline: bool) -> &'t str {
        self.secrets
            .iter()
            .filter_map(|secret| without_end_of(secret, text, after_newline))
            .min_by_key(|rest| rest.len())
            .unwrap_or(text)
    }
}

/// `text` without the longest end of `secret` it starts with, if it starts with one; with
/// `after_newline`, only an end that follows a `\n` of `secret`.
fn without_end_of<'t>(secret: &str, text: &'t str, after_newline: bool) -> Option<&'t str> {
    secret
        .char_indices()
        .filter(|&(at, _)| !after_newline || secret[..at].ends_with('\n'))
        .find_map(|(at, _)| text.strip_prefix(&secret[at..]))
}

/// `text` without the run of bearer-token chars it starts with (a superset of key chars), the
/// end of an `sk-` key or a bearer token. When that run, in any case, is the end of the
/// `Bearer` marker (or empty) and a space follows, the marker was cut and the token after it
/// goes too. So ordinary text that starts with a space loses that space and the word after it.
fn without_token_end(text: &str) -> &str {
    let rest = text.trim_start_matches(is_token68_char);
    let run = &text[..text.len() - rest.len()];
    let marker = BEARER.trim_end_matches(' ');
    match rest.strip_prefix(' ') {
        Some(token) if marker.ends_with(&run.to_ascii_lowercase()) => {
            token.trim_start_matches(is_token68_char)
        }
        _ => rest,
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
