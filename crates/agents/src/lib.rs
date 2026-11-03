//! `clauseo-agents`: installs the agents' ACP adapters (claude-agent-acp, codex-acp) with no
//! terminal and no admin rights.
//!
//! # Owns
//!
//! - The snapshot ([`Snapshot`]): the two agents, their pinned packages (embedded from G16's
//!   pins file, `crates/limits/tests/live/package.json` and its lock), the hosts their install
//!   and sign-in use, their download size and disk need per platform, and Node 24.22.0's archive
//!   SHA-256 per platform.
//! - Each agent's derived pins: a `package.json` with that agent's one root dependency and a lock
//!   with every entry reachable from it through `dependencies`, `optionalDependencies` and
//!   `peerDependencies`, so one agent's install fetches none of the other's packages.
//! - The layout under `<data_dir>/agents`: `runtime/node-v<version>-<os>-<arch>/` (a downloaded
//!   Node), `<id>/install/<hash>/` (`package.json`, `package-lock.json`, `node_modules/`,
//!   `clauseo-receipt.json`), `downloads/` (files named by digest), `npm-cache/`. `<id>/home/`
//!   and `signin/` are S18's. `limits` writes `clauseo-claude-usage.mjs` into Claude's install
//!   folder; nothing here touches it.
//! - Node: the user's when it is a regular file, reports v22 or later and has `npm-cli.js`
//!   beside it; otherwise the pinned download, checked by SHA-256.
//! - Downloads (Rust's): each tarball the derived lock lists for this platform, with `Range`
//!   resume and the lock's SHA-512, then `npm cache add` and `npm ci --offline --ignore-scripts
//!   --no-bin-links` (npm's) in a temp folder, renamed into place, the receipt written last.
//! - Progress, the four failure kinds and their logs, one download at a time, firm IT mode (IT's
//!   runtime, an offline bundle, a mirror, a proxy and a CA file).
//!
//! # Interface
//!
//! - `Agents::start(config, launcher, timer)` gives an `Agents` and the `Receiver` of
//!   [`AgentsEvent`]s; `install`, `pause`, `retry(agent, use_system_proxy)`, `state` and
//!   `installed`.
//! - [`AgentsConfig`], [`InstallConfig`], [`NodeSearch`], [`Clock`] and [`SystemClock`].
//! - [`InstallState`], [`Failure`], [`FailureKind`] (with its error code), [`InstallLog`] and
//!   [`Installed`].
//! - [`Snapshot`] and [`Platform`].
//!
//! # Invariants
//!
//! - Only `nodejs.org` and `registry.npmjs.org` are contacted, or the firm's mirror for
//!   tarballs; an IT runtime and an offline bundle contact nothing.
//! - Every downloaded byte is checked: SHA-512 from the lock, SHA-256 from the snapshot. A
//!   mismatch deletes the file. A mirror cannot change a byte.
//! - No install script runs, and no install folder holds a symbolic link or junction.
//! - Each child (`node --version`, npm) starts through the `process::Launcher` it is given,
//!   outside any sandbox, as `node <script>`: never `npm.cmd`, a shell or a `PATH` lookup.
//! - No log holds a URL's user name or password, raw npm output, or a path outside the data
//!   folder.
//! - Nothing runs while the app is idle once installs are done.
//!
//! # Depends on
//!
//! `protocol` (`AgentId`), `process`, `settings`, `serde`, `serde_json`, `futures`,
//! `async-channel`, `async-io`, `ureq` (rustls, blocking, on its own thread), `sha2`, `base64`,
//! `flate2`, `tar`, `zip`, `thiserror`, `dunce`; `nix` on Unix and `windows` on Windows (free
//! disk space). Never `sandbox`, `tauri` or `tokio`.
//!
//! # Start reading
//!
//! `snapshot.rs` and `pins.rs` (what installs), `worker.rs` (the `Agents` thread),
//! `install.rs` (one agent's install), `download.rs`, `node.rs`, `receipt.rs`, `write.rs`.

mod cancel;
mod config;
mod disk;
mod download;
mod integrity;
mod log;
mod node;
mod pins;
mod platform;
mod receipt;
mod snapshot;
mod state;
mod sysproxy;
mod write;

/// Test support: a local registry, test resolvers, temp folders and tiny npm tarballs.
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use config::{AgentsConfig, Clock, InstallConfig, NodeSearch, SystemClock};
pub use log::InstallLog;
pub use pins::{AgentPins, PinsError};
pub use platform::{Arch, Libc, Os, Platform};
pub use receipt::{install_folder, install_root, script_path};
pub use snapshot::{
    AgentEntry, AgentSizes, NPM_REGISTRY, NodeArchive, NodeEntry, Snapshot, SnapshotError,
};
pub use state::{AgentsEvent, Failure, FailureKind, InstallState, Installed};

/// Tests only: a host-name resolver standing in for the OS's. `Err` reads as a failed name
/// lookup (offline).
#[cfg(feature = "test-support")]
#[derive(Clone)]
pub struct TestResolver(
    /// Resolves a host name to its addresses.
    pub std::sync::Arc<ResolveFn>,
);

/// Tests only: what a [`TestResolver`] calls.
#[cfg(feature = "test-support")]
pub type ResolveFn = dyn Fn(&str) -> std::io::Result<Vec<std::net::SocketAddr>> + Send + Sync;

#[cfg(feature = "test-support")]
impl std::fmt::Debug for TestResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TestResolver")
    }
}
