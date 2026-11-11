//! The `mcp.servers` list, typed entry by entry.

use super::{reserved_mcp_server, typed_value};
use crate::mcp::{MCP_NAME_MAX, command_ok, env_name_loads, headers_ok, name_shaped, url_ok};
use crate::schema::{McpServerLaunch, McpTransport};
use crate::warning::WarningKind;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The fields a local [`McpServerLaunch`] reads; any other is an unknown key.
const LOCAL_FIELDS: [&str; 4] = ["command", "args", "env", "enabled"];

/// The fields a remote [`McpServerLaunch`] reads; any other is an unknown key.
const REMOTE_FIELDS: [&str; 3] = ["url", "headers", "enabled"];

/// The `mcp.servers` list. A value that is not an object refuses the key (a policy's locks the
/// list empty). Inside it, an entry with a bad name, a name holding `__` or ending in `_`, a
/// reserved name ([`RESERVED_MCP_SERVERS`](super::RESERVED_MCP_SERVERS)), a value that does not
/// type, a `command` that is a relative path (a bare name such as `npx` is kept: the host finds
/// it at each launch), a `url` that breaks [`url_ok`], a header name that is no token (or
/// twice in any case) or a variable name an OS refuses is left out with a warning in `dropped`,
/// and the other entries load. An entry with `command` is local, and any `url` or `headers`
/// beside it is an unknown key; one with `url` and no `command` is remote.
pub(super) fn mcp_servers(
    key: &str,
    value: Value,
    dropped: &mut Vec<WarningKind>,
) -> Result<BTreeMap<String, McpServerLaunch>, WarningKind> {
    let Value::Object(entries) = value else {
        return Err(WarningKind::InvalidValue {
            key: key.to_owned(),
            message: "it must be an object from server name to server".to_owned(),
        });
    };
    let mut servers = BTreeMap::new();
    for (name, entry) in entries {
        let entry_key = format!("{key}.{name}");
        if let Err(kind) = name_rule(&name, &entry_key) {
            dropped.push(kind);
            continue;
        }
        let entry = match entry {
            Value::Object(fields) => Value::Object(known_fields(fields, &entry_key, dropped)),
            other => other,
        };
        match typed_value::<McpServerLaunch>(&entry_key, entry) {
            Ok(server) => match server_rule(&server) {
                None => {
                    servers.insert(name, server);
                }
                Some(message) => dropped.push(WarningKind::InvalidValue {
                    key: entry_key,
                    message,
                }),
            },
            Err(kind) => dropped.push(kind),
        }
    }
    Ok(servers)
}

/// The name rules, as a warning on `entry_key`.
fn name_rule(name: &str, entry_key: &str) -> Result<(), WarningKind> {
    let refused = |message: String| WarningKind::InvalidValue {
        key: entry_key.to_owned(),
        message,
    };
    if !name_shaped(name) {
        return Err(refused(format!(
            "a server name is 1 to {MCP_NAME_MAX} letters, digits, _ or -"
        )));
    }
    // Claude names a tool `mcp__<server>__<tool>` and splits it at the first `__` after
    // `mcp__`: the server is everything before it (`kr()` in claude-agent-sdk 0.3.318). So
    // `case-law__x` and `case-law_` (tool `mcp__case-law___x`) both read as server
    // `case-law` and would inherit its pre-allow. A leading `_` cannot split. The name passed
    // [`name_shaped`], so it is name-shaped and is quoted (#411's rule).
    if name.contains("__") || name.ends_with('_') {
        return Err(refused(format!(
            "double underscores and a trailing underscore are not allowed in server names ({name})"
        )));
    }
    if let Some((reserved, what)) = reserved_mcp_server(name) {
        return Err(refused(format!(
            "the server name {reserved} is reserved for {what}"
        )));
    }
    Ok(())
}

/// `fields` without the keys its kind does not read, each one an unknown key in `dropped`: a
/// local entry's (it has `command`) `url` and `headers` included, so a stray `url` beside a
/// `command` leaves the server local, as it loaded before remote servers existed.
fn known_fields(
    mut fields: Map<String, Value>,
    entry_key: &str,
    dropped: &mut Vec<WarningKind>,
) -> Map<String, Value> {
    let known: &[&str] = if fields.contains_key("command") || !fields.contains_key("url") {
        &LOCAL_FIELDS
    } else {
        &REMOTE_FIELDS
    };
    let unknown: Vec<String> = fields
        .keys()
        .filter(|field| !known.contains(&field.as_str()))
        .cloned()
        .collect();
    for field in unknown {
        fields.remove(&field);
        dropped.push(WarningKind::UnknownKey {
            key: format!("{entry_key}.{field}"),
        });
    }
    fields
}

/// The rule a typed server breaks, as a warning's message naming no value but a path, a
/// header's or a variable's place.
fn server_rule(server: &McpServerLaunch) -> Option<String> {
    match &server.transport {
        McpTransport::Local { command, .. } if !command_ok(command) => Some(format!(
            "{command} is neither a program name nor an absolute path"
        )),
        McpTransport::Local { env, .. } => env
            .keys()
            .position(|name| !env_name_loads(name))
            .map(|at| format!("variable {} has a name no OS takes", at + 1)),
        McpTransport::Remote { url, .. } if !url_ok(url) => {
            Some("the url is not https (or http to localhost or 127.0.0.1) with a host".to_owned())
        }
        McpTransport::Remote { headers, .. } if !headers_ok(headers) => {
            Some("a header name is not a token, or appears twice".to_owned())
        }
        McpTransport::Remote { .. } => None,
    }
}
