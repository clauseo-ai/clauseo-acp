//! The copy of the server's `initialize` and `tools/list` results the relay answers from, per
//! protocol version: the last the server gave, cached per origin and version in
//! `<data>/connectors/cache/<hash>-<version>.json` (`{initialize, tools_list, fetched_at}`,
//! plain JSON), or the copies Clauseo ships when there is none (a first start, a new origin, a
//! version not seen yet). The server's answers depend on the version (the shapes) and, for
//! `initialize`, on the origin (its base URL in the instructions), never on the caller; so the
//! key is the two. The file holds the two results only: no credential, session id or header. It
//! is read once, when the relays start; one over [`MAX_CACHE_BYTES`], or one that does not read
//! as usable results, is ignored, and the next warm-up replaces it. Every warm-up writes it again
//! (`forward/warm.rs`), atomically (a temp file, then a rename), outside the lock.
//!
//! The shipped copies (`clauseo_connectors::CASE_LAW_*`) are the server's 2025-06-18 `initialize`
//! and `tools/list` and its 2025-03-26 `tools/list`. The rest is derived as the server does it:
//! 2025-11-25's `initialize` differs only in `protocolVersion`, 2025-03-26's also drops
//! `serverInfo.title`; 2025-11-25's list is 2025-06-18's. The instructions' base URL,
//! `https://data.clauseo.chat`, becomes the origin's.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_connectors::Origin;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::lock;

/// The largest cache file read or written.
pub(crate) const MAX_CACHE_BYTES: usize = 256 * 1024;

/// The versions the server's `initialize` accepts as asked, newest first; any other is answered
/// with the first. (The server also speaks 2026-07-28, which has no `initialize`: an agent that
/// speaks it sends none, and the relay forwards its messages as they come.)
pub(crate) const INITIALIZE_VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

/// The base URL the server's instructions name, replaced by the origin's.
const BASE_PLACEHOLDER: &str = "https://data.clauseo.chat";

/// An `initialize` result and a `tools/list` result.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Served {
    pub(crate) initialize: Arc<Value>,
    pub(crate) tools_list: Arc<Value>,
}

/// The copies in use, per version, and where they are kept.
#[derive(Debug)]
pub(crate) struct Copies {
    dir: PathBuf,
    origin: Origin,
    current: Mutex<BTreeMap<&'static str, Served>>,
}

/// The cache file for `origin` and `version` in `dir`: the first 16 bytes of the SHA-256 of the
/// origin's scheme, host and port, in hex, then the version, so each origin (production, staging,
/// a dev server) and each version has its own.
pub(crate) fn cache_path(dir: &Path, origin: &Origin, version: &str) -> PathBuf {
    let digest = Sha256::digest(origin.prefix().as_bytes());
    let name = digest
        .iter()
        .take(16)
        .fold(String::new(), |mut name, byte| {
            use std::fmt::Write as _;
            if write!(name, "{byte:02x}").is_err() {
                // Writing to a String does not fail.
            }
            name
        });
    dir.join(format!("{name}-{version}.json"))
}

/// The version the copies keep for `version`: itself when it is one of [`INITIALIZE_VERSIONS`],
/// else the newest.
fn known(version: &str) -> &'static str {
    INITIALIZE_VERSIONS
        .iter()
        .find(|known| **known == version)
        .or(INITIALIZE_VERSIONS.first())
        .copied()
        .unwrap_or_default()
}

impl Copies {
    /// The cached copies for `origin` in `dir`, else the shipped ones.
    pub(crate) fn load(dir: &Path, origin: &Origin) -> Copies {
        let current = INITIALIZE_VERSIONS
            .iter()
            .map(|version| {
                let copy = read(&cache_path(dir, origin, version))
                    .unwrap_or_else(|| shipped(version, origin));
                (*version, copy)
            })
            .collect();
        Copies {
            dir: dir.to_owned(),
            origin: origin.clone(),
            current: Mutex::new(current),
        }
    }

    /// The copy for `version`.
    pub(crate) fn current(&self, version: &str) -> Served {
        let version = known(version);
        lock(&self.current)
            .get(version)
            .cloned()
            .unwrap_or_else(|| shipped(version, &self.origin))
    }

    /// The copy for the newest version, for a connection before its `initialize`.
    pub(crate) fn newest(&self) -> Served {
        self.current("")
    }

    /// Takes the server's `initialize` and `tools/list` results for `version` as its copy, and
    /// writes its cache file (`fetched_at`: `now_ms`, Unix milliseconds), outside the lock. A
    /// pair too large for the file is used in memory only.
    pub(crate) fn store(&self, version: &str, served: Served, now_ms: i64) {
        let version = known(version);
        let file = json!({
            "initialize": &*served.initialize,
            "tools_list": &*served.tools_list,
            "fetched_at": now_ms,
        });
        lock(&self.current).insert(version, served);
        let bytes = serde_json::to_vec(&file).unwrap_or_default();
        if bytes.len() > MAX_CACHE_BYTES {
            tracing::warn!("case-law relay: the server's tool list is too large to cache");
            return;
        }
        let path = cache_path(&self.dir, &self.origin, version);
        let written = crate::write::create_dir_all(&self.dir)
            .and_then(|()| crate::write::replace_private_file(&path, &bytes));
        if let Err(err) = written {
            tracing::warn!(error = %crate::log_file::Cause(&err), "case-law relay: the tool list cache was not written");
        }
    }
}

/// The shipped copy for `version` (one of [`INITIALIZE_VERSIONS`]) on `origin`.
pub(crate) fn shipped(version: &str, origin: &Origin) -> Served {
    let mut initialize: Value =
        serde_json::from_str(clauseo_connectors::CASE_LAW_INITIALIZE).unwrap_or_else(|_| json!({}));
    if let Value::Object(map) = &mut initialize {
        map.insert("protocolVersion".to_owned(), json!(version));
        if version == "2025-03-26"
            && let Some(Value::Object(server)) = map.get_mut("serverInfo")
        {
            server.shift_remove("title");
        }
        if let Some(Value::String(text)) = map.get_mut("instructions") {
            *text = text.replace(BASE_PLACEHOLDER, &origin.prefix());
        }
    }
    let list = if version == "2025-03-26" {
        clauseo_connectors::CASE_LAW_TOOLS_LIST_2025_03_26
    } else {
        clauseo_connectors::CASE_LAW_TOOLS_LIST
    };
    let tools_list = serde_json::from_str(list).unwrap_or_else(|_| json!({"tools": []}));
    Served {
        initialize: Arc::new(initialize),
        tools_list: Arc::new(tools_list),
    }
}

/// Whether `initialize` and `tools_list` are results the relay can answer with: an object, and
/// an object with a non-empty `tools` array of objects that each have a name and an object
/// `inputSchema`, and no further page. An empty or broken list never replaces a good copy.
pub(crate) fn usable(initialize: &Value, tools_list: &Value) -> bool {
    let named = |tool: &Value| {
        tool.get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| !name.is_empty())
            && tool.get("inputSchema").is_some_and(Value::is_object)
    };
    let tools_ok = tools_list
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty() && tools.iter().all(named));
    initialize.is_object() && tools_ok && tools_list.get("nextCursor").is_none_or(Value::is_null)
}

/// The cache file at `path`, when it is at most [`MAX_CACHE_BYTES`] and holds usable results.
fn read(path: &Path) -> Option<Served> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_CACHE_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    file.take(limit).read_to_end(&mut bytes).ok()?;
    if bytes.len() > MAX_CACHE_BYTES {
        tracing::warn!(
            "case-law relay: the tool list cache is too large; the shipped copy is used"
        );
        return None;
    }
    let Ok(Value::Object(mut map)) = serde_json::from_slice::<Value>(&bytes) else {
        tracing::warn!(
            "case-law relay: the tool list cache does not read; the shipped copy is used"
        );
        return None;
    };
    let initialize = map.remove("initialize")?;
    let tools_list = map.remove("tools_list")?;
    if !usable(&initialize, &tools_list) {
        tracing::warn!(
            "case-law relay: the tool list cache is not usable; the shipped copy is used"
        );
        return None;
    }
    Some(Served {
        initialize: Arc::new(initialize),
        tools_list: Arc::new(tools_list),
    })
}

#[cfg(test)]
mod tests;
