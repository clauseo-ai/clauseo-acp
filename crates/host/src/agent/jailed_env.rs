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
//!   variables) and the profile's: they pass, as Clauseo's own sign-in and configuration.
//! - **The agent's settings `env`** (`agents.<id>.env`, made in Clauseo by the lawyer or the
//!   firm): it passes, except a credential name ([`CREDENTIALS`]): when jailed, credentials come
//!   only from Clauseo's sign-in, the setup hook and the firm plan. The names it leaves out are
//!   [`ignored`], which the settings view shows.
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
pub(crate) const JAILED_ENV: [Allowed; 24] = [
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

/// One entry of [`CREDENTIALS`]: a whole name, or every name with a prefix.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Credential {
    Name(&'static str),
    Prefix(&'static str),
}

/// The names that carry or choose an agent's credentials: no jailed agent takes them from the
/// lawyer's environment (none passes [`JAILED_ENV`]) nor from its settings `env`. Clauseo's own
/// sign-in, the setup hook's stored key and the firm plan set them instead.
pub(crate) const CREDENTIALS: [Credential; 27] = [
    // Anthropic's keys, tokens and endpoint.
    Credential::Name("ANTHROPIC_API_KEY"),
    Credential::Name("ANTHROPIC_AUTH_TOKEN"),
    Credential::Name("CLAUDE_CODE_OAUTH_TOKEN"),
    Credential::Name("ANTHROPIC_BASE_URL"),
    Credential::Name("ANTHROPIC_CUSTOM_HEADERS"),
    // Bedrock.
    Credential::Name("CLAUDE_CODE_USE_BEDROCK"),
    Credential::Name("CLAUDE_CODE_SKIP_BEDROCK_AUTH"),
    Credential::Name("ANTHROPIC_BEDROCK_BASE_URL"),
    Credential::Prefix("AWS_"),
    // Vertex.
    Credential::Name("CLAUDE_CODE_USE_VERTEX"),
    Credential::Name("CLAUDE_CODE_SKIP_VERTEX_AUTH"),
    Credential::Name("ANTHROPIC_VERTEX_PROJECT_ID"),
    Credential::Name("ANTHROPIC_VERTEX_BASE_URL"),
    Credential::Name("GOOGLE_APPLICATION_CREDENTIALS"),
    Credential::Name("GOOGLE_CLOUD_PROJECT"),
    Credential::Name("CLOUD_ML_REGION"),
    Credential::Prefix("VERTEX_REGION_"),
    // Microsoft Foundry.
    Credential::Name("CLAUDE_CODE_USE_FOUNDRY"),
    Credential::Name("CLAUDE_CODE_SKIP_FOUNDRY_AUTH"),
    Credential::Prefix("ANTHROPIC_FOUNDRY_"),
    // OpenAI and Codex.
    Credential::Name("OPENAI_API_KEY"),
    Credential::Name("OPENAI_BASE_URL"),
    Credential::Name("OPENAI_ORG_ID"),
    Credential::Name("OPENAI_ORGANIZATION"),
    Credential::Name("OPENAI_PROJECT"),
    Credential::Name("CODEX_API_KEY"),
    // Azure OpenAI.
    Credential::Prefix("AZURE_OPENAI_"),
];

/// Whether `name` is in [`CREDENTIALS`], ignoring ASCII case.
pub(crate) fn is_credential(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    CREDENTIALS.iter().any(|credential| match credential {
        Credential::Name(known) => bytes.eq_ignore_ascii_case(known.as_bytes()),
        Credential::Prefix(prefix) => starts_with(bytes, prefix),
    })
}

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

/// The names in an agent's settings `env` a jailed launch leaves out ([`CREDENTIALS`]), sorted:
/// the settings view's note.
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
        .filter(|(name, _)| passes_inherited(name))
        .collect();
    let mut settings = settings.clone();
    let before = settings.env.len();
    settings
        .env
        .retain(|name, _| !is_credential(OsStr::new(name)));
    let credentials = before - settings.env.len();
    let before = settings.env.len() + hook.len();
    settings.env.retain(|name, _| !host_sets(OsStr::new(name)));
    hook.retain(|name, _| !host_sets(name));
    let overrides = before - settings.env.len() - hook.len();
    let inherited_dropped = all - inherited.len();
    tracing::debug!(
        inherited_dropped,
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
/// ([`system_root`]).
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
    vars
}

#[cfg(test)]
mod tests;
