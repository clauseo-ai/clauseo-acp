//! The whole environment of a sign-in, status or detection child, built from a base (the app's
//! own environment, [`crate::SigninConfig::base_env`]).
//!
//! The child gets exactly this map: the caller's launcher is made with
//! `SpawnOptions::clear_env`, because a `LaunchSpec` can only add or override variables, and a
//! variable set to `""` is not the same as one unset (`CLAUDE_SECURESTORAGE_CONFIG_DIR=""` means
//! the lawyer's own store). Nothing here touches this process's environment.
//!
//! The rules, from the brief (S18, "The environment of every run"):
//! - "the inherited one minus every `SSH_*`, `NO_BROWSER` and `CLAUDE_CODE_REMOTE`", and minus
//!   `NODE_OPTIONS` and `NODE_PATH` (`clauseo_process::NODE_ENV`, which the spawn drops anyway);
//! - "plus the agent's home variables";
//! - "`ANTHROPIC_API_KEY=""` for Claude and `CODEX_API_KEY=""` for Codex, so a stray key cannot
//!   win and status reports only the plan login";
//! - "`HTTPS_PROXY` from `network.upstream_proxy`";
//! - "the agent's `network.ca_certs` entry as `NODE_EXTRA_CA_CERTS` for Claude and as
//!   `SSL_CERT_FILE` for Codex (Codex is a native binary)";
//! - "and `BROWSER`" (the shim, Claude's login runs on macOS and Linux).
//!
//! Names are matched without regard to ASCII case: Windows reads them that way, and on Unix a
//! lowercase `ssh_tty` dropped as well costs nothing.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use crate::platform::Os;

/// Which vendor's CLI a run starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Vendor {
    /// Claude Code, through the adapter's `--cli`.
    Claude,
    /// Codex, through `@openai/codex/bin/codex.js`.
    Codex,
}

/// What a run is for, and so which home variables it gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunKind<'a> {
    /// Detection: the lawyer's own login, looked at where the CLI keeps it by default.
    ///
    /// "with the run environment below minus the home variables and `HOME` change, and without
    /// the store override"; Claude "with `CLAUDE_CONFIG_DIR` and
    /// `CLAUDE_SECURESTORAGE_CONFIG_DIR` unset"; Codex "with `CODEX_HOME` unset".
    Detect,
    /// Clauseo's own login, in `home` (`Locations::agent_home`, as text, never canonicalised).
    ///
    /// Claude: "`CLAUDE_CONFIG_DIR=<data_dir>/agents/claude-acp/home` (new),
    /// `CLAUDE_SECURESTORAGE_CONFIG_DIR` unset". Codex:
    /// "`CODEX_HOME=<data_dir>/agents/codex-acp/home`", and "every Codex run but detection gets
    /// `HOME` set to Clauseo's Codex folder on macOS and Linux".
    Own {
        /// The agent's home folder.
        home: &'a Path,
    },
    /// A reused login, as a session sees it (the run-time fallback's status check).
    ///
    /// Claude: "`CLAUDE_CONFIG_DIR` set to Clauseo's folder … and
    /// `CLAUDE_SECURESTORAGE_CONFIG_DIR=""`"; "Claude's `HOME` is never changed: its default
    /// store path depends on it". Codex: as [`RunKind::Own`], since its login is a link in the
    /// same home.
    Reused {
        /// The agent's home folder.
        home: &'a Path,
    },
}

/// The `BROWSER` shim of one Claude login run (macOS and Linux).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shim<'a> {
    /// The `sh` script, `<data_dir>/agents/signin/browser`.
    pub(crate) browser: &'a Path,
    /// This run's URL file, created mode 0600: `CLAUSEO_SIGNIN_URL_FILE`.
    pub(crate) url_file: &'a Path,
}

/// Everything one run's environment is built from. Its `Debug` names the base environment's
/// variables and never prints a value, nor the proxy setting (which may hold a password).
#[derive(Clone, Copy)]
pub(crate) struct EnvInputs<'a> {
    /// The app's environment: [`crate::SigninConfig::base_env`].
    pub(crate) base: &'a BTreeMap<OsString, OsString>,
    /// Whose CLI runs.
    pub(crate) vendor: Vendor,
    /// What for.
    pub(crate) kind: RunKind<'a>,
    /// The OS the run is on: `HOME` moves for Codex on macOS and Linux only.
    pub(crate) os: Os,
    /// `network.upstream_proxy` (else the environment's, as the caller resolved it).
    pub(crate) proxy: Option<&'a str>,
    /// The agent's `network.ca_certs` file.
    pub(crate) ca_file: Option<&'a Path>,
    /// The `BROWSER` shim, for Claude's login runs on macOS and Linux; ignored for Codex,
    /// which opens its own browser.
    pub(crate) shim: Option<Shim<'a>>,
}

impl std::fmt::Debug for EnvInputs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvInputs")
            .field("base", &self.base.keys().collect::<Vec<_>>())
            .field("vendor", &self.vendor)
            .field("kind", &self.kind)
            .field("os", &self.os)
            .field("proxy", &self.proxy.map(|_| "<set>"))
            .field("ca_file", &self.ca_file)
            .field("shim", &self.shim)
            .finish()
    }
}

/// Names dropped by prefix: every `SSH_*`.
const STRIPPED_PREFIXES: [&str; 1] = ["SSH_"];

/// Names dropped from every run: the remote-session hints the CLIs read to skip the browser,
/// and every variable this module sets itself, so an inherited one never survives a run that
/// does not set it (detection's homes) or leaks a stale value.
/// An inherited `BROWSER` stays when there is no shim (Windows, Codex): it is the lawyer's own
/// choice of browser.
const STRIPPED: [&str; 6] = [
    "NO_BROWSER",
    "CLAUDE_CODE_REMOTE",
    "CLAUDE_CONFIG_DIR",
    "CLAUDE_SECURESTORAGE_CONFIG_DIR",
    "CODEX_HOME",
    "CLAUSEO_SIGNIN_URL_FILE",
];

/// Codex's argument that keeps its login a file in `CODEX_HOME`: "with
/// `-c cli_auth_credentials_store="file"` on every Codex run but detection". The value is TOML,
/// so the quotes are part of the argument.
const CODEX_STORE: [&str; 2] = ["-c", "cli_auth_credentials_store=\"file\""];

/// The child's whole environment for `inputs`: the base minus the stripped names, plus the
/// run's own variables.
pub(crate) fn run_env(inputs: &EnvInputs<'_>) -> BTreeMap<OsString, OsString> {
    let mut env: BTreeMap<OsString, OsString> = inputs
        .base
        .iter()
        .filter(|(name, _)| !stripped(name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    match inputs.vendor {
        Vendor::Claude => {
            set(&mut env, "ANTHROPIC_API_KEY", OsStr::new(""));
            match inputs.kind {
                RunKind::Detect => {}
                RunKind::Own { home } => set(&mut env, "CLAUDE_CONFIG_DIR", home.as_os_str()),
                RunKind::Reused { home } => {
                    set(&mut env, "CLAUDE_CONFIG_DIR", home.as_os_str());
                    set(&mut env, "CLAUDE_SECURESTORAGE_CONFIG_DIR", OsStr::new(""));
                }
            }
            if let Some(ca) = inputs.ca_file {
                set(&mut env, "NODE_EXTRA_CA_CERTS", ca.as_os_str());
            }
            if let Some(shim) = inputs.shim {
                set(&mut env, "BROWSER", shim.browser.as_os_str());
                set(
                    &mut env,
                    "CLAUSEO_SIGNIN_URL_FILE",
                    shim.url_file.as_os_str(),
                );
            }
        }
        Vendor::Codex => {
            set(&mut env, "CODEX_API_KEY", OsStr::new(""));
            if let RunKind::Own { home } | RunKind::Reused { home } = inputs.kind {
                set(&mut env, "CODEX_HOME", home.as_os_str());
                if matches!(inputs.os, Os::Darwin | Os::Linux) {
                    set(&mut env, "HOME", home.as_os_str());
                }
            }
            if let Some(ca) = inputs.ca_file {
                set(&mut env, "SSL_CERT_FILE", ca.as_os_str());
            }
        }
    }
    if let Some(proxy) = inputs.proxy {
        set(&mut env, "HTTPS_PROXY", OsStr::new(proxy));
    }
    env
}

/// The arguments every Codex run but detection adds after its command:
/// `-c cli_auth_credentials_store="file"`. Detection gets none ("without the store override").
pub(crate) fn codex_store_args(kind: RunKind<'_>) -> Vec<OsString> {
    match kind {
        RunKind::Detect => Vec::new(),
        RunKind::Own { .. } | RunKind::Reused { .. } => {
            CODEX_STORE.iter().map(OsString::from).collect()
        }
    }
}

/// Whether `name` is dropped from every run.
fn stripped(name: &OsStr) -> bool {
    let upper = name.to_string_lossy().to_ascii_uppercase();
    STRIPPED_PREFIXES
        .iter()
        .any(|prefix| upper.starts_with(prefix))
        || STRIPPED.contains(&upper.as_str())
        || clauseo_process::is_node_env(name)
}

/// Sets `name` to `value`, first removing the base's spelling of it in any case, so a Windows
/// child never sees two spellings.
fn set(env: &mut BTreeMap<OsString, OsString>, name: &str, value: &OsStr) {
    env.retain(|key, _| !key.to_string_lossy().eq_ignore_ascii_case(name));
    env.insert(OsString::from(name), value.to_owned());
}

#[cfg(test)]
mod tests;
