//! When a save asks the lawyer first (0028's amendment, iris's MED-5 on #1012), and what the
//! confirm shows: an add, a change of what starts (the kind, the command, the arguments, the URL,
//! the set of variable or header names), or a new value for a variable that changes how a
//! program loads ([`loader_var`]). The ask names the program and its arguments, or the URL, and
//! every variable or header that is new or whose loader value changed, all verbatim; an ask too
//! long to show whole is refused before any prompt, so nothing is ever cut from view.

use clauseo_protocol::{McpSecretValue, McpServerEdit};
use clauseo_settings::{McpRefusal, McpServerLaunch, McpTransport};

use crate::config::mcp::McpLaunchAsk;
use crate::error::Error;

/// The most characters an ask may hold (its program and arguments, or its URL, and its names),
/// so the confirm shows all of it.
pub(crate) const MAX_ASK_CHARS: usize = 2000;

/// The variables whose value changes how a program loads, matched exactly, in any case on
/// Windows. `DYLD_*` is matched by prefix, and `NPM_CONFIG_REGISTRY` in any case everywhere
/// (npm reads it in either case).
const LOADER_VARS: [&str; 7] = [
    "NODE_OPTIONS",
    "NODE_PATH",
    "PATH",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "PYTHONPATH",
    "PYTHONSTARTUP",
];

/// Whether `name` is a loader variable (module docs).
pub(crate) fn loader_var(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let seen = if cfg!(windows) { upper.as_str() } else { name };
    LOADER_VARS.contains(&seen) || seen.starts_with("DYLD_") || upper == "NPM_CONFIG_REGISTRY"
}

/// The ask a save of `new` (from `edit`) over `old` (`None` for an add) needs, or `None` when
/// it asks nothing. An ask over [`MAX_ASK_CHARS`] is refused: `mcp.command_invalid` for a local
/// server, `mcp.url_invalid` for a remote one.
pub(crate) fn ask(
    old: Option<&McpServerLaunch>,
    new: &McpServerLaunch,
    edit: &McpServerEdit,
) -> Result<Option<McpLaunchAsk>, Error> {
    let same_kind = old.filter(|old| {
        matches!(
            (&old.transport, &new.transport),
            (McpTransport::Local { .. }, McpTransport::Local { .. })
                | (McpTransport::Remote { .. }, McpTransport::Remote { .. })
        )
    });
    let held = same_kind.map(McpServerLaunch::values);
    let mut names: Vec<String> = new
        .values()
        .keys()
        .filter(|name| held.is_none_or(|held| !held.contains_key(*name)))
        .cloned()
        .collect();
    // A new value for a loader variable the server already had (a new one is listed above).
    for change in &edit.env {
        if matches!(change.value, McpSecretValue::Set(_))
            && loader_var(&change.name)
            && !names.contains(&change.name)
        {
            names.push(change.name.clone());
        }
    }
    let launch_changed = match (same_kind.map(|old| &old.transport), &new.transport) {
        (
            Some(McpTransport::Local { command, args, env }),
            McpTransport::Local {
                command: new_command,
                args: new_args,
                env: new_env,
            },
        ) => command != new_command || args != new_args || !env.keys().eq(new_env.keys()),
        (
            Some(McpTransport::Remote { url, headers }),
            McpTransport::Remote {
                url: new_url,
                headers: new_headers,
            },
        ) => url != new_url || !headers.keys().eq(new_headers.keys()),
        // An add, or a change of kind.
        _ => true,
    };
    names.sort();
    names.dedup();
    if !launch_changed && names.is_empty() {
        return Ok(None);
    }
    let (program, url) = match &new.transport {
        McpTransport::Local { command, args, .. } => (
            std::iter::once(command.clone())
                .chain(args.iter().cloned())
                .collect::<Vec<_>>(),
            None,
        ),
        McpTransport::Remote { url, .. } => (Vec::new(), Some(url.clone())),
    };
    let shown: usize = program
        .iter()
        .chain(url.iter())
        .chain(names.iter())
        .map(|part| part.chars().count() + 2)
        .sum();
    if shown > MAX_ASK_CHARS {
        let refusal = if url.is_some() {
            McpRefusal::UrlInvalid
        } else {
            McpRefusal::CommandInvalid
        };
        return Err(Error::Settings(clauseo_settings::Error::Mcp {
            key: format!("mcp.servers.{}", edit.name),
            refusal,
        }));
    }
    Ok(Some(McpLaunchAsk {
        server: edit.name.clone(),
        program,
        url,
        names,
    }))
}
