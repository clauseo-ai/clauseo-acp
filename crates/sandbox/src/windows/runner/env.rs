//! The agent's environment under tier B ([`agent_env`]), built by the outer runner, never the
//! lawyer's filtered: only these reach the agent.
//!
//! - **The Windows base set**, copied from the lawyer's environment ([`BASE`], [`BASE_PREFIXES`]):
//!   `SystemRoot`, `windir`, `ComSpec`, `PATHEXT`, `OS`, `PROCESSOR_*`,
//!   `NUMBER_OF_PROCESSORS`, `SystemDrive`, `ProgramFiles`, `ProgramFiles(x86)`,
//!   `ProgramW6432`, `CommonProgramFiles*`, `ProgramData`, `ALLUSERSPROFILE`, `PUBLIC`,
//!   `DriverData`.
//! - **What the host set for the agent** (`LaunchMessage::agent_env`, never the lawyer's
//!   inherited variables): each kept unless it is one of ours, `NODE_OPTIONS`/`NODE_PATH`
//!   (`clauseo_process::NODE_ENV`), a per-drive folder (`=C:`), or a value with a `;`-separated
//!   part in the lawyer's profile and in none of the paths the launch grants; `PSModulePath`
//!   loses each such entry alone.
//! - **Ours**, whatever else said: `USERPROFILE`, `HOME`, `HOMEDRIVE` + `HOMEPATH` at `<state>`;
//!   `APPDATA` at `<state>\AppData\Roaming`, `LOCALAPPDATA` at `<state>\AppData\Local`; `TEMP`
//!   and `TMP` at the session's temp folder (`Policy::session_tmpdir`), else `<state>\tmp`.
//!   git over OpenSSL (§3: Schannel fails under any restricting-SID token,
//!   `s15-spikes-traverse.md` (b)): `GIT_CONFIG_COUNT=1`, `GIT_CONFIG_KEY_0=http.sslBackend`,
//!   `GIT_CONFIG_VALUE_0=openssl`. The CA bundle (`s15-spikes-traverse.md` (d)):
//!   `CODEX_CA_CERTIFICATE` and `SSL_CERT_FILE` at it, when the launch names one.
//! - **Git Bash** (0049, "The Git tree"; [`GitEnv`], made by `git_bash.rs`): with the run's
//!   checked tree, `CLAUDE_CODE_GIT_BASH_PATH` at `<tree>\bin\bash.exe`; with none, at a checked
//!   stub (`git_bash/off.rs`) and the reason in its `OFF_VAR`, so Claude Code never looks for the
//!   lawyer's Git Bash. There is no third case.
//! - **`PATH`, ours alone** ([`built_path`]): `<tree>\cmd` with a tree, then the launch's
//!   runtime paths, then Windows' own folders under `SystemRoot`. No entry of the lawyer's or the
//!   host's `PATH` is kept, so no Git of the lawyer's is on it.
//!
//! The lawyer's `EDITOR`, `PAGER`, proxies, `PERL5OPT`, `GIT_*` and the rest never reach the
//! agent. The result is sorted by name without case, one entry per name, as `CreateProcess` wants
//! an environment block.

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
/// The Windows base set copied from the lawyer's environment, by name without case.
pub const BASE: [&str; 15] = [
    "SystemRoot",
    "windir",
    "ComSpec",
    "PATHEXT",
    "OS",
    "NUMBER_OF_PROCESSORS",
    "SystemDrive",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "ProgramData",
    "ALLUSERSPROFILE",
    "PUBLIC",
    "DriverData",
    "CommonProgramW6432",
];
/// The base set's name prefixes, without case.
pub const BASE_PREFIXES: [&str; 2] = ["PROCESSOR_", "CommonProgramFiles"];
/// Windows' own `PATH` folders, under `SystemRoot`.
const SYSTEM_PATH: [&str; 5] = [
    r"System32",
    "",
    r"System32\Wbem",
    r"System32\WindowsPowerShell\v1.0",
    r"System32\OpenSSH",
];

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
}

#[cfg(test)]
impl Default for GitEnv {
    /// Git Bash off with the machine's stub: the tests' plain case.
    fn default() -> GitEnv {
        GitEnv {
            bash: Bash::Off {
                stub: WinPath::parse(r"C:\Program Files\Clauseo\Sandbox\git\_off\bash.exe")
                    .unwrap(),
                reason: "off".to_owned(),
            },
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
        if is_base(name) {
            out.insert(name.to_uppercase(), (name.clone(), value.clone()));
        }
    }
    for (name, value) in &message.agent_env {
        if name.starts_with('=')
            || name.eq_ignore_ascii_case("PATH")
            || clauseo_process::is_node_env(std::ffi::OsStr::new(name))
            || overridden(name, message)
        {
            continue;
        }
        let kept = if name.eq_ignore_ascii_case("PSModulePath") {
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
    let system_root = out.get("SYSTEMROOT").map(|(_, value)| value.clone());
    let tree = match &git.bash {
        Bash::Tree(tree) => Some(tree),
        Bash::Off { .. } => None,
    };
    let path = built_path(tree, &runtime, system_root.as_deref());
    if !path.is_empty() {
        out.insert("PATH".to_owned(), ("PATH".to_owned(), path));
    }
    let mut put = |name: &str, value: String| {
        out.insert(name.to_owned(), (name.to_owned(), value));
    };
    match &git.bash {
        Bash::Off { stub, reason } => {
            put(GIT_BASH_VAR, stub.display());
            put(OFF_VAR, reason.clone());
        }
        Bash::Tree(tree) => put(GIT_BASH_VAR, tree.join("bin").join("bash.exe").display()),
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

/// Whether `name` is in the Windows base set ([`BASE`], [`BASE_PREFIXES`]), without case.
pub fn is_base(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    BASE.iter().any(|base| base.eq_ignore_ascii_case(name))
        || BASE_PREFIXES
            .iter()
            .any(|prefix| upper.starts_with(&prefix.to_ascii_uppercase()))
}

/// The agent's `PATH`: `<tree>\cmd` with a tree, the launch's runtime paths, then Windows' own
/// folders under `system_root` when it is a full local path.
pub fn built_path(
    tree: Option<&WinPath>,
    runtime: &[WinPath],
    system_root: Option<&str>,
) -> String {
    let mut entries: Vec<String> = tree
        .map(|tree| tree.join("cmd").display())
        .into_iter()
        .collect();
    entries.extend(runtime.iter().map(WinPath::display));
    if let Some(root) = system_root.and_then(WinPath::parse) {
        entries.extend(SYSTEM_PATH.iter().map(|rel| {
            rel.split('\\')
                .filter(|part| !part.is_empty())
                .fold(root.clone(), |path, part| path.join(part))
                .display()
        }));
    }
    entries.join(";")
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
