//! The rules for the lawyer's MCP servers (Settings › Tools), shared by the load of
//! `mcp.servers` (`leaf/mcp.rs`) and the write of one server (`effective/mcp_write.rs`), and the
//! keychain names their values are stored under.
//!
//! - A server name is 1 to [`MCP_NAME_MAX`] ASCII letters, digits, `_` or `-`, holding no `__`
//!   and not ending in `_` (Claude splits a tool name `mcp__<server>__<tool>` at the first `__`
//!   after `mcp__`), and not a reserved name ([`crate::RESERVED_MCP_SERVERS`]) in any case.
//! - A local server's `command` is what the lawyer typed: a bare program name (`npx`, no path
//!   separator), which the host finds on this computer's search paths at each save and launch,
//!   or an absolute path. A relative path with a separator, an empty one, or a network path (a UNC
//!   share, which the program search refuses) is refused.
//! - A remote server's `url` is ASCII, `https://` with a host, or `http://` with the host
//!   `localhost` or `127.0.0.1` only; a port of at most 65535 if any; no user name or password
//!   (`@`), fragment (`#`), backslash, space or control character. A path and a query may
//!   follow.
//! - A header name is an HTTP token (RFC 9110 `tchar`s); a variable name a C identifier
//!   (`[A-Za-z_][A-Za-z0-9_]*`) when the lawyer types it. A file's variable names are only held to
//!   what an OS takes (not empty, no `=`, no NUL), so a firm's `ProgramFiles(x86)` still loads.
//!   A name the lawyer types appears once per server in any case (Windows' variables, HTTP's
//!   headers and Credential Manager's accounts are case-blind); a file's headers too.
//! - A value the lawyer types goes to the keychain under [`MCP_KEYCHAIN_SERVICE`], account
//!   [`keychain_account`]: `<server>/env/<name>` or `<server>/header/<name>`. No server name, and
//!   no variable or header name the lawyer types, holds `/`, so their accounts never collide; a
//!   hand-edited file's variable name may hold one, and its account is still that file's own.
//!   `settings.json` holds `{"keychain": true}` in the value's place.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use crate::leaf::reserved_mcp_server;
use crate::schema::{McpServerLaunch, McpTransport, McpValue};

/// The longest MCP server name.
pub const MCP_NAME_MAX: usize = 64;

/// The longest variable or header name.
pub const MCP_VALUE_NAME_MAX: usize = 128;

/// The keychain service every MCP value is stored under, apart from the plan keys
/// (`ai.clauseo.desktop`), the connectors' and the block list's. Like theirs, one name for
/// every build.
pub const MCP_KEYCHAIN_SERVICE: &str = "ai.clauseo.desktop.mcp";

/// Which of a server's lists a value belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum McpValueKind {
    /// A local server's variable.
    Env,
    /// A remote server's header.
    Header,
}

impl McpValueKind {
    /// The kind of the values `server` holds: a local server's are variables.
    pub fn of(server: &McpServerLaunch) -> Self {
        match server.transport {
            McpTransport::Local { .. } => Self::Env,
            McpTransport::Remote { .. } => Self::Header,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Header => "header",
        }
    }
}

/// The keychain account of `server`'s value `name`: `<server>/env/<name>` or
/// `<server>/header/<name>`.
pub fn keychain_account(server: &str, kind: McpValueKind, name: &str) -> String {
    format!("{server}/{}/{name}", kind.word())
}

/// Why a server is refused. Each names a rule, never a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum McpRefusal {
    /// The name is empty.
    NameEmpty,
    /// Another server has the name.
    NameTaken,
    /// The name is kept for one of Clauseo's own tools.
    NameReserved,
    /// The name breaks the name rule.
    NameInvalid,
    /// A local server's `command` is neither a bare name nor an absolute path.
    CommandInvalid,
    /// A local server's `command` is not on this computer (the host's check, when it resolves
    /// it).
    CommandNotFound,
    /// A remote server's `url` breaks the URL rule.
    UrlInvalid,
    /// A header name is not a token, or appears twice.
    HeaderNameInvalid,
    /// A variable name is not an identifier, or appears twice.
    EnvNameInvalid,
    /// No server has the name.
    Unknown,
}

impl fmt::Display for McpRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NameEmpty => "the server has no name",
            Self::NameTaken => "another server has this name",
            Self::NameReserved => {
                "the name is kept for one of Clauseo's or Claude Code's own tools"
            }
            Self::NameInvalid => {
                "a server name is 1 to 64 letters, digits, _ or -, with no __ and no trailing _"
            }
            Self::CommandInvalid => "the program is neither a bare name nor an absolute path",
            Self::CommandNotFound => "the program is not on this computer",
            Self::UrlInvalid => "the URL is not https (or http on this computer) with a host",
            Self::HeaderNameInvalid => "a header name is not a token, or appears twice",
            Self::EnvNameInvalid => "a variable name is not an identifier, or appears twice",
            Self::Unknown => "no server has this name",
        })
    }
}

/// The name rule, with the refusal it breaks first.
pub fn check_name(name: &str) -> Result<(), McpRefusal> {
    if name.is_empty() {
        return Err(McpRefusal::NameEmpty);
    }
    if !name_shaped(name) || name.contains("__") || name.ends_with('_') {
        return Err(McpRefusal::NameInvalid);
    }
    if reserved_mcp_server(name).is_some() {
        return Err(McpRefusal::NameReserved);
    }
    Ok(())
}

/// Whether `name` is 1 to [`MCP_NAME_MAX`] ASCII letters, digits, `_` or `-`.
pub(crate) fn name_shaped(name: &str) -> bool {
    (1..=MCP_NAME_MAX).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// Whether `command` is a bare program name (no `/`, nor `\` on any OS, so a Windows path
/// typed on a Mac is no bare name) or an absolute path on this computer: a network path (a UNC
/// share such as `\\server\share\tool.exe`, `//server/share`, or a `\\?\` device path) is
/// refused, as the program search refuses it (`mcp_import::resolve_command`), so a firm's server
/// on a share is left out with a warning at load, never silently at launch.
pub fn command_ok(command: &str) -> bool {
    !command.is_empty()
        && !command_on_network(command)
        && (Path::new(command).is_absolute() || !command.contains(['/', '\\']))
        && !command.contains('\0')
}

/// Whether `command` is a network or device path: it starts with two slashes of either kind.
pub(crate) fn command_on_network(command: &str) -> bool {
    let mut start = command.chars().take(2);
    start.all(|c| c == '/' || c == '\\') && command.len() >= 2
}

/// Whether `name` is a variable name the lawyer may type: a C identifier of at most
/// [`MCP_VALUE_NAME_MAX`] bytes.
pub fn env_name_ok(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= MCP_VALUE_NAME_MAX
        && bytes
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Whether a file's variable name is one an OS takes: not empty, no `=`, no NUL.
pub(crate) fn env_name_loads(name: &str) -> bool {
    !name.is_empty() && !name.contains(['=', '\0'])
}

/// Whether `name` is an HTTP header name: 1 to [`MCP_VALUE_NAME_MAX`] token characters.
pub fn header_name_ok(name: &str) -> bool {
    (1..=MCP_VALUE_NAME_MAX).contains(&name.len())
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

/// Whether `url` is a URL a remote server may have (the module's rule).
pub fn url_ok(url: &str) -> bool {
    let after = |scheme: &str| {
        url.get(..scheme.len())
            .filter(|start| start.eq_ignore_ascii_case(scheme))
            .and_then(|_| url.get(scheme.len()..))
    };
    if !url.is_ascii()
        || url.chars().any(|c| {
            matches!(c, '@' | '#' | '\\') || c.is_ascii_whitespace() || c.is_ascii_control()
        })
    {
        return false;
    }
    let (rest, local_only) = match (after("https://"), after("http://")) {
        (Some(rest), _) => (rest, false),
        (None, Some(rest)) => (rest, true),
        (None, None) => return false,
    };
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    if !crate::plans::authority_ok(authority) {
        return false;
    }
    if !local_only {
        return true;
    }
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _port)| host);
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1"
}

/// The rules a server the lawyer saves must pass, but whether its name is free (the caller
/// knows the list): the name, the command or URL, and each variable's or header's name, unique
/// (a header's in any case).
pub fn check_server(name: &str, server: &McpServerLaunch) -> Result<(), McpRefusal> {
    check_name(name)?;
    match &server.transport {
        McpTransport::Local { command, env, .. } => {
            if !command_ok(command) {
                return Err(McpRefusal::CommandInvalid);
            }
            // Twice in any case is refused too: Windows' variables and Credential Manager's
            // accounts are case-blind.
            let mut seen = BTreeSet::new();
            if !env
                .keys()
                .all(|name| env_name_ok(name) && seen.insert(name.to_ascii_lowercase()))
            {
                return Err(McpRefusal::EnvNameInvalid);
            }
        }
        McpTransport::Remote { url, headers } => {
            if !url_ok(url) {
                return Err(McpRefusal::UrlInvalid);
            }
            if !headers_ok(headers) {
                return Err(McpRefusal::HeaderNameInvalid);
            }
        }
    }
    Ok(())
}

/// Whether every header name is a token and none appears twice in any case.
pub(crate) fn headers_ok(headers: &BTreeMap<String, McpValue>) -> bool {
    let mut seen = BTreeSet::new();
    headers
        .keys()
        .all(|name| header_name_ok(name) && seen.insert(name.to_ascii_lowercase()))
}
