//! agent-browser's output on its way out of the guard, and the guard's own notes, with the
//! relay's URL and secret and the proxy's password taken out.
//!
//! agent-browser holds the relay's URL (`AGENT_BROWSER_CDP`, `devtools.rs`), and an error or a
//! reply of its that prints the URL would otherwise put the secret in the agent's logs, which a
//! command in Codex's sandbox may read. On Windows it holds the session key too
//! (`AGENT_BROWSER_SESSION_KEY`, `env.rs`), which keeps every process without it off its
//! daemon's pipe. The proxy's password never reaches it, but Chrome sends
//! it in each request's `Proxy-Authorization` header, which CDP can show. So [`MARK`] takes
//! the place of:
//! - the whole URL, its letters in any case;
//! - the secret and the session key (64 hex digits each) in any case, wherever they appear;
//! - a run of [`MIN_PREFIX`] or more characters that is the start of either, in any case: a
//!   message cut short;
//! - the proxy's password, as the URL's secret ([`Secrets::literal`]), and its base64
//!   `Proxy-Authorization` value, whole or a start of it that runs [`MIN_PREFIX`] characters
//!   past the user name's part ([`Secrets::start_after`], `proxy_auth::hide`): that part
//!   starts every Basic value for the same user, so a shorter start is no secret, and another
//!   Basic value is left as it is.
//!
//! Two streams carry it. agent-browser's stderr is a pipe, and [`forward`] copies it to the
//! guard's stderr through a [`Redactor`]: reads may split a match anywhere, so it holds back the
//! bytes that may still begin one, and gives them out once they cannot (a line end always
//! decides). Its stdout, the MCP stream, goes out whole line by whole line (`pumps.rs`), each
//! through [`line`]. Both look for every secret [`remember`]ed in this guard: each URL
//! agent-browser was given and the password's two forms. A line with no match goes out byte for
//! byte as it came; in one with a match, the mark (no quote, no backslash, no control character)
//! stands in a JSON string where the hex stood, so the line stays the same valid JSON. The
//! guard's own notes and failures go through [`redacted`] too, since some quote Chrome's
//! stderr or its CDP errors.
//!
//! Each stderr copy runs on a thread of its own ([`start`]); the guard waits a little for them on
//! its way out ([`drain`]), once agent-browser and its daemon are ended, so their last lines are
//! not lost with the guard.

use std::borrow::Cow;
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

/// Every relay URL handed to an agent-browser in this guard, and the proxy's password's forms.
static KNOWN: Mutex<Vec<Secrets>> = Mutex::new(Vec::new());

/// What stands in for a URL, a secret or the password.
pub(crate) const MARK: &[u8] = b"[secret redacted]";

/// The shortest start of the secret taken out on its own: 64 bits, which no other text matches
/// by chance.
pub(crate) const MIN_PREFIX: usize = 16;

/// One secret taken out of the text: its bytes, and the shortest start of them taken out on its
/// own.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    bytes: Vec<u8>,
    min_start: usize,
}

/// The relay's URL and the secrets taken out with it: its secret (the URL's last path segment)
/// and, on Windows, the session key; or one literal as both (the proxy's password). The first
/// entry is the URL's secret (or the literal), and [`Secrets::start_after`] changes only it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Secrets {
    url: Vec<u8>,
    entries: Vec<Entry>,
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
    /// The URL in agent-browser's environment `env`, its secret and, on Windows, the session key;
    /// none when it has no URL (a one-shot agent-browser, which gets no key either:
    /// `env::agent_env`).
    pub(crate) fn of(env: &BTreeMap<String, OsString>) -> Option<Secrets> {
        let url = env.get(env::CDP).and_then(|url| url.to_str())?;
        let secret = url.rsplit('/').next().unwrap_or_default();
        let key = env
            .get(env::SESSION_KEY)
            .and_then(|key| key.to_str())
            .filter(|key| !key.is_empty());
        (!url.is_empty() && !secret.is_empty()).then(|| Secrets {
            url: url.as_bytes().to_vec(),
            entries: [Some(secret), key]
                .into_iter()
                .flatten()
                .map(|hex| Entry {
                    bytes: hex.as_bytes().to_vec(),
                    min_start: MIN_PREFIX,
                })
                .collect(),
        })
    }

    /// `bytes` as both the URL and the secret: taken out whole, or a start of it of
    /// [`MIN_PREFIX`] or more characters.
    pub(crate) fn literal(bytes: &[u8]) -> Secrets {
        Secrets {
            url: bytes.to_vec(),
            entries: vec![Entry {
                bytes: bytes.to_vec(),
                min_start: MIN_PREFIX,
            }],
        }
    }

    /// The same, but a start of the secret (the first entry) is taken out only when it runs
    /// [`MIN_PREFIX`] characters past its first `fixed`, which are no secret. Other entries,
    /// the session key, keep [`MIN_PREFIX`].
    pub(crate) fn start_after(mut self, fixed: usize) -> Secrets {
        if let Some(first) = self.entries.first_mut() {
            first.min_start = fixed.saturating_add(MIN_PREFIX);
        }
        self
    }

    /// Whether `text` starts with the URL, one of the secrets or a long enough start of one.
    /// Undecided only while `text` may still grow (`more`).
    fn at_start(&self, text: &[u8], more: bool) -> Found {
        if self.url.is_empty() {
            return Found::No;
        }
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
        let (mut undecided, mut longest) = (false, None::<usize>);
        for entry in &self.entries {
            if entry.bytes.is_empty() {
                continue;
            }
            let same = text
                .iter()
                .zip(&entry.bytes)
                .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                .count();
            if same == entry.bytes.len() {
                return Found::Hit(same);
            }
            undecided |= more && same == text.len();
            if same >= entry.min_start {
                longest = longest.max(Some(same));
            }
        }
        if undecided {
            Found::Undecided
        } else if let Some(len) = longest {
            Found::Hit(len)
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

/// Notes a URL agent-browser was given, or a form of the proxy's password, for [`line`] and
/// every [`Redactor`].
pub(crate) fn remember(secrets: &Secrets) {
    let mut known = lock(&KNOWN);
    if !known.contains(secrets) {
        known.push(secrets.clone());
    }
}

/// A whole line of agent-browser's stdout redacted with every secret [`remember`]ed; none when
/// nothing in it matched, so it goes out as it is.
pub(crate) fn line(line: &[u8]) -> Option<Vec<u8>> {
    scan(line, &lock(&KNOWN), false).0
}

/// One of the guard's own notes or failures, redacted as [`line`] does.
pub(crate) fn redacted(text: &str) -> Cow<'_, str> {
    match line(text.as_bytes()) {
        Some(changed) => Cow::Owned(String::from_utf8_lossy(&changed).into_owned()),
        None => Cow::Borrowed(text),
    }
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

/// Copies `from` to the guard's stderr until it ends, redacting `secrets` and every secret
/// [`remember`]ed. A failed write to the guard's stderr drops the bytes and keeps reading, so
/// agent-browser never blocks on it.
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

/// A streaming search and replace of its own secrets and every one [`remember`]ed.
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
        let (out, decided) = scan(&self.held, &self.all(), true);
        let rest = self.held.split_off(decided);
        let plain = std::mem::replace(&mut self.held, rest);
        out.unwrap_or(plain)
    }

    /// What is left once the input has ended.
    pub(crate) fn finish(self) -> Vec<u8> {
        let (out, _) = scan(&self.held, &self.all(), false);
        out.unwrap_or(self.held)
    }

    /// Its own secrets, then every one [`remember`]ed now.
    fn all(&self) -> Vec<Secrets> {
        let mut all = vec![self.secrets.clone()];
        all.extend(lock(&KNOWN).iter().cloned());
        all
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;

    use super::{MARK, Redactor, Secrets, line, remember};
    use crate::browser_guard::devtools::base64;
    use crate::browser_guard::proxy_auth::{self, Password};

    const SECRET: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    /// An obviously fake proxy password.
    const PASSWORD: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";

    /// Remembers the fake password's two forms, as the guard's start does, and returns its
    /// `Proxy-Authorization` value.
    fn hide_password() -> String {
        proxy_auth::hide(&Password::parse(PASSWORD).unwrap());
        base64(format!("clauseo:{PASSWORD}").as_bytes())
    }

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

    #[test]
    fn the_password_is_hidden_on_stdout_on_stderr_and_in_the_guards_notes() {
        hide_password();
        let mark = mark();
        let text = format!(
            "proxy said {PASSWORD} and {}",
            PASSWORD.to_ascii_uppercase()
        );
        let expected = format!("proxy said {mark} and {mark}");
        let out = line(text.as_bytes()).expect("the line held the password");
        assert_eq!(String::from_utf8(out).unwrap(), expected);
        assert_eq!(
            redacted(&format!("{text}\n"), 4096),
            format!("{expected}\n")
        );
        assert_eq!(super::redacted(&text), expected);
    }

    #[test]
    fn the_proxy_authorization_value_is_hidden_whole_however_the_reads_split() {
        let basic = hide_password();
        let mark = mark();
        let header = format!("Proxy-Authorization: Basic {basic}\nnext\n");
        for step in [1, 5, 16, 95, 96, 97, 4096] {
            assert_eq!(
                redacted(&header, step),
                format!("Proxy-Authorization: Basic {mark}\nnext\n"),
                "reads of {step} bytes"
            );
        }
        let reply =
            serde_json::json!({"headers": {"Proxy-Authorization": format!("Basic {basic}")}})
                .to_string();
        let out = String::from_utf8(line(reply.as_bytes()).unwrap()).unwrap();
        assert!(out.contains(&format!("Basic {mark}")) && !out.contains(&basic));
    }

    /// Every Basic value for the user `clauseo` starts the same 12 characters, so a start of
    /// the credential is taken out only past them: another value sharing its first 16 (and so
    /// the password's first 4 hex digits) is left as it is, and a cut-short real one is not.
    #[test]
    fn another_basic_value_with_the_same_start_is_not_hidden() {
        let basic = hide_password();
        let mark = mark();
        let other = base64(format!("clauseo:feed{}", &SECRET[..60]).as_bytes());
        assert_eq!(other[..16], basic[..16]);
        assert_ne!(other[..17], basic[..17]);
        let header = format!("Proxy-Authorization: Basic {other}\n");
        assert_eq!(line(header.as_bytes()), None);
        for step in [1, 16, 17, 4096] {
            assert_eq!(redacted(&header, step), header, "reads of {step} bytes");
        }
        // 27 characters: the user's 12 and 15 more, too short to tell.
        let short = format!("Basic {}...", &basic[..27]);
        assert_eq!(line(short.as_bytes()), None);
        let cut = format!("Basic {}...", &basic[..28]);
        assert_eq!(
            String::from_utf8(line(cut.as_bytes()).unwrap()).unwrap(),
            format!("Basic {mark}...")
        );
    }

    #[test]
    fn the_password_split_across_reads_is_still_hidden() {
        hide_password();
        let mark = mark();
        let input = format!("a {PASSWORD} b\nc {}\u{2026}\n", &PASSWORD[..20]);
        for step in [1, 2, 7, 16, 63, 64, 65] {
            let out = redacted(&input, step);
            assert_eq!(
                out,
                format!("a {mark} b\nc {mark}\u{2026}\n"),
                "reads of {step} bytes"
            );
            assert!(!out.contains(&PASSWORD[..16]), "reads of {step} bytes");
        }
    }

    #[test]
    fn a_literal_is_taken_out_with_a_long_enough_start() {
        let hex = Secrets::literal(SECRET.as_bytes());
        assert_eq!(
            super::scan(&SECRET.as_bytes()[..16], std::slice::from_ref(&hex), false).1,
            16
        );
        let found = super::scan(&SECRET.as_bytes()[..16], &[hex], false).0;
        assert_eq!(found, Some(MARK.to_vec()));
        let later = Secrets::literal(SECRET.as_bytes()).start_after(40);
        assert_eq!(
            super::scan(
                &SECRET.as_bytes()[..55],
                std::slice::from_ref(&later),
                false
            )
            .0,
            None
        );
        let found = super::scan(
            &SECRET.as_bytes()[..56],
            std::slice::from_ref(&later),
            false,
        )
        .0;
        assert_eq!(found, Some(MARK.to_vec()));
        assert_eq!(
            super::scan(SECRET.as_bytes(), &[later], false).0,
            Some(MARK.to_vec())
        );
        // An empty literal matches nothing.
        assert_eq!(super::scan(b"abc", &[Secrets::literal(b"")], false).0, None);
    }

    /// The session key (Windows) is taken out as the secret is, from stderr and from stdout.
    #[test]
    fn the_session_key_is_taken_out_wherever_it_appears() {
        const KEY: &str = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";
        let env = BTreeMap::from([
            ("AGENT_BROWSER_CDP".to_owned(), OsString::from(url())),
            ("AGENT_BROWSER_SESSION_KEY".to_owned(), OsString::from(KEY)),
        ]);
        let with_key = Secrets::of(&env).unwrap();
        let mark = mark();
        let input = format!(
            "key {KEY}\nupper {}\ncut {}\nsecret {SECRET}\n",
            KEY.to_ascii_uppercase(),
            &KEY[..20]
        );
        let expected = format!("key {mark}\nupper {mark}\ncut {mark}\nsecret {mark}\n");
        for step in [1, 5, 64, 4096] {
            let mut redactor = Redactor::new(with_key.clone());
            let mut out = Vec::new();
            for chunk in input.as_bytes().chunks(step) {
                out.extend(redactor.push(chunk));
            }
            out.extend(redactor.finish());
            assert_eq!(
                String::from_utf8(out).unwrap(),
                expected,
                "reads of {step} bytes"
            );
        }
        remember(&with_key);
        let reply = format!("{{\"text\":\"{KEY}\"}}");
        let out = line(reply.as_bytes()).expect("the line held the key");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("{{\"text\":\"{mark}\"}}")
        );
    }

    /// The session key (Windows) is its own entry: its start is taken out at [`MIN_PREFIX`]
    /// characters, and a shorter start is no secret.
    #[test]
    fn a_session_key_start_is_taken_out_at_min_prefix() {
        const KEY: &str = "00ffeeddccbbaa998877665544332211ffeeddccbbaa998877665544332211";
        let env = BTreeMap::from([
            ("AGENT_BROWSER_CDP".to_owned(), OsString::from(url())),
            ("AGENT_BROWSER_SESSION_KEY".to_owned(), OsString::from(KEY)),
        ]);
        let with_key = Secrets::of(&env).unwrap();
        let mark = mark();
        let input = format!("cut {}\nshort {} kept\n", &KEY[..16], &KEY[..15]);
        let expected = format!("cut {mark}\nshort {} kept\n", &KEY[..15]);
        for step in [1, 3, 16, 4096] {
            let mut redactor = Redactor::new(with_key.clone());
            let mut out = Vec::new();
            for chunk in input.as_bytes().chunks(step) {
                out.extend(redactor.push(chunk));
            }
            out.extend(redactor.finish());
            assert_eq!(
                String::from_utf8(out).unwrap(),
                expected,
                "reads of {step} bytes"
            );
        }
    }
}
