//! A jailed agent's environment (R17 PR-D2a, reviewer M3): built from nothing, so the lawyer's
//! own variables (a shell profile's tokens, a Bedrock or Vertex set-up, an `OpenAI` key) never
//! reach an agent in the jail. Unjailed (tier C) launches keep `launch_spec`'s rule: the
//! inherited environment minus its deny lists.
//!
//! Each source has its own rule ([`sources`]):
//!
//! - **The lawyer's inherited environment:** only the names [`JAILED_ENV`] lets through from it
//!   ([`Rule::Inherited`], [`Rule::InheritedPrefix`], [`Rule::HostElseInherited`]); every other
//!   name is dropped, however harmless it looks.
//! - **Desktop's setup hook** (Clauseo's stored key, the firm plan's and the network route's
//!   variables) and the profile's: they pass, as Clauseo's own sign-in and configuration, except
//!   a credential its selected route does not need (`filters::ROUTES`).
//! - **The agent's settings `env`** (`agents.<id>.env`, made in Clauseo by the lawyer or the
//!   firm): it passes, except a credential name ([`is_credential`]: `filters::CREDENTIALS` and
//!   every secret-shaped name): when jailed, credentials come only from Clauseo's sign-in, the
//!   setup hook and the firm plan. The names it leaves out are [`ignored`], which the settings
//!   view shows.
//!
//! From every source, a name that runs code or moves a tool's config (`filters::BLOCKED`:
//! `BASH_ENV`, `GIT_*`, `LD_PRELOAD` …) and a name that is not ASCII are dropped.
//!
//! Neither the hook nor the settings may set a name the host sets for the jail ([`Rule::Host`],
//! [`Rule::HostElseInherited`]): such a value is dropped. What is dropped is logged as counts,
//! never a name's value.
//!
//! `PATH` is [`path`]: the policy's runtime folders, then [`OS_PATH`]'s fixed system folders.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use clauseo_settings::AgentLaunch;

mod filters;
use filters::{ascii, hook_keeps, is_blocked, is_credential, route_names};

/// Where an allowed name's value comes from in a jailed launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule {
    /// Only the host sets it (the forced variables, the jail's own): the inherited value, the
    /// hook's and the settings' are dropped.
    Host,
    /// The host sets it when it has one; else the lawyer's own passes. The hook's and the
    /// settings' are dropped.
    HostElseInherited,
    /// The lawyer's own passes, as it is.
    Inherited,
    /// Every inherited name that starts with it passes.
    InheritedPrefix,
    /// [`Rule::Inherited`], on Windows only.
    InheritedOnWindows,
}

/// One name a jailed launch lets through, and from where.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Allowed {
    pub(crate) name: &'static str,
    pub(crate) rule: Rule,
}

const fn allow(name: &'static str, rule: Rule) -> Allowed {
    Allowed { name, rule }
}

/// The jailed launch's allow-list: every name its environment may hold, beside what the hook and
/// the settings add. Compared ignoring ASCII case (Windows does), so `https_proxy` is
/// `HTTPS_PROXY`'s entry.
pub(crate) const JAILED_ENV: [Allowed; 41] = [
    // The agent's program search: the policy's runtime folders, then the fixed OS folders
    // (`path`). Never the lawyer's own `PATH`.
    allow("PATH", Rule::Host),
    // Codex's is its state folder on macOS and Linux (`codex::USER_HOME_VAR`), so its start-up
    // files are the state folder's; Claude's stays the lawyer's home, which the jail fences.
    allow("HOME", Rule::HostElseInherited),
    // The session's temp folder, `run/<session>/tmp` (Codex's forced, the Seatbelt launcher's).
    allow("TMPDIR", Rule::Host),
    // Claude's config folder: the matter's state folder (`home::CLAUDE_CONFIG_VAR`).
    allow("CLAUDE_CONFIG_DIR", Rule::Host),
    // Claude's login store: Clauseo's own login home, the sign-in Clauseo set up
    // (`home::CLAUDE_STORE_VAR`).
    allow("CLAUDE_SECURESTORAGE_CONFIG_DIR", Rule::Host),
    // Codex's home: the matter's state folder, holding Clauseo's sign-in link (`codex::HOME_VAR`).
    allow("CODEX_HOME", Rule::Host),
    // Codex's config, written by the host (`codex::CONFIG_VAR`).
    allow("CODEX_CONFIG", Rule::Host),
    // Codex's first mode (`codex::MODE_VAR`).
    allow("INITIAL_AGENT_MODE", Rule::Host),
    // Codex passes the session's MCP servers as Clauseo chose them (`codex::MCP_FILTER_VAR`).
    allow("DISABLE_MCP_CONFIG_FILTERING", Rule::Host),
    // claude.ai's MCP servers off (`loads::CLAUDE_AI_MCP_VAR`).
    allow("ENABLE_CLAUDEAI_MCP_SERVERS", Rule::Host),
    // The session proxy, with its token: the jail's only way out (`ProxyHandle::env`).
    allow("HTTPS_PROXY", Rule::Host),
    // The same proxy, for plain HTTP.
    allow("HTTP_PROXY", Rule::Host),
    // Set empty by the proxy: nothing bypasses it.
    allow("NO_PROXY", Rule::Host),
    // Removed by the proxy; listed so no hook or setting brings one back.
    allow("ALL_PROXY", Rule::Host),
    // The run folder's CA bundle, for Codex (`sandbox::ca::CA_VARS`).
    allow("CODEX_CA_CERTIFICATE", Rule::Host),
    // The same bundle, for OpenSSL tools (git, Python).
    allow("SSL_CERT_FILE", Rule::Host),
    // The same bundle, for Node.
    allow("NODE_EXTRA_CA_CERTS", Rule::Host),
    // The Windows folder, from the OS (`system_root`), never the inherited value: programs and
    // system DLLs need it.
    allow("SystemRoot", Rule::Host),
    // Its older name, which some programs still read.
    allow("windir", Rule::Host),
    // Windows' own system names (round 3b), each from the OS on Windows only ([`system`]),
    // never inherited, hooked or set: a name the OS gives no value for stays unset.
    // cmd, which Node's `child_process` and npm's scripts run:
    // `<real SystemRoot>\System32\cmd.exe`.
    allow("ComSpec", Rule::Host),
    // The Windows folder's drive (`C:`), from the real SystemRoot: some installers' scripts use it.
    allow("SystemDrive", Rule::Host),
    // `FOLDERID_ProgramData`: where tools look for machine-wide settings.
    allow("ProgramData", Rule::Host),
    // `FOLDERID_ProgramFiles`: where programs look for installed tools (Git, Node).
    allow("ProgramFiles", Rule::Host),
    // `FOLDERID_ProgramFilesX86`: the 32-bit programs' folder.
    allow("ProgramFiles(x86)", Rule::Host),
    // `FOLDERID_ProgramFilesX64`: the 64-bit programs' folder, whatever this process's bitness.
    allow("ProgramW6432", Rule::Host),
    // `FOLDERID_ProgramFilesCommon`: shared program files.
    allow("CommonProgramFiles", Rule::Host),
    // `FOLDERID_ProgramFilesCommonX86`: the 32-bit shared program files.
    allow("CommonProgramFiles(x86)", Rule::Host),
    // `FOLDERID_ProgramFilesCommonX64`: the 64-bit shared program files.
    allow("CommonProgramW6432", Rule::Host),
    // `GetNativeSystemInfo`'s architecture (`AMD64`, `ARM64`): Node and Python pick a native
    // build with it.
    allow("PROCESSOR_ARCHITECTURE", Rule::Host),
    // `GetNativeSystemInfo`'s processor count: build tools size their workers by it.
    allow("NUMBER_OF_PROCESSORS", Rule::Host),
    // `Windows_NT`, fixed: scripts tell Windows from Unix by it.
    allow("OS", Rule::Host),
    // The profile names, at the jailed home: the Windows runner sets them to the state folder
    // whatever the spec says (`clauseo_sandbox::windows::runner::env`, `set_by_runner`), so
    // Node's `os.homedir()` and Claude Code read the jail's home. Host-set, so no hook, setting
    // or inherited value reaches the spec. `USERPROFILE` = the state folder.
    allow("USERPROFILE", Rule::Host),
    // `<state>\AppData\Roaming`.
    allow("APPDATA", Rule::Host),
    // `<state>\AppData\Local`.
    allow("LOCALAPPDATA", Rule::Host),
    // The state folder's drive (git and ssh read it before `USERPROFILE`).
    allow("HOMEDRIVE", Rule::Host),
    // The state folder below its drive.
    allow("HOMEPATH", Rule::Host),
    // The locale: the agent's messages and its tools' sorting and encoding.
    allow("LANG", Rule::Inherited),
    // The locale's parts (`LC_ALL`, `LC_CTYPE` …).
    allow("LC_", Rule::InheritedPrefix),
    // Windows' temp folder: the runner sets it to the session's own (`runner::env`); this is the
    // value until then.
    allow("TEMP", Rule::InheritedOnWindows),
    // Its other name, set by the runner too.
    allow("TMP", Rule::InheritedOnWindows),
    // Which extensions run as programs: Node and cmd look a program up with it.
    allow("PATHEXT", Rule::InheritedOnWindows),
];

/// The OS folders a jailed agent's `PATH` holds after its runtime folders, fixed: the shell's
/// own tools (`ls`, `git`, `python3` from the OS). Homebrew's folders (`/opt/homebrew/bin`,
/// `/usr/local/bin`) are left out on purpose: the lawyer can write into them, so what runs there
/// is not the OS's. On macOS `/usr/bin/git` without the Command Line Tools is a shim that asks
/// for their install; the jail denies the service it asks (`com.apple.dt.CommandLineTools.
/// installondemand` is no `mach-lookup` the base profile allows), so `git` fails plainly.
#[cfg(not(windows))]
pub(crate) const OS_PATH: [&str; 4] = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"];
/// The OS folders a jailed agent's `PATH` holds after its runtime folders, under the Windows
/// folder the OS names ([`system_root`]): cmd and the system tools, WMI's, and PowerShell.
#[cfg(windows)]
pub(crate) const OS_PATH: [&str; 4] = [
    "System32",
    "",
    "System32\\Wbem",
    "System32\\WindowsPowerShell\\v1.0",
];

/// `name`'s entry in [`JAILED_ENV`], ignoring ASCII case; `None` when it is not on the list.
pub(crate) fn allowed(name: &OsStr) -> Option<Rule> {
    let bytes = name.as_encoded_bytes();
    JAILED_ENV
        .iter()
        .find(|entry| match entry.rule {
            Rule::InheritedPrefix => starts_with(bytes, entry.name),
            Rule::Host | Rule::HostElseInherited | Rule::Inherited | Rule::InheritedOnWindows => {
                bytes.eq_ignore_ascii_case(entry.name.as_bytes())
            }
        })
        .map(|entry| entry.rule)
}

/// Whether the host alone sets `name` in a jailed launch: no hook or setting may.
fn host_sets(name: &OsStr) -> bool {
    matches!(allowed(name), Some(Rule::Host | Rule::HostElseInherited))
}

/// Whether an inherited `name` reaches a jailed agent.
fn passes_inherited(name: &OsStr) -> bool {
    match allowed(name) {
        Some(Rule::HostElseInherited | Rule::Inherited | Rule::InheritedPrefix) => true,
        Some(Rule::InheritedOnWindows) => cfg!(windows),
        Some(Rule::Host) | None => false,
    }
}

fn starts_with(bytes: &[u8], prefix: &str) -> bool {
    bytes
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
}

/// The credential names in an agent's settings `env` a jailed launch leaves out
/// ([`is_credential`]), sorted: the settings view's note, whose words say credential. The
/// blocked and non-ASCII names it also drops are not listed there (`pr-d2a.md`'s follow-up).
pub(crate) fn ignored(env: &BTreeMap<String, String>) -> Vec<String> {
    env.keys()
        .filter(|name| is_credential(OsStr::new(name)))
        .cloned()
        .collect()
}

/// A jailed launch's sources for `launch_spec`, each by its rule (module header).
pub(crate) struct Sources {
    /// The inherited variables [`JAILED_ENV`] lets through.
    pub(crate) inherited: Vec<(OsString, OsString)>,
    /// The agent's settings, with no credential name and no name the host sets in its `env`.
    pub(crate) settings: AgentLaunch,
    /// The hook's variables, with no name the host sets.
    pub(crate) hook: BTreeMap<OsString, OsString>,
}

/// The jailed launch's sources from the `inherited` environment, the agent's `settings` and
/// the profile `hook`'s variables (before the host's own switch goes in, `loads::launch_env`).
/// Logs what each source lost as a count.
pub(crate) fn sources(
    inherited: Vec<(OsString, OsString)>,
    settings: &AgentLaunch,
    mut hook: BTreeMap<OsString, OsString>,
) -> Sources {
    let all = inherited.len();
    let inherited: Vec<_> = inherited
        .into_iter()
        .filter(|(name, _)| passes_inherited(name) && ascii(name) && !is_blocked(name))
        .collect();
    let inherited_dropped = all - inherited.len();
    let mut settings = settings.clone();
    let before = settings.env.len() + hook.len();
    settings
        .env
        .retain(|name, _| ascii(OsStr::new(name)) && !is_blocked(OsStr::new(name)));
    hook.retain(|name, _| ascii(name) && !is_blocked(name));
    let blocked = before - settings.env.len() - hook.len();
    let before = settings.env.len() + hook.len();
    settings
        .env
        .retain(|name, _| !is_credential(OsStr::new(name)));
    let routed = route_names(&hook);
    hook.retain(|name, _| hook_keeps(name, &routed));
    let credentials = before - settings.env.len() - hook.len();
    let before = settings.env.len() + hook.len();
    settings.env.retain(|name, _| !host_sets(OsStr::new(name)));
    hook.retain(|name, _| !host_sets(name));
    let overrides = before - settings.env.len() - hook.len();
    tracing::debug!(
        inherited_dropped,
        blocked,
        credentials,
        overrides,
        "the jailed agent's environment left variables out"
    );
    Sources {
        inherited,
        settings,
        hook,
    }
}

/// The jailed agent's `PATH`: `runtime` (the policy's runtime folders, in order), then
/// [`OS_PATH`]'s folders; `None` when a folder holds the separator, which `runtime`'s canonical
/// folders never do.
pub(crate) fn path(runtime: &[PathBuf]) -> Option<OsString> {
    let folders = runtime.iter().cloned().chain(os_folders());
    std::env::join_paths(folders).ok()
}

/// [`OS_PATH`] as folders: as written on macOS and Linux, under [`system_root`] on Windows (none
/// when the OS names no Windows folder).
fn os_folders() -> Vec<PathBuf> {
    #[cfg(not(windows))]
    {
        OS_PATH.iter().map(PathBuf::from).collect()
    }
    #[cfg(windows)]
    {
        system_root()
            .map(|root| {
                OS_PATH
                    .iter()
                    .map(|rel| {
                        if rel.is_empty() {
                            root.clone()
                        } else {
                            root.join(rel)
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// The Windows folder as the OS names it (`FOLDERID_Windows`), never `%SystemRoot%` from the
/// inherited environment; `None` off Windows.
pub(crate) fn system_root() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use clauseo_settings::{KnownFolder, PlatformDirs as _, SystemDirs};
        SystemDirs.known_folder(KnownFolder::Windows)
    }
    #[cfg(not(windows))]
    None
}

/// The variables the host sets for the jail besides the proxy's and the CA bundle's: `PATH`
/// ([`path`]) from the policy's `runtime`, and on Windows `SystemRoot` and `windir`
/// ([`system_root`]) and Windows' system names ([`system::native`]; the ones the OS gives no
/// value for are counted in the log, never inherited).
pub(crate) fn host_vars(runtime: &[PathBuf]) -> Vec<(OsString, Option<OsString>)> {
    let mut vars: Vec<(OsString, Option<OsString>)> = path(runtime)
        .map(|path| (OsString::from("PATH"), Some(path)))
        .into_iter()
        .collect();
    if let Some(root) = system_root() {
        for name in ["SystemRoot", "windir"] {
            vars.push((OsString::from(name), Some(root.as_os_str().to_owned())));
        }
    }
    #[cfg(windows)]
    {
        let (system, unset) = system::native();
        if unset > 0 {
            tracing::debug!(
                unset,
                "Windows gave no value for some of the jail's system names"
            );
        }
        vars.extend(system.into_iter().map(|(name, value)| (name, Some(value))));
    }
    vars
}

// Read only on Windows; its rules are tested on every OS.
#[cfg(any(windows, test))]
pub(crate) mod system;
#[cfg(test)]
mod tests;
