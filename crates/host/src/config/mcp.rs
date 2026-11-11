//! The keychain seam of the lawyer's MCP servers (Settings › Tools), kept apart from
//! `config.rs`.

use std::sync::Arc;

use clauseo_connectors::SecretStore;
use clauseo_settings::mcp_import::{
    CandidateKind, Env, ImportCandidate, find_candidates, resolve_command,
};

use super::HostConfig;

/// What the confirm before a lawyer's MCP server is added, or its launch changed, shows
/// (`HostConfig::with_mcp_confirm`): the program and its arguments as the lawyer typed them, or
/// a remote server's URL. Never a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpLaunchAsk {
    /// The server's name.
    pub server: String,
    /// What starts, verbatim: a local server's command as typed, then each argument; empty for a
    /// remote server.
    pub program: Vec<String>,
    /// A remote server's URL; `None` for a local one.
    pub url: Option<String>,
    /// The variables (a local server's) or headers (a remote one's) that are new, or whose value
    /// changes how the program loads (`NODE_OPTIONS`, `PATH`, `DYLD_*`, …), verbatim, in name
    /// order; empty when none. Never a value.
    pub names: Vec<String>,
}

/// The native confirm before a lawyer's MCP server is added or its launch changes (its command,
/// arguments, variable or header names, URL, or a loader variable's value): `true` when the
/// lawyer allows it. It blocks: the host calls it off the actor, on its keychain thread, one at
/// a time, after the program is found and before anything is written. Desktop draws it in Rust,
/// as the pay link's.
pub trait McpLaunchConfirm: Send + Sync {
    /// Asks about `ask`; `true` allows the save.
    fn confirm(&self, ask: &McpLaunchAsk) -> bool;
}

/// Finds a lawyer's MCP server's program: the command as typed (a bare name or an absolute path)
/// to the program's path, or `None` when there is none (`HostConfig::with_mcp_resolver`).
pub type McpResolver = Arc<dyn Fn(&str) -> Option<std::path::PathBuf> + Send + Sync>;

/// Finds the MCP servers Claude Desktop, Claude Code and Codex set up, given the names the
/// lawyer's list already has (`HostConfig::with_mcp_import`).
pub type McpFinder = Arc<dyn Fn(&[String]) -> Vec<ImportCandidate> + Send + Sync>;

#[expect(
    clippy::multiple_inherent_impl,
    reason = "config.rs is at its 600-line limit; the MCP servers' seams live in their own file"
)]
impl HostConfig {
    /// The keychain the lawyer's MCP server values are written to and read from: desktop's
    /// `KeyringStore` under `clauseo_settings::MCP_KEYCHAIN_SERVICE`, each value under
    /// `clauseo_settings::keychain_account`. Unset (the default, and on Linux), a save that
    /// sets a value is refused with `mcp.secret_store_failed`, and a server whose settings name
    /// a keychain value is left out of every session.
    #[must_use]
    pub fn with_mcp_secrets(mut self, secrets: Arc<dyn SecretStore>) -> Self {
        self.wiring.mcp_secrets = Some(secrets);
        self
    }

    /// The confirm a `SaveServer` that adds a server, or changes its command, arguments,
    /// variable or header names, URL, or a loader variable's value (`NODE_OPTIONS`, `PATH`,
    /// `DYLD_*`, …), must pass (`McpLaunchConfirm`): declined, the save answers
    /// `mcp.save_declined` with nothing written. A rename, a turn on or off, or other new values
    /// alone ask nothing; an ask too long to show whole is refused before it. Unset, every such
    /// save is declined: no settings-window script adds a program unseen.
    #[must_use]
    pub fn with_mcp_confirm(mut self, confirm: Arc<dyn McpLaunchConfirm>) -> Self {
        self.wiring.mcp_confirm = Some(confirm);
        self
    }

    /// How a lawyer's MCP server's program is found, at a save (`mcp.command_not_found` when it
    /// is not) and at each launch (the server left out when it is not): `resolve` takes the
    /// command as typed, a bare name or an absolute path, and answers the program's path. It runs
    /// off the actor. Unset, `clauseo_settings::mcp_import::resolve_command` over this process's
    /// environment (tests set their own).
    #[must_use]
    pub fn with_mcp_resolver(mut self, resolve: McpResolver) -> Self {
        self.wiring.mcp_resolve = resolve;
        self
    }

    /// How Settings › Tools finds the servers other apps set up (`McpCommand::Import` and
    /// `McpToolsView::importable`): desktop's runs `clauseo_settings::mcp_import::find_candidates`
    /// over this computer ([`find_mcp_imports_here`]). It runs off the actor, at the start, at
    /// each app catch-up, after a removal and at each import. Unset (the default), nothing is
    /// importable and an import adds nothing.
    #[must_use]
    pub fn with_mcp_import(mut self, find: McpFinder) -> Self {
        self.wiring.mcp_find = Some(find);
        self
    }
}

/// The servers Claude Desktop, Claude Code and Codex set up on this computer, for
/// [`HostConfig::with_mcp_import`]; none on an OS Clauseo is not built for or with no home
/// folder. As [`find_mcp_imports_in`].
pub fn find_mcp_imports_here(existing: &[String]) -> Vec<ImportCandidate> {
    use clauseo_settings::mcp_import::RealEnv;
    RealEnv::from_process().map_or_else(Vec::new, |env| find_mcp_imports_in(&env.env(), existing))
}

/// The servers `find_candidates` finds under `env`, each local one's program as the settings
/// keep it: the bare file name (`npx`) when searching for that name under `env` finds the same
/// program, so a launch finds it again after an nvm, Homebrew or Volta upgrade moves it; else
/// the absolute path found.
pub fn find_mcp_imports_in(env: &Env<'_>, existing: &[String]) -> Vec<ImportCandidate> {
    let mut candidates = find_candidates(env, existing);
    for candidate in &mut candidates {
        if let CandidateKind::Local { command, .. } = &mut candidate.kind
            && let Some(name) = command.file_name().and_then(|name| name.to_str())
            && resolve_command(env, name).as_deref() == Some(command.as_path())
        {
            *command = std::path::PathBuf::from(name);
        }
    }
    candidates
}
