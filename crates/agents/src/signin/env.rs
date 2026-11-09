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
//! - "`HTTPS_PROXY` from `network.upstream_proxy`", with `NO_PROXY` holding the loopback names
//!   (then the inherited ones), since the OAuth callback is on this computer (S17);
//! - "the agent's `network.ca_certs` entry as `NODE_EXTRA_CA_CERTS` for Claude and as
//!   `CODEX_CA_CERTIFICATE` for Codex" (S17: Codex loads it beside the system roots, while
//!   `SSL_CERT_FILE` would reach the tools in its shell, many of which take it to replace them;
//!   an inherited `SSL_CERT_FILE` is left as it is);
//! - `NODE_USE_SYSTEM_CA=1` unless the base sets it (S17: Node trusts the OS store too, where a
//!   firm's TLS-inspecting proxy puts its root; Node 24.6 and 22.19 on, older ones ignore it);
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
    /// Codex, through `@openai/codex/bin/codex.js` found in the install (`Installed::codex_cli`),
    /// never the install's `script`, codex-acp's ACP adapter.
    Codex,
}

/// What a run is for, and so which home variables it gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunKind<'a> {
    /// Detection: the lawyer's own login, read from the store the CLI keeps it in by default,
    /// while every file the CLI writes lands in `config`, a throwaway Clauseo folder.
    ///
    /// Claude: `CLAUDE_CONFIG_DIR=<config>` and `CLAUDE_SECURESTORAGE_CONFIG_DIR=""`, the shape
    /// of [`RunKind::Reused`], `HOME` the lawyer's (the default store path depends on it). Claude
    /// Code's `auth status` rewrites its config file and writes a backup the first time a
    /// version meets a config folder, so detection never runs it on the lawyer's
    /// `~/.claude` and `~/.claude.json` (`super::scratch`). Codex: no detection run starts
    /// its CLI (its `login status` writes `~/.codex/tmp/arg0`); `CODEX_HOME` is unset.
    Detect {
        /// The run's own config folder, fresh for the run and removed after it.
        config: &'a Path,
    },
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
                RunKind::Own { home } => set(&mut env, "CLAUDE_CONFIG_DIR", home.as_os_str()),
                RunKind::Reused { home } | RunKind::Detect { config: home } => {
                    set(&mut env, "CLAUDE_CONFIG_DIR", home.as_os_str());
                    set(&mut env, "CLAUDE_SECURESTORAGE_CONFIG_DIR", OsStr::new(""));
                }
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
        }
    }
    if let Some(ca) = inputs.ca_file {
        set(&mut env, ca_var(inputs.vendor), ca.as_os_str());
    }
    if let Some(proxy) = inputs.proxy {
        set(&mut env, "HTTPS_PROXY", OsStr::new(proxy));
        let no_proxy = with_loopback(&env);
        set(&mut env, "NO_PROXY", OsStr::new(&no_proxy));
    }
    let system_ca = env.iter().any(|(name, value)| {
        name.to_string_lossy().eq_ignore_ascii_case(SYSTEM_CA_VAR) && !value.is_empty()
    });
    if !system_ca {
        set(&mut env, SYSTEM_CA_VAR, OsStr::new("1"));
    }
    env
}

/// The variable that makes Node trust the OS's certificate store beside its own roots:
/// `NODE_USE_SYSTEM_CA=1`, for every Node Clauseo starts (sign-in's runs here, desktop's
/// launches and readers through [`crate::SYSTEM_CA_VAR`]).
pub(crate) const SYSTEM_CA_VAR: &str = "NODE_USE_SYSTEM_CA";

/// What a run with a proxy never sends through it: this computer, where the OAuth callback
/// listens.
const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// The loopback names, then those of `env`'s `NO_PROXY` (in any case) not already there.
fn with_loopback(env: &BTreeMap<OsString, OsString>) -> String {
    let inherited = env
        .iter()
        .find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case("NO_PROXY"))
        .map(|(_, value)| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut names: Vec<&str> = LOOPBACK.to_vec();
    for name in inherited.split(',').map(str::trim) {
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names.join(",")
}

/// The variable `vendor`'s CLI reads an extra CA file from: `NODE_EXTRA_CA_CERTS` for Claude
/// (Node), `CODEX_CA_CERTIFICATE` for Codex (which loads it beside the system roots; never
/// `SSL_CERT_FILE`, which the tools in an agent's shell take to replace theirs). Desktop's launch
/// env takes the same mapping (`crate::ca_cert_var`).
pub(crate) fn ca_var(vendor: Vendor) -> &'static str {
    match vendor {
        Vendor::Claude => "NODE_EXTRA_CA_CERTS",
        Vendor::Codex => "CODEX_CA_CERTIFICATE",
    }
}

/// The arguments every Codex run but detection adds after its command:
/// `-c cli_auth_credentials_store="file"`. Detection gets none ("without the store override").
pub(crate) fn codex_store_args(kind: RunKind<'_>) -> Vec<OsString> {
    match kind {
        RunKind::Detect { .. } => Vec::new(),
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
