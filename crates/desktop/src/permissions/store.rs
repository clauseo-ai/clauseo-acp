//! What Clauseo asked on this build: `<data folder>/permissions.json`, `{"build": "<build id>",
//! "asked": {"files_documents": "granted", "keychain": "denied"}}`. macOS keeps its own answers,
//! but tells them only when asked again, so the first-start step keeps what it saw here and
//! never asks twice. A different build id means nothing was asked: a new build is a new app to
//! macOS's privacy and keychain checks. Written whole and atomic (`Files::write_atomic`), as the
//! key marker is (`setup/keys/marker.rs`); a file that does not read or parse counts as empty.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use clauseo_settings::Files;
use serde::{Deserialize, Serialize};

use super::{OsPermissionState, PermissionId};

/// The file's name, in the data folder.
pub(crate) const FILE: &str = "permissions.json";

/// What the lawyer answered when Clauseo asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Answer {
    /// macOS granted it.
    Granted,
    /// macOS refused it, or the lawyer cancelled.
    Denied,
}

impl Answer {
    /// The row's state for this answer.
    pub(crate) fn state(self) -> OsPermissionState {
        match self {
            Self::Granted => OsPermissionState::Granted,
            Self::Denied => OsPermissionState::Denied,
        }
    }

    /// `Granted` when `granted`, else `Denied`.
    pub(crate) fn from_granted(granted: bool) -> Self {
        if granted { Self::Granted } else { Self::Denied }
    }
}

/// The answers recorded on this build.
pub(crate) type Asked = BTreeMap<PermissionId, Answer>;

/// The file's text.
#[derive(Debug, Serialize, Deserialize)]
struct StoreText {
    build: String,
    #[serde(default)]
    asked: Asked,
}

/// One read, change and write of the file at a time, in this process.
static WRITES: Mutex<()> = Mutex::new(());

/// The file, the files it goes through and this build's id.
pub(crate) struct Store {
    files: Arc<dyn Files + Send + Sync>,
    path: PathBuf,
    build: String,
}

impl Store {
    /// The file in `data_dir`, for the build `build`.
    pub(crate) fn new(files: Arc<dyn Files + Send + Sync>, data_dir: &Path, build: String) -> Self {
        Self {
            files,
            path: data_dir.join(FILE),
            build,
        }
    }

    /// What was asked on this build; nothing when the file is missing, does not read or parse,
    /// or names another build.
    pub(crate) fn asked(&self) -> Asked {
        match self.files.read(&self.path) {
            Ok(Some(bytes)) => match serde_json::from_slice::<StoreText>(&bytes) {
                Ok(text) if text.build == self.build => text.asked,
                Ok(_) => Asked::new(),
                Err(error) => {
                    tracing::warn!(
                        kind = ?error.classify(),
                        "permissions.json does not parse: nothing counts as asked"
                    );
                    Asked::new()
                }
            },
            Ok(None) => Asked::new(),
            Err(error) => {
                tracing::warn!(
                    kind = ?error.kind(),
                    "permissions.json does not read: nothing counts as asked"
                );
                Asked::new()
            }
        }
    }

    /// Records `answer` for `id` on this build, keeping this build's other answers and dropping
    /// another build's.
    pub(crate) fn record(&self, id: PermissionId, answer: Answer) -> io::Result<()> {
        let _one = WRITES.lock().unwrap_or_else(PoisonError::into_inner);
        let mut asked = self.asked();
        asked.insert(id, answer);
        let text = StoreText {
            build: self.build.clone(),
            asked,
        };
        let bytes = serde_json::to_vec_pretty(&text).map_err(io::Error::other)?;
        self.files.write_atomic(&self.path, &bytes)
    }
}

/// This build's id: the version About shows (tauri.conf.json's `version`, plus
/// `CLAUSEO_VERSION_SUFFIX` when the Rust build set one, `ui/app/src/settings/app-version.ts`),
/// then `+<mtime>-<length>` of the running executable. The version alone would not do: release
/// dry runs set the suffix for the UI only, so every ad hoc build reads `0.0.0`, yet macOS takes
/// each new build for a new app and asks again. `exe` is the executable's modified time in whole
/// seconds since the epoch and its length; `None` (unreadable) gives the version alone.
pub(crate) fn build_id(version: &str, suffix: Option<&str>, exe: Option<(u64, u64)>) -> String {
    let version = match suffix {
        Some(suffix) if !suffix.is_empty() => format!("{version}-{suffix}"),
        _ => version.to_owned(),
    };
    match exe {
        Some((mtime, len)) => format!("{version}+{mtime}-{len}"),
        None => version,
    }
}

/// The running executable's modified time (whole seconds since the epoch) and length, for
/// [`build_id`]; `None` when either cannot be read.
pub(crate) fn exe_identity() -> Option<(u64, u64)> {
    let metadata = std::fs::metadata(std::env::current_exe().ok()?).ok()?;
    let mtime = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((mtime, metadata.len()))
}
