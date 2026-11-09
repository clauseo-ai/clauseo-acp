//! `clauseo-agents`: installs the agents' ACP adapters (claude-agent-acp, codex-acp) with no
//! terminal and no admin rights, and signs the lawyer in through each vendor's own login (S18).
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
//!   `clauseo-receipt.json`), `downloads/` (files named by digest), `npm-cache/`, `npmrc`,
//!   `npmrc-global` and `.lock`. `<id>/home/`, `<id>/detect-scratch/` and `signin/` are S18's.
//!   `limits` writes `clauseo-claude-usage.mjs` into Claude's install folder; nothing here
//!   touches it.
//! - The pinned Python runtime (`python.rs`): Clauseo's archive for this platform (none on
//!   Linux, and none anywhere until the archives are published), checked by SHA-256 and
//!   unpacked read-only on Unix under `runtime/<archive stem>/` with a manifest and a receipt,
//!   run once before its receipt, as the second step of an agent's install job. Its own
//!   [`PythonState`]; a failure never blocks the agent's `Ready`. [`python_command_dir`] is the
//!   folder for the agent's `PATH`; [`python_path_dirs`] the folders an agent session's `PATH`
//!   takes (all three on Windows: pandoc's, the interpreter's and pip's).
//! - Node: the user's when it is a regular file, reports v22 or later and has `npm-cli.js`
//!   beside it; otherwise the pinned download, checked by SHA-256.
//! - Downloads (Rust's): each tarball the derived lock lists for this platform, with `Range`
//!   resume and the lock's SHA-512, then `npm ci --offline --ignore-scripts --no-bin-links`
//!   (npm's) in a temp folder, reading each checked tarball in place through a `file:`
//!   `resolved` URL and checking it against the lock's `integrity` again (no `npm cache add`),
//!   renamed into place, the receipt written last.
//! - Progress, the four failure kinds and their logs, one download at a time, firm IT mode (IT's
//!   runtime, an offline bundle, a mirror, a proxy and a CA file).
//! - Sign-in (S18, decision 0046): the sign-in states and codes, the pasted code, the whole
//!   environment of every sign-in, status and detection child (`signin/env.rs`), the runner
//!   that starts one and keeps its output in a 64 KB ring never logged (`signin/run.rs`), the
//!   status parsers (`status.rs`) and file identity (`fileid.rs`); one job model for every
//!   sign-in, sign-out and status check (`signin/job.rs`, one job per agent on its own thread);
//!   the queued start (`signin/queue.rs`: a sign-in pressed before the install is `Ready` or
//!   while its sign-in host does not resolve is `Queued`, and starts by itself once both hold);
//!   Claude's flow (`signin/claude.rs`: hidden `auth login`, status polled every 1.5 s, the
//!   nudge at 30 s, expiry at 10 minutes, the paste route, sign-out) and its `BROWSER` shim on
//!   macOS and Linux (`signin/shim.rs`); Codex's flow (`signin/codex.rs`: the port 1455 probe
//!   and "Try again", hidden `login` or `login --device-auth` with the device code read
//!   strictly from its output, `login status` polled, the account from `app-server`'s
//!   `account/read`, sign-out); detection of the lawyer's own login (`signin/detect.rs`, Claude's
//!   run in a throwaway config folder, `signin/scratch.rs`; it never writes the lawyer's), the
//!   status check and the lawyer's choices about it (`signin/found.rs`), Clauseo's `login-source`
//!   and `no-reuse` markers (`signin/marker.rs`), and Codex's hard link, guard and split check
//!   (`signin/reuse.rs`, the one file that may name Codex's login file, and never opens it).
//!
//! # Interface
//!
//! - [`Agents::start`]`(config, launcher, timer)` gives an [`Agents`] and the `Receiver` of
//!   [`AgentsEvent`]s ([`Agents::start_with_launchers`]`(config, install, signin, timer)` with
//!   one launcher for installs and one, clearing the environment, for sign-in); `install`,
//!   `pause`, `retry(agent, use_system_proxy)`, `state` and `installed`; `sign_in(agent,
//!   Method)`, `try_again` (Codex's busy port), `cancel_sign_in`,
//!   `paste_code(agent, PasteCode)`, `sign_out`, `check_status`, `find_login`,
//!   `use_found_login`, `use_different_account`, `stop_using_login(agent, Option<Found>)` (each
//!   `Result<(), SigninError>`), `signin_state` and `sign_in_going`.
//! - [`AgentsConfig`], [`InstallConfig`], [`NodeSearch`], [`Clock`] and [`SystemClock`];
//!   [`Agents::set_network`] with a [`Network`] (the proxies and CA files, for the next job), and
//!   [`ca_cert_var`], the CA variable of each agent's CLI.
//! - [`InstallState`], [`Failure`], [`FailureKind`] (with its error code), [`InstallLog`] and
//!   [`Installed`]; [`PythonState`] ([`AgentsEvent::Python`], [`Agents::python_state`]),
//!   [`PythonFailure`], [`PythonFailureKind`], [`PythonBlocker`], [`python_command_dir`] and
//!   [`python_path_dirs`].
//! - [`SigninState`], [`Found`], [`LoginFrom`], [`Source`], [`SigninCode`] (with its error
//!   code), [`Method`], [`SigninError`], [`PasteCode`] and [`SigninConfig`];
//!   [`SigninNote`]; [`AgentsEvent::Signin`], [`AgentsEvent::OpenUrl`] and
//!   [`AgentsEvent::SigninNote`]; [`same_file`] and [`is_sign_in_again`]; Codex's split check and
//!   guard, [`codex_link_holds`] ([`CodexLink`]) and [`codex_login_shared`], and the leave order's
//!   unlink step, [`unlink_reused_login`].
//! - [`Snapshot`] and [`Platform`].
//! - [`keys`]: each agent's API key in the OS keychain (`store`, `load`, `clear`, `available`).
//!
//! # Invariants
//!
//! - Only `nodejs.org` and `registry.npmjs.org` are contacted (and, once its archives are
//!   published, Python's release asset on `github.com` and its redirect), over https alone, or
//!   the firm's https mirror for tarballs; an IT runtime and an offline bundle contact nothing.
//!   Sign-in's own network use is one name lookup before a login run (the agent's first sign-in
//!   host, or the proxy's); the vendors' CLIs make the sign-in's connections themselves.
//! - Every downloaded byte is checked: SHA-512 from the lock, SHA-256 from the snapshot. A
//!   mismatch deletes the file. A mirror cannot change a byte.
//! - No install script runs, and no install folder holds a symbolic link or junction.
//! - Each child (`node --version`, npm) starts through the `process::Launcher` it is given,
//!   outside any sandbox, as `node <script>`: never `npm.cmd`, a shell or a `PATH` lookup.
//! - No log holds a URL's user name or password, raw npm output, or a path outside the data
//!   folder.
//! - Callers never wait on I/O: the work runs on the worker thread, one thread per install and
//!   one per sign-in job.
//!   Installs run one after another, and a paused install sends nothing after its `Paused`.
//! - Nothing runs while the app is idle once installs are done.
//! - An API key lives only in the OS keychain: never in a file, a log, a `Debug` or an error.
//! - Clauseo never opens or reads an agent's credential, and no file under `src` names one
//!   outside a comment (`tests/credential_names.rs`). Emails and plans come only from the
//!   vendors' own status output. A pasted code is zeroed once written and never logged.
//! - No `auth logout` or `codex logout` runs while the source is `Reused`, and no Codex `login`
//!   or `logout` while the home's login file is the lawyer's (same file id). Reuse starts only
//!   on `use_found_login`; detection never starts it.
//! - Only an address the `BROWSER` shim captured, on one of the snapshot's sign-in hosts over
//!   https, becomes `OpenUrl`, once; an address in a run's output never does. No status
//!   process runs once a sign-in is confirmed, cancelled, expired or failed.
//! - A sign-in, status or detection child gets exactly the environment `signin/env.rs` builds
//!   (`SSH_*`, `NO_BROWSER`, `CLAUDE_CODE_REMOTE`, `NODE_OPTIONS` and `NODE_PATH` removed).
//!   **The caller's sign-in launcher must be made with `SpawnOptions::clear_env`** (R20's
//!   desktop wiring): a launcher that inherits the app's environment would pass those names on,
//!   since a `LaunchSpec` can add variables but never remove one (only `NODE_OPTIONS` and
//!   `NODE_PATH`, which every spawn drops).
//!
//! # Depends on
//!
//! `protocol` (`AgentId`), `process`, `codex_appserver` (Codex's `account/read`), `serde`,
//! `serde_json`, `futures`, `async-channel`, `tracing` (the Node line at each install and
//! start), `ureq` (rustls, blocking, on the install's own thread), `rustls`, `sha2`, `base64`,
//! `flate2`, `tar`, `zip`, `thiserror`, `dunce`, `zeroize` (the pasted code and the API
//! keys); `keyring` on macOS and Windows only (the keychain; none
//! on Linux); `nix` on Unix and `windows` on Windows (free disk space, the Windows proxy setting,
//! file ids, and the signer and version of the VC++ runtime DLL placed beside Codex's ARM64
//! `rg.exe`: `vcruntime.rs`); `rcgen` with `test-support` only. Never `sandbox`, `tauri` or
//! `tokio`; `settings` only in tests and the example (the caller passes the data folder and the
//! agent homes in).
//!
//! # Start reading
//!
//! `snapshot.rs` and `pins.rs` (what installs), `worker.rs` (the `Agents` thread),
//! `install.rs` (one agent's install), `download.rs`, `node.rs`, `receipt.rs`, `write.rs`;
//! for sign-in, `signin.rs` and its modules, then `status.rs` and `fileid.rs`.

mod cancel;
mod config;
mod disk;
mod download;
mod fileid;
mod install;
mod integrity;
pub mod keys;
mod log;
mod node;
mod pins;
mod platform;
mod python;
mod receipt;
#[cfg_attr(
    not(any(windows, test, feature = "test-support")),
    expect(dead_code, reason = "only Windows and tests settle Codex's rg.exe")
)]
mod sac;
mod signin;
mod snapshot;
mod state;
mod status;
mod sysproxy;
#[cfg_attr(
    not(any(windows, test)),
    expect(
        dead_code,
        reason = "only Windows reads a DLL's signer and version; elsewhere nothing is placed"
    )
)]
mod vcruntime;
mod worker;
mod write;

/// Test support: a local registry, test resolvers, temp folders and tiny npm tarballs.
#[cfg(any(test, feature = "test-support"))]
pub mod testing;

#[cfg(feature = "test-support")]
pub use config::RenameFault;
pub use config::{
    AgentsConfig, Clock, InstallConfig, Network, NodeSearch, RefreshFn, SigninConfig, SystemClock,
};
pub use fileid::same_file;
pub use log::InstallLog;
pub use pins::{AgentPins, PinsError};
pub use platform::{Arch, Libc, Os, Platform};
pub use python::{python_command_dir, python_contents_match, python_path_dirs};
pub use receipt::{install_folder, install_root, script_path};
#[cfg(feature = "test-support")]
pub use sac::SacForTests;
pub use sac::SacState;
pub use signin::reuse::{CodexLink, codex_link_holds, codex_login_shared, unlink_reused_login};
pub use signin::{
    Found, LoginFrom, Method, PasteCode, SigninCode, SigninError, SigninNote, SigninState, Source,
    Unconfirmed,
};
pub use snapshot::{
    AgentEntry, AgentSizes, NPM_REGISTRY, NodeArchive, NodeEntry, PythonArchive, PythonEntry,
    Snapshot, SnapshotError,
};
pub use state::{
    AgentsEvent, Failure, FailureKind, InstallState, Installed, PythonBlocker, PythonFailure,
    PythonFailureKind, PythonState,
};
pub use status::is_sign_in_again;
pub use worker::{Agents, OwnLogin};

/// The variable `agent`'s CLI reads an extra CA file (`network.ca_certs`) from:
/// `NODE_EXTRA_CA_CERTS` for Claude, `CODEX_CA_CERTIFICATE` for Codex; `None` for an agent with
/// no vendor here. The one mapping: sign-in's runs take it too.
pub fn ca_cert_var(agent: &clauseo_protocol::AgentId) -> Option<&'static str> {
    signin::vendor_of(agent).map(signin::env::ca_var)
}

/// `NODE_USE_SYSTEM_CA`: set to `1`, Node trusts the OS's certificate store beside its own
/// roots (Node 24.6 and 22.19 on). Sign-in's runs set it; desktop's launches and readers take it
/// from here.
pub const SYSTEM_CA_VAR: &str = signin::env::SYSTEM_CA_VAR;

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
