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

/// `text` made safe for the log: the user name and password are dropped from every URL in it
/// (`scheme://user:pass@host` becomes `scheme://host`), the data folder becomes `<data>`, and
/// any other absolute path (`/…`, `C:\…`, `\\server\…`) becomes `<path>`.
pub(crate) fn redact(text: &str, data_dir: &Path) -> String {
    let text = strip_userinfo(text);
    let data = data_dir.to_string_lossy();
    let text = if data.is_empty() {
        text
    } else {
        text.replace(data.as_ref(), "<data>")
    };
    let mut out = String::with_capacity(text.len());
    let mut token = String::new();
    for ch in text.chars() {
        if is_separator(ch) {
            out.push_str(&redact_token(&token));
            token.clear();
            out.push(ch);
        } else {
            token.push(ch);
        }
    }
    out.push_str(&redact_token(&token));
    out
}

/// A URL with its user name and password dropped, for a line naming a host or a mirror.
pub(crate) fn redact_url(url: &str) -> String {
    strip_userinfo(url)
}

/// Characters that end a path or URL token.
fn is_separator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | '(' | ')' | ',' | ';')
}

/// `<path>` for an absolute path outside the data folder, else the token as it is.
fn redact_token(token: &str) -> String {
    if token.starts_with("<data>") {
        return token.to_owned();
    }
    let bytes = token.as_bytes();
    let unix = bytes.first() == Some(&b'/');
    let unc = token.starts_with("\\\\");
    let drive = matches!(bytes, [letter, b':', b'\\' | b'/', ..] if letter.is_ascii_alphabetic());
    if unix || unc || drive {
        "<path>".to_owned()
    } else {
        token.to_owned()
    }
}

/// Drops `user[:pass]@` from the authority of every `scheme://` in `text`.
fn strip_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end = tail
            .find(|ch: char| matches!(ch, '/' | '?' | '#') || is_separator(ch))
            .unwrap_or(tail.len());
        let (authority, after) = tail.split_at(end);
        match authority.rfind('@') {
            Some(userinfo_end) => out.push_str(&authority[userinfo_end + 1..]),
            None => out.push_str(authority),
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests;
