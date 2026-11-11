//! The `mcp.servers` list, typed entry by entry.

use super::{reserved_mcp_server, typed_value};
use crate::schema::McpServerLaunch;
use crate::warning::WarningKind;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// The fields an [`McpServerLaunch`] reads; any other is an unknown key.
const MCP_SERVER_FIELDS: [&str; 4] = ["command", "args", "env", "enabled"];

/// The longest MCP server name.
const MCP_NAME_MAX: usize = 64;

/// The `mcp.servers` list. A value that is not an object refuses the key (a policy's locks the
/// list empty). Inside it, an entry with a bad name, a name holding `__` or ending in `_`, a
/// reserved name ([`RESERVED_MCP_SERVERS`]), a value that does not type or a relative `command`
/// is left out with a warning in `dropped`, and the other entries load.
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
        if !mcp_name_ok(&name) {
            dropped.push(WarningKind::InvalidValue {
                key: entry_key,
                message: format!("a server name is 1 to {MCP_NAME_MAX} letters, digits, _ or -"),
            });
            continue;
        }
        // Claude names a tool `mcp__<server>__<tool>` and splits it at the first `__` after
        // `mcp__`: the server is everything before it (`kr()` in claude-agent-sdk 0.3.318). So
        // `case-law__x` and `case-law_` (tool `mcp__case-law___x`) both read as server
        // `case-law` and would inherit its pre-allow. A leading `_` cannot split. The name passed
        // [`mcp_name_ok`], so it is name-shaped and is quoted (#411's rule).
        if name.contains("__") || name.ends_with('_') {
            dropped.push(WarningKind::InvalidValue {
                message: format!(
                    "double underscores and a trailing underscore are not allowed in server \
                     names ({name})"
                ),
                key: entry_key,
            });
            continue;
        }
        if let Some((reserved, what)) = reserved_mcp_server(&name) {
            dropped.push(WarningKind::InvalidValue {
                key: entry_key,
                message: format!("the server name {reserved} is reserved for {what}"),
            });
            continue;
        }
        if let Value::Object(fields) = &entry {
            dropped.extend(
                fields
                    .keys()
                    .filter(|field| !MCP_SERVER_FIELDS.contains(&field.as_str()))
                    .map(|field| WarningKind::UnknownKey {
                        key: format!("{entry_key}.{field}"),
                    }),
            );
        }
        match typed_value::<McpServerLaunch>(&entry_key, entry) {
            Ok(server) if Path::new(&server.command).is_absolute() => {
                servers.insert(name, server);
            }
            Ok(server) => dropped.push(WarningKind::InvalidValue {
                key: entry_key,
                message: format!("{} is not an absolute path", server.command),
            }),
            Err(kind) => dropped.push(kind),
        }
    }
    Ok(servers)
}

/// Whether `name` is 1 to [`MCP_NAME_MAX`] ASCII letters, digits, `_` or `-`.
fn mcp_name_ok(name: &str) -> bool {
    (1..=MCP_NAME_MAX).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}
