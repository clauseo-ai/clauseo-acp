//! agent-browser's stderr on its way to the guard's, with the relay's URL and secret taken out.
//!
//! agent-browser holds the relay's URL (`AGENT_BROWSER_CDP`, `devtools.rs`), and an error of its
//! that prints the URL would otherwise put the secret in the agent's logs, which a command in
//! Codex's sandbox may read. So agent-browser's stderr is a pipe, and [`forward`] copies it to the
//! guard's stderr through a [`Redactor`], which writes [`MARK`] in place of the whole URL and of
//! the secret wherever it appears alone. Reads may split the URL anywhere: the redactor holds back
//! the bytes that could begin it until they cannot, and at most a line's end later.
//!
//! Each copy runs on a thread of its own ([`start`]); the guard waits a little for them on its
//! way out ([`drain`]), once agent-browser and its daemon are ended, so their last lines are
//! not lost with the guard.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use super::super::{env, now, pause};
use super::lines::lock;

/// The copies still running or not yet waited for.
static COPIES: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

/// What stands in for the URL or the secret.
pub(crate) const MARK: &[u8] = b"[relay url redacted]";

/// The strings taken out of agent-browser's stderr for its environment `env`: the relay's URL
/// and its last path segment, the secret. None when it has no URL (a one-shot agent-browser).
pub(crate) fn secrets_of(env: &BTreeMap<String, OsString>) -> Vec<Vec<u8>> {
    let Some(url) = env.get(env::CDP).and_then(|url| url.to_str()) else {
        return Vec::new();
    };
    let secret = url.rsplit('/').next().unwrap_or_default();
    [url, secret]
        .into_iter()
        .filter(|text| !text.is_empty())
        .map(|text| text.as_bytes().to_vec())
        .collect()
}

/// Starts [`forward`] on a thread of its own. It ends when the last holder of the pipe's other
/// end (agent-browser or its daemon) exits.
pub(crate) fn start(from: impl Read + Send + 'static, secrets: Vec<Vec<u8>>) {
    let copy = std::thread::spawn(move || forward(from, secrets));
    lock(&COPIES).push(copy);
}

/// Waits up to `wait` for every copy to reach the end of its pipe.
pub(crate) fn drain(wait: Duration) {
    let deadline = now() + wait;
    loop {
        let mut copies = lock(&COPIES);
        copies.retain(|copy| !copy.is_finished());
        if copies.is_empty() || now() >= deadline {
            return;
        }
        drop(copies);
        pause(Duration::from_millis(10));
    }
}

/// Copies `from` to the guard's stderr until it ends, redacting `secrets`. A failed write to
/// the guard's stderr drops the bytes and keeps reading, so agent-browser never blocks on it.
pub(crate) fn forward(mut from: impl Read, secrets: Vec<Vec<u8>>) {
    let mut redactor = Redactor::new(secrets);
    let mut chunk = [0_u8; 4096];
    loop {
        let out = match from.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => redactor.push(chunk.get(..read).unwrap_or_default()),
        };
        drop(io::stderr().lock().write_all(&out));
    }
    drop(io::stderr().lock().write_all(&redactor.finish()));
}

/// A streaming search and replace of the secrets.
pub(crate) struct Redactor {
    /// Longest first, so the URL is replaced whole before its secret alone.
    secrets: Vec<Vec<u8>>,
    /// Bytes read and not yet given out: a tail that may be the start of a secret.
    held: Vec<u8>,
}

impl Redactor {
    pub(crate) fn new(mut secrets: Vec<Vec<u8>>) -> Redactor {
        secrets.retain(|secret| !secret.is_empty());
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        Redactor {
            secrets,
            held: Vec::new(),
        }
    }

    /// Takes `bytes` and returns what may be written now, redacted.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.held.extend_from_slice(bytes);
        for secret in &self.secrets {
            self.held = replace(&self.held, secret);
        }
        // No secret holds a line end, so none spans one: a whole line goes out at once.
        let keep = if self.held.ends_with(b"\n") {
            0
        } else {
            let longest = self.secrets.first().map_or(0, Vec::len);
            longest.saturating_sub(1).min(self.held.len())
        };
        let tail = self.held.split_off(self.held.len() - keep);
        std::mem::replace(&mut self.held, tail)
    }

    /// What is left once the input has ended.
    pub(crate) fn finish(self) -> Vec<u8> {
        self.held
    }
}

/// `text` with every `secret` in it replaced by [`MARK`].
fn replace(text: &[u8], secret: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest
        .windows(secret.len())
        .position(|window| window == secret)
    {
        out.extend_from_slice(rest.get(..at).unwrap_or_default());
        out.extend_from_slice(MARK);
        rest = rest.get(at + secret.len()..).unwrap_or_default();
    }
    out.extend_from_slice(rest);
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;

    use super::{MARK, Redactor, secrets_of};

    const SECRET: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn url() -> String {
        format!("ws://127.0.0.1:50123/devtools/browser/{SECRET}")
    }

    /// Everything the redactor gives out for `input` read `step` bytes at a time.
    fn redacted(input: &str, step: usize) -> String {
        let env = BTreeMap::from([("AGENT_BROWSER_CDP".to_owned(), OsString::from(url()))]);
        let mut redactor = Redactor::new(secrets_of(&env));
        let mut out = Vec::new();
        for chunk in input.as_bytes().chunks(step) {
            out.extend(redactor.push(chunk));
        }
        out.extend(redactor.finish());
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn the_url_and_the_secret_never_reach_the_guards_stderr_however_the_reads_split() {
        let mark = String::from_utf8(MARK.to_vec()).unwrap();
        let input = format!(
            "error: cannot connect to {} (refused)\nsecret {SECRET} alone\nno secret here\n\
             ends on a URL: {}",
            url(),
            url()
        );
        let expected = format!(
            "error: cannot connect to {mark} (refused)\nsecret {mark} alone\nno secret here\n\
             ends on a URL: {mark}"
        );
        for step in [1, 2, 7, 63, 64, 65, 4096] {
            let out = redacted(&input, step);
            assert_eq!(out, expected, "reads of {step} bytes");
            assert!(!out.contains(&SECRET[..16]), "reads of {step} bytes");
        }
    }

    #[test]
    fn a_line_goes_out_when_it_ends_and_text_without_a_url_is_unchanged() {
        let env = BTreeMap::from([("AGENT_BROWSER_CDP".to_owned(), OsString::from(url()))]);
        let mut redactor = Redactor::new(secrets_of(&env));
        assert_eq!(redactor.push(b"warn: slow\n"), b"warn: slow\n");
        // A one-shot agent-browser has no URL: nothing to take out.
        assert_eq!(secrets_of(&BTreeMap::new()), Vec::<Vec<u8>>::new());
        let mut plain = Redactor::new(Vec::new());
        assert_eq!(plain.push(b"partial"), b"partial");
    }
}
