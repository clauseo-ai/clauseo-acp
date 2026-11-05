//! Reading Codex's login output: the device URL and code of `login --device-auth`, and the two
//! refusals that have their own code (the device-code setting, the busy port). Pure functions.
//!
//! Parsing is strict (the brief, "Codex's commands": the URL `https://auth.openai.com/codex/device`
//! and the code `XXXX-XXXX`, pinned at 0.162.1):
//! - terminal escapes (Codex colours both) are dropped first, then each whitespace-separated
//!   token is looked at with surrounding punctuation trimmed;
//! - the URL is exactly [`DEVICE_URL`], and it must also pass the shim's address check against
//!   the snapshot's Codex sign-in hosts (https, the host exactly one of them, no user name, no
//!   IP literal);
//! - the code is exactly four, a `-`, and four, each an ASCII capital or digit, and it counts
//!   only after the URL was read;
//! - the first URL and code win; nothing later replaces them.

use std::time::Duration;

use crate::signin::shim::sign_in_address;

/// The device page Codex's `login --device-auth` prints.
pub(crate) const DEVICE_URL: &str = "https://auth.openai.com/codex/device";

/// How long a device code works: 15 minutes from when Codex prints it.
pub(crate) const CODE_LIFETIME: Duration = Duration::from_mins(15);

/// The device URL and code a run printed, read line by line.
#[derive(Debug, Default)]
pub(crate) struct DeviceLines {
    url: Option<String>,
    found: bool,
}

impl DeviceLines {
    /// Reads one output line. Gives the URL and code once, on the line that completes them.
    pub(crate) fn feed(&mut self, line: &str, hosts: &[String]) -> Option<(String, String)> {
        if self.found {
            return None;
        }
        let line = strip_escapes(line);
        for token in line.split_whitespace() {
            let token = token.trim_matches(|c: char| {
                matches!(
                    c,
                    '.' | ',' | ';' | ':' | '(' | ')' | '<' | '>' | '"' | '\''
                )
            });
            match &self.url {
                None => {
                    if token == DEVICE_URL && sign_in_address(token, hosts) {
                        self.url = Some(token.to_owned());
                    }
                }
                Some(url) => {
                    if is_code(token) {
                        self.found = true;
                        return Some((url.clone(), token.to_owned()));
                    }
                }
            }
        }
        None
    }
}

/// Whether `token` is a device code: `XXXX-XXXX`, each `X` an ASCII capital or digit.
pub(crate) fn is_code(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == 9
        && bytes.iter().enumerate().all(|(at, byte)| {
            if at == 4 {
                *byte == b'-'
            } else {
                byte.is_ascii_uppercase() || byte.is_ascii_digit()
            }
        })
}

/// Whether a line is Codex's refusal when device-code sign-in is off for the account, in any
/// case: it says `Enable device code` and `Security Settings` (Codex's message, from Rohan's
/// own sign-in on 2026-11-02, is in the brief).
pub(crate) fn is_device_code_off(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("enable device code") && line.contains("security settings")
}

/// Whether a line says the callback port is taken (Codex lost the race for 1455 after the
/// probe): it names `1455` and says `in use`, in any case.
pub(crate) fn is_port_busy(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("1455") && line.contains("in use")
}

/// `line` without terminal escapes (`ESC [ … final`, or `ESC` and one character) and without
/// any other control character.
fn strip_escapes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
        } else if c.is_control() && c != '\t' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}
