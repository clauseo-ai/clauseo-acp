//! The log a failure carries for the window's "Details": English lines, with no user name or
//! password from any URL and no path outside the data folder.

use std::fmt;
use std::path::Path;

use clauseo_protocol::AgentId;

/// The lines of one install's log, in order. The first two are always `Clauseo download log`
/// and `agent: {id}`; the rest are added as the install goes: `received: {n} of {total} MB`,
/// `host: {host}`, `status: {code}`, `result: {kind} after {n} tries`, and short details passed
/// through [`redact`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallLog {
    lines: Vec<String>,
}

impl InstallLog {
    /// A log with its two heading lines.
    pub fn new(agent: &AgentId) -> InstallLog {
        InstallLog {
            lines: vec!["Clauseo download log".to_owned(), format!("agent: {agent}")],
        }
    }

    /// Adds `line`. A line that holds text from outside Clauseo (a URL, an error, npm's
    /// output) must have passed through [`redact`] first.
    pub fn push(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    /// The lines, in order.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }
}

impl fmt::Display for InstallLog {
    /// The lines joined by `\n`, as the window's "Details" shows them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (n, line) in self.lines.iter().enumerate() {
            if n > 0 {
                f.write_str("\n")?;
            }
            f.write_str(line)?;
        }
        Ok(())
    }
}

/// `text` made safe for the log, in three passes:
/// - the data folder becomes `<data>`;
/// - each URL loses its user name and password (`scheme://user:pass@host` becomes
///   `scheme://host`, the password may hold `/`, `;` or `@`) and its query (`?<query>`), and a
///   bare `user:pass@host` loses `user:pass@`;
/// - any other absolute path (`/…`, `C:\…`, `\\server\…`) becomes `<path>`: after a quote, up
///   to the closing quote; otherwise up to white space, carried on across a space while the next
///   word holds a `/` or `\` (`C:\Users\Jane Doe\ca.pem`).
///
/// It is a second line of defence: details never echo a setting's value (the proxy, the mirror,
/// a CA file's path) in the first place.
pub(crate) fn redact(text: &str, data_dir: &Path) -> String {
    let data = data_dir.to_string_lossy();
    let text = if data.is_empty() {
        text.to_owned()
    } else {
        text.replace(data.as_ref(), "<data>")
    };
    hide_paths(&strip_userinfo(&text))
}

/// A URL with its user name, password and query dropped, for a line naming a host or a URL.
pub(crate) fn redact_url(url: &str) -> String {
    strip_userinfo(url)
}

/// A quote, which may open or close a path.
fn is_quote(ch: char) -> bool {
    matches!(ch, '"' | '\'' | '`')
}

/// Characters a path may start after.
fn is_separator(ch: char) -> bool {
    ch.is_whitespace() || is_quote(ch) || matches!(ch, '(' | ')' | ',' | ';' | '=')
}

/// Whether `text` starts with an absolute path: `/`, `\\` or a drive (`C:\` or `C:/`).
fn starts_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.first() == Some(&b'/')
        || text.starts_with("\\\\")
        || matches!(bytes, [letter, b':', b'\\' | b'/', ..] if letter.is_ascii_alphabetic())
}

/// Each white-space or quote separated token of `text` through [`clean_token`].
fn strip_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut token = String::new();
    for ch in text.chars() {
        if ch.is_whitespace() || is_quote(ch) {
            out.push_str(&clean_token(&token));
            token.clear();
            out.push(ch);
        } else {
            token.push(ch);
        }
    }
    out.push_str(&clean_token(&token));
    out
}

/// A token with any URL's user info and query dropped, or a bare `user:pass@` dropped.
fn clean_token(token: &str) -> String {
    if let Some(at) = token.find("://") {
        let (head, rest) = token.split_at(at + 3);
        let rest = userinfo_end(rest)
            .and_then(|end| rest.get(end + 1..))
            .unwrap_or(rest);
        return match rest.find(['?', '#']) {
            Some(query) => format!("{head}{}?<query>", rest.get(..query).unwrap_or_default()),
            None => format!("{head}{rest}"),
        };
    }
    if starts_path(token) || token.starts_with("<data>") {
        return token.to_owned();
    }
    match token.rfind('@') {
        Some(at) if token.get(..at).is_some_and(|user| user.contains(':')) => {
            token.get(at + 1..).unwrap_or_default().to_owned()
        }
        _ => token.to_owned(),
    }
}

/// Where the user info after `scheme://` ends: the last `@` that does not follow a `/` (an
/// npm scope, `/@scope/`, is in the path). A path holding `@` loses more than it must, which
/// errs the safe way.
fn userinfo_end(rest: &str) -> Option<usize> {
    rest.char_indices()
        .filter(|&(at, ch)| {
            ch == '@' && !rest.get(..at).is_some_and(|before| before.ends_with('/'))
        })
        .map(|(at, _)| at)
        .next_back()
}

/// Every absolute path in `text` replaced by `<path>` (see [`redact`]).
fn hide_paths(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut prev: Option<char> = None;
    while let Some(ch) = rest.chars().next() {
        if prev.is_none_or(is_separator) && starts_path(rest) {
            let end = path_end(rest, prev.filter(|&before| is_quote(before)));
            out.push_str("<path>");
            rest = rest.get(end..).unwrap_or_default();
            prev = Some('>');
            continue;
        }
        out.push(ch);
        prev = Some(ch);
        rest = rest.get(ch.len_utf8()..).unwrap_or_default();
    }
    out
}

/// The length of the path `text` starts with: up to `quote` when it opened one, else up to
/// white space, carried on across spaces while the next word holds a separator.
fn path_end(text: &str, quote: Option<char>) -> usize {
    if let Some(quote) = quote {
        return text.find(quote).unwrap_or(text.len());
    }
    let word_end = |from: usize| {
        text.get(from..)
            .and_then(|tail| tail.find(|ch: char| ch.is_whitespace() || is_quote(ch)))
            .map_or(text.len(), |len| from + len)
    };
    let mut end = word_end(0);
    loop {
        let after = text.get(end..).unwrap_or_default();
        let next = after.trim_start_matches(' ');
        let start = text.len() - next.len();
        if start == end || next.is_empty() {
            return end;
        }
        let next_end = word_end(start);
        let word = text.get(start..next_end).unwrap_or_default();
        if word.contains(['/', '\\']) && !starts_path(word) {
            end = next_end;
        } else {
            return end;
        }
    }
}

#[cfg(test)]
mod tests;
