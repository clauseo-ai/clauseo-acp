//! What the canary test uses: a scratch folder whose path holds the canary, a limits reader
//! that launches nothing, an `Upload` that records every body and answers 200, and the search
//! for the canary in a body.

use std::fmt::Write as _;
use std::future::Future;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

use clauseo_counts::{Upload, UploadError, UploadPath};
use clauseo_limits::{ReadFailure, ReadLimits, ReadOutcome};
use clauseo_protocol::AgentKind;
use futures::future::{BoxFuture, Either};

/// How long one wait on the host may take in real time before the test fails naming it.
pub(crate) const WATCHDOG: Duration = Duration::from_secs(20);

/// Runs `fut` to completion; fails the test naming `what` when [`WATCHDOG`] passes first.
pub(crate) fn wait<T>(what: &str, fut: impl Future<Output = T>) -> T {
    futures::executor::block_on(async {
        let fut = std::pin::pin!(fut);
        match futures::future::select(fut, async_io::Timer::after(WATCHDOG)).await {
            Either::Left((value, _timer)) => value,
            Either::Right(_) => panic!("timed out after {WATCHDOG:?} waiting for {what}"),
        }
    })
}

/// A folder of the test's own under the temp folder, removed when dropped by the one that made
/// it.
pub(crate) struct Scratch {
    root: PathBuf,
    /// Whether this one made it, and so removes it.
    owned: bool,
}

impl Scratch {
    /// A fresh, empty `<temp>/<name>`.
    #[expect(
        clippy::disallowed_methods,
        reason = "test set-up writes its scratch folder; the desktop crate has no write module"
    )]
    pub(crate) fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(name);
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        Self { root, owned: true }
    }

    /// The folder `root`, made by another (the outer test's process); left in place when dropped.
    pub(crate) fn adopt(root: PathBuf) -> Self {
        Self { root, owned: false }
    }

    /// The folder.
    pub(crate) fn path(&self) -> &Path {
        &self.root
    }

    /// Makes the folder `rel` under the scratch folder, with its parents; answers its path.
    #[expect(
        clippy::disallowed_methods,
        reason = "test set-up writes its scratch folder; the desktop crate has no write module"
    )]
    pub(crate) fn dir(&self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    /// Writes `bytes` to `path` (its folder must exist).
    #[expect(
        clippy::disallowed_methods,
        reason = "test set-up writes its scratch folder; the desktop crate has no write module"
    )]
    pub(crate) fn write(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
    }
}

impl Drop for Scratch {
    #[expect(
        clippy::disallowed_methods,
        reason = "test clean-up removes its scratch folder; the desktop crate has no write module"
    )]
    fn drop(&mut self) {
        if self.owned {
            let _gone = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// A limits reader that fails every read at once and launches nothing (the default one would
/// start the real Claude reader for a `claude-acp` entry).
pub(crate) struct NoReader;

impl ReadLimits for NoReader {
    fn read(&self, _agent: AgentKind) -> Pin<Box<dyn Future<Output = ReadOutcome> + Send>> {
        let failed = ReadFailure::LaunchFailed("the canary test launches no limits reader".into());
        Box::pin(std::future::ready(ReadOutcome::Failed(failed)))
    }
}

/// One POST the mock upload saw.
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    pub(crate) path: UploadPath,
    pub(crate) body: Vec<u8>,
}

/// The mock `Upload`: records every body and answers 200.
#[derive(Default)]
pub(crate) struct RecordingUpload {
    seen: Mutex<Vec<Seen>>,
}

impl RecordingUpload {
    /// Every POST so far, in order.
    pub(crate) fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Upload for RecordingUpload {
    fn post(
        &self,
        path: UploadPath,
        gzip_body: Vec<u8>,
    ) -> BoxFuture<'_, Result<u16, UploadError>> {
        self.seen.lock().unwrap().push(Seen {
            path,
            body: gzip_body,
        });
        Box::pin(std::future::ready(Ok(200)))
    }
}

/// `body`, a gzip stream, unzipped.
pub(crate) fn gunzip(body: &[u8]) -> Vec<u8> {
    assert_eq!(body.get(..2), Some(&[0x1f, 0x8b][..]), "a gzip body");
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(body)
        .read_to_end(&mut json)
        .unwrap();
    json
}

/// Every form of `secret` a body could carry it in, each with a label: its UTF-8 bytes, its
/// JSON string form (`serde_json::to_string` without the quotes), its `\uXXXX` forms (lower
/// and upper case hex, as an ASCII-only JSON writer would put non-ASCII text), and its
/// percent-encoded forms (lower and upper case hex, every byte outside the unreserved set).
pub(crate) fn forms(secret: &str) -> Vec<(String, Vec<u8>)> {
    let json = serde_json::to_string(secret).unwrap();
    let json = json
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap()
        .to_owned();
    let escaped = |upper: bool| -> String {
        let mut out = String::new();
        for c in secret.chars() {
            if c.is_ascii() {
                out.push(c);
            } else {
                let mut units = [0_u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    if upper {
                        write!(out, "\\u{unit:04X}").unwrap();
                    } else {
                        write!(out, "\\u{unit:04x}").unwrap();
                    }
                }
            }
        }
        out
    };
    let percent = |upper: bool| -> String {
        let mut out = String::new();
        for byte in secret.bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                out.push(char::from(byte));
            } else if upper {
                write!(out, "%{byte:02X}").unwrap();
            } else {
                write!(out, "%{byte:02x}").unwrap();
            }
        }
        out
    };
    let mut all = vec![
        ("utf-8".to_owned(), secret.to_owned()),
        ("json".to_owned(), json),
        ("\\u lower".to_owned(), escaped(false)),
        ("\\u upper".to_owned(), escaped(true)),
        ("percent lower".to_owned(), percent(false)),
        ("percent upper".to_owned(), percent(true)),
    ];
    // Keep one of each distinct form (an ASCII secret has the same text in most of them).
    let mut seen = Vec::new();
    all.retain(|(_, text)| {
        let fresh = !seen.contains(text);
        seen.push(text.clone());
        fresh
    });
    all.into_iter()
        .map(|(label, text)| (label, text.into_bytes()))
        .collect()
}

/// Whether `needle` occurs in `haystack`.
pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Every string in `value`, keys included, decoded from its JSON escapes.
pub(crate) fn strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => out.push(text.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                out.push(key.clone());
                strings(item, out);
            }
        }
        _ => {}
    }
}

/// Where in `seen` any form of any of `secrets` occurs: in the raw gzip bytes, the unzipped
/// bytes, or a decoded JSON string of the unzipped body. One line per hit; empty when none.
pub(crate) fn leaks(seen: &Seen, secrets: &[&str]) -> Vec<String> {
    let json = gunzip(&seen.body);
    let mut decoded = Vec::new();
    let value: serde_json::Value = serde_json::from_slice(&json).unwrap_or_else(|err| {
        panic!(
            "a {:?} body is not JSON ({err}): {}",
            seen.path,
            String::from_utf8_lossy(&json)
        )
    });
    strings(&value, &mut decoded);
    let mut hits = Vec::new();
    for secret in secrets {
        for (form, needle) in forms(secret) {
            let places = [
                ("raw gzip bytes", contains(&seen.body, &needle)),
                ("gunzipped bytes", contains(&json, &needle)),
                (
                    "a decoded JSON string",
                    decoded
                        .iter()
                        .any(|text| contains(text.as_bytes(), &needle)),
                ),
            ];
            for (place, hit) in places {
                if hit {
                    hits.push(format!(
                        "{secret:?} ({form}) in the {:?} body's {place}",
                        seen.path
                    ));
                }
            }
        }
    }
    hits
}
