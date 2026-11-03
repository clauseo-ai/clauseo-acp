//! The test's folder under the temp folder, the session id and the inputs from the environment.
//! Chrome's temp profile folders are looked for here too.

#![expect(
    clippy::disallowed_methods,
    reason = "a by-hand smoke test: it starts the guard, writes its own folder and waits on the \
              wall clock"
)]

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Chrome's temp profile folder, made under the temp folder
/// (agent-browser v0.41.1 `cli/src/native/cdp/chrome.rs:545-546`).
pub(crate) const PROFILE_PREFIX: &str = "agent-browser-chrome-";

/// The absolute path in variable `name`, which must name an existing file.
pub(crate) fn env_file(name: &str) -> PathBuf {
    let value = std::env::var_os(name).unwrap_or_else(|| {
        panic!("{name} is not set; see the steps at the top of browser_smoke.rs")
    });
    let path = PathBuf::from(value);
    assert!(
        path.is_absolute() && path.is_file(),
        "{name}={} must be the absolute path of an existing file",
        path.display()
    );
    path
}

/// The test's folder under the temp folder, and the session id named after it.
pub(crate) struct Scratch {
    pub(crate) root: PathBuf,
    pub(crate) session: String,
}

impl Scratch {
    pub(crate) fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        // The first 16 characters name the Unix socket folder, so the varying part leads.
        let session = format!("smoke{:08x}{:x}", nanos & 0xffff_ffff, std::process::id());
        let root = std::env::temp_dir().join(format!("clauseo-browser-{session}"));
        std::fs::create_dir_all(root.join("browser")).expect("make the test folder");
        std::fs::write(root.join("config.json"), "{}").expect("write the config");
        Self { root, session }
    }

    /// `--cwd`.
    pub(crate) fn cwd(&self) -> PathBuf {
        self.root.join("browser")
    }

    /// Removes the folder and, on Unix, the guard's socket folder under `/tmp`.
    pub(crate) fn remove(&self) {
        let mut folders = vec![self.root.clone()];
        if cfg!(unix) {
            let prefix = self.session.get(..16).unwrap_or(&self.session);
            folders.push(PathBuf::from(format!("/tmp/clauseo-ab-{prefix}")));
        }
        for folder in folders {
            if let Err(error) = std::fs::remove_dir_all(&folder) {
                eprintln!("could not remove {}: {error}", folder.display());
            }
        }
    }
}

/// Every folder under `root` whose name starts with [`PROFILE_PREFIX`].
pub(crate) fn profiles(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(PROFILE_PREFIX)
                {
                    found.push(path.clone());
                }
                pending.push(path);
            }
        }
    }
    found
}
