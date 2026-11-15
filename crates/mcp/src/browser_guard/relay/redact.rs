//! agent-browser's output on its way out of the guard, with the relay's URL and secret taken out.
//!
//! agent-browser holds the relay's URL (`AGENT_BROWSER_CDP`, `devtools.rs`), and an error or a
//! reply of its that prints the URL would otherwise put the secret in the agent's logs, which a
//! command in Codex's sandbox may read. So [`MARK`] takes the place of:
//! - the whole URL, its letters in any case;
//! - the secret (64 hex digits) in any case, wherever it appears;
//! - a run of [`MIN_PREFIX`] or more characters that is the secret's start, in any case: a
//!   message cut short.
//!
//! Two streams carry it. agent-browser's stderr is a pipe, and [`forward`] copies it to the
//! guard's stderr through a [`Redactor`]: reads may split a match anywhere, so it holds back the
//! bytes that may still begin one, and gives them out once they cannot (a line end always
//! decides). Its stdout, the MCP stream, goes out whole line by whole line (`pumps.rs`), each
//! through [`line`], with every URL agent-browser was given in this guard ([`remember`]). A line
//! with no match goes out byte for byte as it came; in one with a match, the mark (no quote, no
//! backslash, no control character) stands in a JSON string where the hex stood, so the line
//! stays the same valid JSON.
//!
//! Each stderr copy runs on a thread of its own ([`start`]); the guard waits a little for them on
//! its way out ([`drain`]), once agent-browser and its daemon are ended, so their last lines are
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

/// Every relay URL handed to an agent-browser in this guard, for its stdout.
static KNOWN: Mutex<Vec<Secrets>> = Mutex::new(Vec::new());

/// What stands in for the URL or the secret.
pub(crate) const MARK: &[u8] = b"[relay url redacted]";

/// The shortest start of the secret taken out on its own: 64 bits, which no other text matches
/// by chance.
pub(crate) const MIN_PREFIX: usize = 16;

/// The relay's URL and its secret (the URL's last path segment).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Secrets {
    url: Vec<u8>,
    secret: Vec<u8>,
}

/// Whether a pattern is at the start of some text.
enum Found {
    /// Yes, this many bytes of the text.
    Hit(usize),
    No,
    /// The text ends before it can tell.
    Undecided,
}

impl Secrets {
    /// The URL in agent-browser's environment `env`; none when it has none (a one-shot
    /// agent-browser).
    pub(crate) fn of(env: &BTreeMap<String, OsString>) -> Option<Secrets> {
        let url = env.get(env::CDP).and_then(|url| url.to_str())?;
        let secret = url.rsplit('/').next().unwrap_or_default();
        (!url.is_empty() && !secret.is_empty()).then(|| Secrets {
            url: url.as_bytes().to_vec(),
            secret: secret.as_bytes().to_vec(),
        })
    }

    /// Whether `text` starts with the URL, the secret or a long enough start of the secret.
    /// Undecided only while `text` may still grow (`more`).
    fn at_start(&self, text: &[u8], more: bool) -> Found {
        if text.len() >= self.url.len() {
            if text
                .get(..self.url.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(&self.url))
            {
                return Found::Hit(self.url.len());
            }
        } else if more
            && self
                .url
                .get(..text.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(text))
        {
            return Found::Undecided;
        }
        let same = text
            .iter()
            .zip(&self.secret)
            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
            .count();
        if same == self.secret.len() {
            Found::Hit(same)
        } else if more && same == text.len() {
            Found::Undecided
        } else if same >= MIN_PREFIX {
            Found::Hit(same)
        } else {
            Found::No
        }
    }
}

/// `text` with every match of `all` replaced by [`MARK`], up to the first byte still undecided
/// (none when `more` is false), and how many bytes of `text` that covers. None for the text
/// when nothing matched: it goes out as it is.
fn scan(text: &[u8], all: &[Secrets], more: bool) -> (Option<Vec<u8>>, usize) {
    let mut out: Option<Vec<u8>> = None;
    let (mut at, mut copied) = (0, 0);
    'bytes: while at < text.len() {
        let rest = text.get(at..).unwrap_or_default();
        for secrets in all {
            match secrets.at_start(rest, more) {
                Found::No => {}
                Found::Undecided => break 'bytes,
                Found::Hit(len) => {
                    let out = out.get_or_insert_with(|| Vec::with_capacity(text.len()));
                    out.extend_from_slice(text.get(copied..at).unwrap_or_default());
                    out.extend_from_slice(MARK);
                    at += len;
                    copied = at;
                    continue 'bytes;
                }
            }
        }
        at += 1;
    }
    if let Some(out) = &mut out {
        out.extend_from_slice(text.get(copied..at).unwrap_or_default());
    }
    (out, at)
}

/// Notes the URL agent-browser was given, for [`line`].
pub(crate) fn remember(secrets: &Secrets) {
    let mut known = lock(&KNOWN);
    if !known.contains(secrets) {
        known.push(secrets.clone());
    }
}

/// A whole line of agent-browser's stdout redacted with every URL [`remember`]ed; none when
/// nothing in it matched, so it goes out as it is.
pub(crate) fn line(line: &[u8]) -> Option<Vec<u8>> {
    scan(line, &lock(&KNOWN), false).0
}

/// Starts [`forward`] on a thread of its own. It ends when the last holder of the pipe's other
/// end (agent-browser or its daemon) exits.
pub(crate) fn start(from: impl Read + Send + 'static, secrets: Secrets) {
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
fn forward(mut from: impl Read, secrets: Secrets) {
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
    secrets: Secrets,
    /// Bytes read and not yet given out: a tail that may still begin a match.
    held: Vec<u8>,
}

impl Redactor {
    pub(crate) fn new(secrets: Secrets) -> Redactor {
        Redactor {
            secrets,
            held: Vec::new(),
        }
    }

    /// Takes `bytes` and returns what may be written now, redacted.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.held.extend_from_slice(bytes);
        let (out, decided) = scan(&self.held, std::slice::from_ref(&self.secrets), true);
        let rest = self.held.split_off(decided);
        let plain = std::mem::replace(&mut self.held, rest);
        out.unwrap_or(plain)
    }

    /// What is left once the input has ended.
    pub(crate) fn finish(self) -> Vec<u8> {
        let (out, _) = scan(&self.held, std::slice::from_ref(&self.secrets), false);
        out.unwrap_or(self.held)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;

    use super::{MARK, Redactor, Secrets, line, remember};

    const SECRET: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn url() -> String {
        format!("ws://127.0.0.1:50123/devtools/browser/{SECRET}")
    }

    fn secrets() -> Secrets {
        let env = BTreeMap::from([("AGENT_BROWSER_CDP".to_owned(), OsString::from(url()))]);
        Secrets::of(&env).unwrap()
    }

    fn mark() -> String {
        String::from_utf8(MARK.to_vec()).unwrap()
    }

    /// Everything the redactor gives out for `input` read `step` bytes at a time.
    fn redacted(input: &str, step: usize) -> String {
        let mut redactor = Redactor::new(secrets());
        let mut out = Vec::new();
        for chunk in input.as_bytes().chunks(step) {
            out.extend(redactor.push(chunk));
        }
        out.extend(redactor.finish());
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn the_url_and_the_secret_never_reach_the_guards_stderr_however_the_reads_split() {
        let mark = mark();
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
    fn the_secret_in_any_case_and_its_cut_short_start_are_taken_out_too() {
        let mark = mark();
        let upper = SECRET.to_ascii_uppercase();
        let url_upper = url().to_ascii_uppercase();
        let cut = &SECRET[..16];
        let short = &SECRET[..15];
        let input = format!(
            "A {upper} B\nC {url_upper}\nD browser/{cut}\u{2026}\nE x{} y\nF {short} kept\n\
             G ab{SECRET}cd\n",
            &SECRET[..40]
        );
        let expected = format!(
            "A {mark} B\nC {mark}\nD browser/{mark}\u{2026}\nE x{mark} y\nF {short} kept\n\
             G ab{mark}cd\n"
        );
        for step in [1, 3, 16, 17, 4096] {
            assert_eq!(redacted(&input, step), expected, "reads of {step} bytes");
        }
    }

    #[test]
    fn a_line_goes_out_when_it_ends_and_text_without_a_url_is_unchanged() {
        let mut redactor = Redactor::new(secrets());
        assert_eq!(redactor.push(b"warn: slow\n"), b"warn: slow\n");
        // Hex that is not the secret's start goes out once it cannot be.
        assert_eq!(redactor.push(b"id 0012"), b"id 0012");
        // A one-shot agent-browser has no URL: nothing to take out.
        assert_eq!(Secrets::of(&BTreeMap::new()), None);
    }

    /// agent-browser's MCP replies: a line with the secret stays the same JSON with the mark in
    /// its place; a line without one, invalid UTF-8 included, is passed on as it came.
    #[test]
    fn stdout_lines_keep_their_json_and_their_bytes() {
        remember(&secrets());
        let reply = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": {"content": [{"type": "text", "text": format!(
                "connected to {} ({}...)", url(), &SECRET[..20].to_ascii_uppercase()
            )}]},
        })
        .to_string();
        let out = line(reply.as_bytes()).expect("the line held the secret");
        let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(
            parsed["result"]["content"][0]["text"],
            format!("connected to {} ({}...)", mark(), mark())
        );
        assert!(
            !String::from_utf8_lossy(&out)
                .to_ascii_lowercase()
                .contains(&SECRET[..16])
        );
        let plain: &[u8] =
            b"{\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{\"text\":\"00112233445566 \xff\"}}";
        assert_eq!(line(plain), None);
    }
}
