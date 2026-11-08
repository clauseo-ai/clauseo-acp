//! The agent's environment under tier B ([`agent_env`]), made by the outer runner from the
//! launch spec's: what Windows' own profile variables would say for a user whose profile is the
//! state folder, git over OpenSSL, the CA bundle, and nothing that points into the lawyer's
//! profile.
//!
//! - **Set**, whatever the spec said: `USERPROFILE`, `HOME`, `HOMEDRIVE` + `HOMEPATH` (git and
//!   ssh look there before `USERPROFILE`) at `<state>`; `APPDATA` at `<state>\AppData\Roaming`,
//!   `LOCALAPPDATA` at `<state>\AppData\Local`; `TEMP` and `TMP` at the session's temp folder
//!   (`Policy::session_tmpdir`), else `<state>\tmp` (`Policy::tmpdir`'s fallback).
//! - **git over OpenSSL** (§3: Schannel fails under any restricting-SID token,
//!   `s15-spikes-traverse.md` (b)): `GIT_CONFIG_COUNT=1`, `GIT_CONFIG_KEY_0=http.sslBackend`,
//!   `GIT_CONFIG_VALUE_0=openssl`; any other `GIT_CONFIG_KEY_n`/`VALUE_n` the spec had goes.
//! - **Git Bash** (0049, "The Git tree"; [`GitEnv`], made by `git_bash.rs`): with the run's
//!   checked tree, `CLAUDE_CODE_GIT_BASH_PATH` at `<tree>\bin\bash.exe` and `<tree>\cmd` first
//!   in `PATH`, so the agent's `git` is the tree's and finds the tree's `sh.exe`. With none,
//!   `CLAUDE_CODE_GIT_BASH_PATH` at a checked stub (`git_bash/off.rs`) and the reason in its
//!   `OFF_VAR`, so Claude Code never looks for the lawyer's Git Bash; there is no third case.
//!   Either way the spec's values of both go, and so does each `PATH` entry `git_bash/path_ids`
//!   judged (by file ID) to be in the lawyer's Git, behind a link or on the network.
//! - **The lawyer's Git and shell settings go** ([`lawyer_only`]): every `GIT_*` (the runner
//!   sets its own `GIT_CONFIG_*`), `MSYSTEM*`, `MINGW_*`, `MSYS*`, `SSH_ASKPASS*`,
//!   `SSH_AUTH_SOCK`, `EXEPATH`, `ORIGINAL_PATH`/`TEMP`/`TMP`, `CYGWIN`, `CHERE_INVOKING`,
//!   `PLINK_PROTOCOL`, `SHELL`, `BASH_ENV`, `ENV`, `INPUTRC`, `GNUPGHOME` and the `XDG_*`
//!   folders: what Git, its shell or ssh would read in place of the agent's own.
//! - **CA bundle** (`s15-spikes-traverse.md` (d): codex's rustls finds no roots under the token):
//!   `CODEX_CA_CERTIFICATE` and `SSL_CERT_FILE` at the bundle, when the launch names one.
//!   `SSL_CERT_FILE` makes every OpenSSL tool trust exactly the machine's roots, as intended.
//! - **Out:** `NODE_OPTIONS` and `NODE_PATH` (`clauseo_process::NODE_ENV`); any variable whose
//!   value, or any `;`-separated part of it, lies in the lawyer's profile and in none of the
//!   paths the launch grants (the state and data folders sit in the lawyer's `LocalAppData`); in
//!   `PATH` and `PSModulePath`, each entry in the lawyer's profile unless it lies in one of the
//!   launch's runtime paths; names starting with `=` (cmd's per-drive folders).
//!
//! The result is sorted by name without case, one entry per name, as `CreateProcess` wants an
//! environment block.

use std::collections::BTreeMap;

use super::paths::{WinPath, text_within};
use super::stub::OFF_VAR;
use super::{LaunchError, LaunchMessage};

/// The git variables that turn on its OpenSSL backend (§3).
pub const GIT_OPENSSL: [(&str, &str); 3] = [
    ("GIT_CONFIG_COUNT", "1"),
    ("GIT_CONFIG_KEY_0", "http.sslBackend"),
    ("GIT_CONFIG_VALUE_0", "openssl"),
];
/// The variables codex (`CODEX_CA_CERTIFICATE`, read first) and OpenSSL tools read the CA
/// bundle from.
pub const CA_VARS: [&str; 2] = ["CODEX_CA_CERTIFICATE", "SSL_CERT_FILE"];
/// Where Claude Code finds Git Bash.
pub const GIT_BASH_VAR: &str = "CLAUDE_CODE_GIT_BASH_PATH";
/// The variables whose entries are filtered one by one rather than kept or dropped whole.
const LISTS: [&str; 2] = ["PATH", "PSModulePath"];

/// The firm CA files to merge into the launch's CA bundle: `firm_ca`'s, each once and none the
/// bundle itself; none without a bundle. The spec's own `SSL_CERT_FILE` is never one: it may be
/// the lawyer's ambient value, not the firm's setting, and the bundle's variable replaces it.
pub fn firm_ca_files(message: &LaunchMessage) -> Vec<String> {
    let Some(bundle) = message.ca_bundle.as_deref().and_then(WinPath::parse) else {
        return Vec::new();
    };
    let mut files: Vec<String> = Vec::new();
    let mut seen: Vec<WinPath> = vec![bundle];
    for file in &message.firm_ca {
        let Some(path) = WinPath::parse(file) else {
            continue;
        };
        if !seen
            .iter()
            .any(|known| path.within(known) && known.within(&path))
        {
            seen.push(path.clone());
            files.push(path.display());
        }
    }
    files
}

/// The state folder's places the environment names, which the inner runner makes before the
/// agent starts: `AppData\Roaming`, `AppData\Local`, and `tmp` when there is no session temp
/// folder.
pub fn profile_folders(message: &LaunchMessage) -> Result<Vec<String>, LaunchError> {
    let places = Places::of(message)?;
    let mut folders = vec![places.roaming.display(), places.local.display()];
    if message.tmpdir.is_none() {
        folders.push(places.temp.display());
    }
    Ok(folders)
}

/// The Git Bash `CLAUDE_CODE_GIT_BASH_PATH` names: always one, never left to Claude Code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bash {
    /// The run's checked Git tree.
    Tree(WinPath),
    /// Git Bash is off: a checked stub (`git_bash/off.rs`), and why, for the stub to say.
    Off {
        /// The stub.
        stub: WinPath,
        /// Why Git Bash is off.
        reason: String,
    },
}

/// What the agent's environment says of Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitEnv {
    /// Its Git Bash.
    pub bash: Bash,
    /// The lawyer's `PATH` entries, as written, that the agent's leaves out
    /// (`git_bash::path_ids`).
    pub dropped: Vec<String>,
}

#[cfg(test)]
impl Default for GitEnv {
    /// Git Bash off with the machine's stub, no `PATH` entry dropped: the tests' plain case.
    fn default() -> GitEnv {
        GitEnv {
            bash: Bash::Off {
                stub: WinPath::parse(r"C:\Program Files\Clauseo\Sandbox\git\_off\bash.exe")
                    .unwrap(),
                reason: "off".to_owned(),
            },
            dropped: Vec::new(),
        }
    }
}

/// The agent's environment from `message`, for a lawyer whose profile folder is `profile`,
/// with `git`'s Git Bash.
pub fn agent_env(
    message: &LaunchMessage,
    profile: &WinPath,
    git: &GitEnv,
) -> Result<Vec<(String, String)>, LaunchError> {
    let places = Places::of(message)?;
    let runtime: Vec<WinPath> = message
        .runtime
        .iter()
        .filter_map(|p| WinPath::parse(p))
        .collect();
    let mut granted: Vec<WinPath> = message
        .granted
        .iter()
        .filter_map(|p| WinPath::parse(p))
        .collect();
    granted.push(places.state.clone());
    granted.push(places.temp.clone());
    granted.extend(message.ca_bundle.as_deref().and_then(WinPath::parse));
    let set = set_by_runner(message, &places);
    // Keyed by the upper-cased name, so `Path` and `PATH` are one variable; the last wins.
    let mut out: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (name, value) in &message.env {
        if name.starts_with('=')
            || clauseo_process::is_node_env(std::ffi::OsStr::new(name))
            || overridden(name, message)
            || lawyer_only(name)
        {
            continue;
        }
        let kept = if name.eq_ignore_ascii_case("PATH") {
            filter_list(value, profile, &runtime).and_then(|value| without(&value, &git.dropped))
        } else if LISTS.iter().any(|list| list.eq_ignore_ascii_case(name)) {
            filter_list(value, profile, &runtime)
        } else if lies_outside(value, profile, &granted) {
            None
        } else {
            Some(value.clone())
        };
        if let Some(value) = kept {
            out.insert(name.to_uppercase(), (name.clone(), value));
        }
    }
    for (name, value) in set {
        out.insert(name.to_uppercase(), (name, value));
    }
    let mut put = |name: &str, value: String| {
        out.insert(name.to_owned(), (name.to_owned(), value));
    };
    match &git.bash {
        Bash::Off { stub, reason } => {
            put(GIT_BASH_VAR, stub.display());
            put(OFF_VAR, reason.clone());
        }
        Bash::Tree(tree) => {
            put(GIT_BASH_VAR, tree.join("bin").join("bash.exe").display());
            let cmd = tree.join("cmd").display();
            let path = match out.remove("PATH") {
                Some((name, value)) => (name, format!("{cmd};{value}")),
                None => ("PATH".to_owned(), cmd),
            };
            out.insert("PATH".to_owned(), path);
        }
    }
    Ok(out.into_values().collect())
}

/// The state folder's places, parsed.
struct Places {
    state: WinPath,
    roaming: WinPath,
    local: WinPath,
    temp: WinPath,
}

impl Places {
    fn of(message: &LaunchMessage) -> Result<Places, LaunchError> {
        let state = WinPath::parse(&message.state).ok_or(LaunchError::Field {
            field: "state",
            rule: "is not a full local path",
        })?;
        let temp = match &message.tmpdir {
            Some(tmpdir) => WinPath::parse(tmpdir).ok_or(LaunchError::Field {
                field: "tmpdir",
                rule: "is not a full local path",
            })?,
            None => state.join("tmp"),
        };
        Ok(Places {
            roaming: state.join("AppData").join("Roaming"),
            local: state.join("AppData").join("Local"),
            temp,
            state,
        })
    }
}

/// What the runner sets, in a fixed order.
fn set_by_runner(message: &LaunchMessage, places: &Places) -> Vec<(String, String)> {
    let state = places.state.display();
    let mut set = vec![
        ("USERPROFILE".to_owned(), state.clone()),
        ("HOME".to_owned(), state),
        (
            "HOMEDRIVE".to_owned(),
            places.state.drive().unwrap_or_default().to_owned(),
        ),
        ("HOMEPATH".to_owned(), places.state.below_drive()),
        ("APPDATA".to_owned(), places.roaming.display()),
        ("LOCALAPPDATA".to_owned(), places.local.display()),
        ("TEMP".to_owned(), places.temp.display()),
        ("TMP".to_owned(), places.temp.display()),
    ];
    set.extend(
        GIT_OPENSSL
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );
    if let Some(bundle) = message.ca_bundle.as_deref().and_then(WinPath::parse) {
        set.extend(
            CA_VARS
                .iter()
                .map(|name| ((*name).to_owned(), bundle.display())),
        );
    }
    set
}

/// Whether the runner sets `name` itself, so the spec's value goes.
fn overridden(name: &str, message: &LaunchMessage) -> bool {
    const SET: [&str; 8] = [
        "USERPROFILE",
        "HOME",
        "HOMEDRIVE",
        "HOMEPATH",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
    ];
    let upper = name.to_ascii_uppercase();
    SET.contains(&upper.as_str())
        || upper == GIT_BASH_VAR
        || upper == OFF_VAR
        || upper == "GIT_CONFIG_COUNT"
        || upper
            .strip_prefix("GIT_CONFIG_KEY_")
            .is_some_and(all_digits)
        || upper
            .strip_prefix("GIT_CONFIG_VALUE_")
            .is_some_and(all_digits)
        || (message.ca_bundle.is_some() && CA_VARS.contains(&upper.as_str()))
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// Whether some `;`-separated part of `value` lies in `profile` and in none of `granted`.
fn lies_outside(value: &str, profile: &WinPath, granted: &[WinPath]) -> bool {
    value
        .split(';')
        .any(|part| text_within(part, profile) && !granted.iter().any(|g| text_within(part, g)))
}

/// `value` (a `;` list) without its entries in `dropped` (as written); `None` when nothing
/// is left.
fn without(value: &str, dropped: &[String]) -> Option<String> {
    let kept: Vec<&str> = value
        .split(';')
        .filter(|entry| !dropped.iter().any(|gone| gone == entry))
        .collect();
    (!kept.is_empty()).then(|| kept.join(";"))
}

/// Whether `name` is one of the lawyer's Git, shell or ssh settings the agent never gets
/// (module docs); the runner sets its own `GIT_CONFIG_*` after.
pub fn lawyer_only(name: &str) -> bool {
    const PREFIXES: [&str; 6] = ["GIT_", "MSYSTEM", "MINGW_", "MSYS", "SSH_ASKPASS", "XDG_"];
    const NAMES: [&str; 13] = [
        "SSH_AUTH_SOCK",
        "EXEPATH",
        "ORIGINAL_PATH",
        "ORIGINAL_TEMP",
        "ORIGINAL_TMP",
        "CYGWIN",
        "CHERE_INVOKING",
        "PLINK_PROTOCOL",
        "SHELL",
        "BASH_ENV",
        "ENV",
        "INPUTRC",
        "GNUPGHOME",
    ];
    let upper = name.to_ascii_uppercase();
    PREFIXES.iter().any(|prefix| upper.starts_with(prefix)) || NAMES.contains(&upper.as_str())
}

/// `value` (a `;` list) without its entries that lie in `profile` and in none of `runtime`;
/// `None` when nothing is left.
fn filter_list(value: &str, profile: &WinPath, runtime: &[WinPath]) -> Option<String> {
    let kept: Vec<&str> = value
        .split(';')
        .filter(|entry| !entry.trim().is_empty())
        .filter(|entry| {
            !text_within(entry, profile) || runtime.iter().any(|r| text_within(entry, r))
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join(";"))
}
