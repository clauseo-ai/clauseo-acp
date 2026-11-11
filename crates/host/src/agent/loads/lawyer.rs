//! The lawyer's own MCP servers in one session (Settings › Tools): which go, with what
//! environment, and the keychain values they need, read on the launch-prep thread.
//!
//! - Only under [`McpControl::Clauseo`], and never in the Safe workspace (the orchestrator's
//!   call, 2026-11-11: a lawyer's server is a child of the agent, and the fence is not shown to
//!   cover it). The built-ins (`browser`, `case-law`) are not decided here.
//! - Only the enabled local servers go, in name order, as stdio servers. A remote one is kept
//!   and shown but not sent: this version's ACP client has no HTTP server (see [`servers`]).
//! - A server's `command` is what the lawyer typed, a bare name (`npx`) or an absolute path; the
//!   launch-prep thread resolves it ([`Resolve`], the same search a save makes) and the session
//!   gets the resolved path. A command not found leaves its server out, with its name logged.
//! - A variable whose value is `{"keychain": true}` takes the keychain item
//!   `clauseo_settings::keychain_account(server, Env, name)`. [`wanted`] lists those accounts on
//!   the actor, [`read`] reads them on the launch-prep thread (each read is a blocking keychain
//!   call), and the values ride the run's folders ([`McpSecrets`]) to the session calls. A
//!   server with a value missing or unreadable (not UTF-8, holding NUL, or a keychain error) is
//!   left out of the session, with a log line naming the server and the variable, never a value.
//!   A plain value (a firm's policy, a hand-edited file) goes as it is.
//! - `PATH`: unless the server's own variables set it, the server's `PATH` is its resolved
//!   command's folder, then the `PATH` Clauseo was started with. Each agent starts a stdio
//!   server with its own `PATH` and the entry's variables over it; on macOS an app started from
//!   the Dock or Finder has launchd's `/usr/bin:/bin:/usr/sbin:/sbin`, so `npx` (a
//!   `#!/usr/bin/env node` script) in `/opt/homebrew/bin` or an nvm folder could not find the
//!   `node` beside it.
//! - On Windows the entry also gets the variables Windows programs need that the host has
//!   (`case_law::windows_env`: `SystemRoot`, `windir`, `TEMP`, `TMP`, `PATHEXT`), each unless
//!   the server sets it, as the case-law entry does.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_acp::McpServer;
use clauseo_connectors::SecretStore;
use clauseo_settings::{Effective, McpTransport, McpValue, McpValueKind, keychain_account};
use zeroize::Zeroizing;

use super::McpControl;

/// Finds a server's program: a bare name on this computer's search paths, or an absolute path
/// that is a file; `None` when there is none. File system calls: off the actor only.
pub(crate) type Resolve = crate::config::mcp::McpResolver;

/// What the launch-prep thread found for one session's lawyer servers: the keychain values by
/// account, and each command as typed with the program it resolved to. `Debug` prints counts
/// only.
#[derive(Default)]
pub(crate) struct McpSecrets {
    values: BTreeMap<String, Zeroizing<String>>,
    unreadable: BTreeSet<String>,
    commands: BTreeMap<String, PathBuf>,
}

impl fmt::Debug for McpSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpSecrets")
            .field("values", &self.values.len())
            .field("unreadable", &self.unreadable.len())
            .field("commands", &self.commands.len())
            .finish()
    }
}

/// What the launch-prep thread reads and resolves for a session ([`wanted`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Wanted {
    /// The keychain accounts.
    pub(crate) accounts: Vec<String>,
    /// The commands, as typed.
    pub(crate) commands: Vec<String>,
}

impl McpSecrets {
    /// The value of `account`, when it was read and is usable.
    fn get(&self, account: &str) -> Option<&str> {
        self.values.get(account).map(|value| value.as_str())
    }

    /// Values and resolved commands in memory, as `read` would make them (tests).
    #[cfg(test)]
    pub(crate) fn of(values: &[(&str, &str)], commands: &[(&str, &str)]) -> Self {
        Self {
            values: values
                .iter()
                .map(|(account, value)| {
                    ((*account).to_owned(), Zeroizing::new((*value).to_owned()))
                })
                .collect(),
            unreadable: BTreeSet::new(),
            commands: commands
                .iter()
                .map(|(typed, path)| ((*typed).to_owned(), PathBuf::from(path)))
                .collect(),
        }
    }
}

/// What decides a session's lawyer servers besides the settings.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Lawyer {
    /// Who decides MCP on this computer.
    pub(crate) control: McpControl,
    /// Whether the session runs in the Safe workspace: then no lawyer server goes.
    pub(crate) safe_workspace: bool,
}

/// Whether `lawyer` lets any of the lawyer's servers go.
fn allowed(lawyer: Lawyer) -> bool {
    lawyer.control == McpControl::Clauseo && !lawyer.safe_workspace
}

/// What the session's servers need: every `{"keychain": true}` variable's account and every
/// command of each enabled local server; nothing in the Safe workspace. On the actor; no
/// keychain or file system call. The firm's `managed-mcp.json` is checked at the session call
/// ([`servers`]), not here.
pub(crate) fn wanted(effective: &Effective, safe_workspace: bool) -> Wanted {
    let mut wanted = Wanted::default();
    if safe_workspace {
        return wanted;
    }
    for (name, server) in effective.enabled_mcp_servers() {
        let McpTransport::Local { command, env, .. } = &server.transport else {
            continue;
        };
        wanted.commands.push(command.clone());
        wanted.accounts.extend(
            env.iter()
                .filter(|(_, value)| matches!(value, McpValue::Keychain(_)))
                .map(|(var, _)| keychain_account(name, McpValueKind::Env, var)),
        );
    }
    wanted
}

/// What a launch's prep thread reads and resolves for the session's lawyer servers, gathered on
/// the actor ([`Reads::of`]) and read off it ([`Reads::read`]).
pub(crate) struct Reads {
    store: Option<Arc<dyn SecretStore>>,
    resolve: Resolve,
    wanted: Wanted,
}

impl Reads {
    /// The keychain, the resolver and what the enabled servers need ([`wanted`]); nothing in
    /// the Safe workspace.
    pub(crate) fn of(actor: &crate::actor::Actor) -> Self {
        Self {
            store: actor.wiring.mcp_secrets.clone(),
            resolve: Arc::clone(&actor.wiring.mcp_resolve),
            wanted: wanted(&actor.effective, actor.protection.safe_workspace()),
        }
    }

    /// The reads and the resolves ([`read`]): blocking, on the launch-prep thread.
    pub(crate) fn read(self) -> McpSecrets {
        read(self.store.as_deref(), &self.resolve, self.wanted)
    }
}

/// Reads `wanted`'s accounts from `store` and resolves its commands with `resolve`: blocking
/// keychain and file system calls, so on the launch-prep thread. A missing item, a keychain
/// error, an item that is not UTF-8 or holds NUL is unreadable (its bytes zeroed); with no
/// store every account is.
pub(crate) fn read(
    store: Option<&dyn SecretStore>,
    resolve: &Resolve,
    wanted: Wanted,
) -> McpSecrets {
    let mut secrets = McpSecrets::default();
    for command in wanted.commands {
        if let Some(found) = resolve(&command) {
            secrets.commands.insert(command, found);
        }
    }
    for account in wanted.accounts {
        let value = store.and_then(|store| match store.get(&account) {
            Ok(Some(bytes)) => usable(bytes),
            Ok(None) => None,
            Err(error) => {
                // The store's error names its kind, never the item.
                tracing::warn!(%error, "an MCP server's keychain value could not be read");
                None
            }
        });
        match value {
            Some(value) => {
                secrets.values.insert(account, value);
            }
            None => {
                secrets.unreadable.insert(account);
            }
        }
    }
    secrets
}

/// `bytes` as text an environment can hold: UTF-8 without NUL. The bytes are zeroed either way.
fn usable(bytes: Vec<u8>) -> Option<Zeroizing<String>> {
    let bytes = Zeroizing::new(bytes);
    let text = std::str::from_utf8(&bytes).ok()?;
    (!text.contains('\0')).then(|| Zeroizing::new(text.to_owned()))
}

/// The host's environment the entries draw on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HostEnv<'a> {
    /// The `PATH` Clauseo was started with: its name as the host has it (`Path` on Windows)
    /// and its value; `None` when it has none.
    pub(crate) path: Option<(&'a str, &'a OsString)>,
    /// On Windows, the variables a Windows program needs (`case_law::windows_env`); else none.
    pub(crate) windows_env: &'a [(String, String)],
}

/// A server left out because a value it needs is missing or unreadable: the server and the
/// variable.
pub(crate) type LeftOut = (String, String);

/// The session's lawyer servers, in name order, and those left out for a missing value.
pub(crate) fn servers(
    effective: &Effective,
    lawyer: Lawyer,
    secrets: &McpSecrets,
    host: HostEnv<'_>,
) -> (Vec<McpServer>, Vec<LeftOut>) {
    let mut sent = Vec::new();
    let mut left_out = Vec::new();
    if !allowed(lawyer) {
        return (sent, left_out);
    }
    for (name, server) in effective.enabled_mcp_servers() {
        let (typed, args, env) = match &server.transport {
            McpTransport::Local { command, args, env } => (command, args, env),
            // PR B plugs in here: a remote server goes as ACP's HTTP server, its headers filled
            // from the keychain as `env` is below, when the agent's `mcpCapabilities.http` says
            // it takes one.
            McpTransport::Remote { .. } => continue,
        };
        let Some(command) = secrets.commands.get(typed) else {
            tracing::warn!(
                server = name,
                "an MCP server's program is not on this computer; the server is left out of this \
                 session"
            );
            continue;
        };
        match filled(name, env, secrets) {
            Ok(mut env) => {
                add_host_env(&mut env, command, host);
                sent.push(McpServer::Stdio {
                    name: name.to_owned(),
                    command: command.clone(),
                    args: args.clone(),
                    env,
                });
            }
            Err(var) => {
                tracing::warn!(
                    server = name,
                    variable = var.as_str(),
                    "an MCP server's keychain value is missing or unreadable; the server is left \
                     out of this session"
                );
                left_out.push((name.to_owned(), var));
            }
        }
    }
    (sent, left_out)
}

/// `env` with each keychain value filled, in name order; or the first variable whose value
/// `secrets` does not hold.
fn filled(
    server: &str,
    env: &BTreeMap<String, McpValue>,
    secrets: &McpSecrets,
) -> Result<Vec<(String, String)>, String> {
    env.iter()
        .map(|(var, value)| match value {
            McpValue::Plain(text) => Ok((var.clone(), text.clone())),
            McpValue::Keychain(_) => secrets
                .get(&keychain_account(server, McpValueKind::Env, var))
                .map(|text| (var.clone(), text.to_owned()))
                .ok_or_else(|| var.clone()),
        })
        .collect()
}

/// Adds the `PATH` (the command's folder first) and the Windows variables to `env`, each only
/// when the server does not set that name (in any case).
fn add_host_env(env: &mut Vec<(String, String)>, command: &Path, host: HostEnv<'_>) {
    let has = |env: &[(String, String)], name: &str| {
        env.iter().any(|(own, _)| own.eq_ignore_ascii_case(name))
    };
    if !has(env, "PATH")
        && let Some(path) = server_path(command, host.path)
    {
        env.push(path);
    }
    for (name, value) in host.windows_env {
        if !has(env, name) {
            env.push((name.clone(), value.clone()));
        }
    }
}

/// The server's `PATH`: `command`'s folder, then the host's `PATH`, under the host's name for
/// it; `None` when neither gives a folder or the result is not UTF-8 (logged).
fn server_path(command: &Path, host: Option<(&str, &OsString)>) -> Option<(String, String)> {
    let folder = command.parent().filter(|folder| folder.is_absolute())?;
    let name = host.map_or("PATH", |(name, _)| name);
    let mut env = BTreeMap::from([(
        OsString::from(name),
        host.map(|(_, value)| value.clone()).unwrap_or_default(),
    )]);
    crate::agent::spec::prepend_path(&mut env, &[folder.to_owned()]);
    let value = env.remove(&OsString::from(name))?;
    let Ok(value) = value.into_string() else {
        tracing::warn!("an MCP server's PATH is not UTF-8; it gets the agent's own");
        return None;
    };
    Some((name.to_owned(), value))
}

/// The search a save and a launch make: the settings crate's resolver over this process's
/// environment (`clauseo_settings::mcp_import::resolve_command`): an absolute path that is a
/// file, or a bare name on the `PATH`, then in Homebrew's, nvm's, Volta's and the Windows Node
/// folders. File system calls: off the actor only.
pub(crate) fn resolve_here(command: &str) -> Option<PathBuf> {
    let env = clauseo_settings::mcp_import::RealEnv::from_process()?;
    clauseo_settings::mcp_import::resolve_command(&env.env(), command)
}
