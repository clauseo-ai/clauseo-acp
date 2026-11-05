//! The test's folder under the temp folder, the session id and the inputs from the environment.
//! Chrome's temp profile folders are looked for here too.

#![expect(
    clippy::disallowed_methods,
    reason = "a by-hand smoke test: it starts the guard, writes its own folder and waits on the \
              wall clock"
)]

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clauseo_sandbox::ports::{self, PortClaim};

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
    /// The guard's `--cdp-port`, claimed as the host claims it.
    pub(crate) cdp: PortClaim,
    /// `AGENT_BROWSER_STREAM_PORT`.
    pub(crate) stream: PortClaim,
    /// Windows: the guard's `--daemon-port`, the hash of [`Self::session`].
    pub(crate) daemon: Option<PortClaim>,
    /// macOS and Linux: `AGENT_BROWSER_SOCKET_DIR`, made as the host makes it; removed on drop.
    #[cfg(unix)]
    pub(crate) socket_dir: clauseo_sandbox::BrowserSocketDir,
}

impl Scratch {
    pub(crate) fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let base = format!("smoke{:08x}{:x}", nanos & 0xffff_ffff, std::process::id());
        let (session, daemon) = session_and_daemon(&base);
        let root = std::env::temp_dir().join(format!("clauseo-browser-{base}"));
        std::fs::create_dir_all(root.join("browser")).expect("make the test folder");
        std::fs::write(root.join("config.json"), "{}").expect("write the config");
        Self {
            root,
            session,
            cdp: ports::claim_port().expect("claim the CDP port"),
            stream: ports::claim_port().expect("claim the stream port"),
            daemon,
            #[cfg(unix)]
            socket_dir: clauseo_sandbox::BrowserSocketDir::create()
                .expect("make the socket folder"),
        }
    }

    /// `--cwd`.
    pub(crate) fn cwd(&self) -> PathBuf {
        self.root.join("browser")
    }

    /// Removes the folder; the socket folder goes when the value drops.
    pub(crate) fn remove(&self) {
        if let Err(error) = std::fs::remove_dir_all(&self.root) {
            eprintln!("could not remove {}: {error}", self.root.display());
        }
    }
}

/// macOS and Linux: `base` as the session, no daemon port. Windows: `base` and a counter,
/// counted up until agent-browser's daemon port for the name (the host's
/// `browser::session::daemon_port_of`, copied: it is crate-private) is one `claim_port_at`
/// takes in the jail's range.
fn session_and_daemon(base: &str) -> (String, Option<PortClaim>) {
    if cfg!(unix) {
        return (base.to_owned(), None);
    }
    for at in 0..200_000_u32 {
        let name = format!("{base}-{at}");
        let mut hash: i32 = 0;
        for c in name.chars() {
            hash = (hash << 5).wrapping_sub(hash).wrapping_add(c as i32);
        }
        let port = u16::try_from(49_152 + hash.unsigned_abs() % 16_383).expect("a port");
        if ports::WINDOWS_PORTS.contains(&port)
            && let Ok(claim) = ports::claim_port_at(port)
        {
            return (name, Some(claim));
        }
    }
    panic!("no session name gave a daemon port in the jail's range");
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
