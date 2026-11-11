//! Codex's `config.toml` and its `[mcp_servers.<name>]` tables.
//!
//! Keys read (from the Codex docs, developers.openai.com/codex/mcp, the "STDIO servers" and
//! "Streamable HTTP servers" lists): `command`, `args`, `env` (stdio); `url`, `http_headers`,
//! `env_http_headers` (header to variable name), `bearer_token_env_var` (streamable HTTP);
//! `enabled = false` disables a server. Codex's `env_vars`, `cwd` and `auth` are not read.

use super::command;
use super::path::{self, join};
use super::{CandidateKind, Entry, Env, Problem, Source};
use std::path::Path;

/// Adds the enabled servers of Codex's `config.toml` to `out`. `CODEX_HOME` names the folder
/// when it is set, else `~/.codex`.
pub(crate) fn read(env: &Env<'_>, out: &mut Vec<Entry>) {
    let config = match path::var(env, "CODEX_HOME") {
        Some(dir) => join(env.os, &dir, "config.toml"),
        None => join(env.os, &path::home(env), ".codex/config.toml"),
    };
    let Some(text) = env.fs.read_to_string(Path::new(&config)) else {
        return;
    };
    let Ok(doc) = toml::from_str::<toml::Table>(&text) else {
        return;
    };
    let Some(servers) = doc.get("mcp_servers").and_then(toml::Value::as_table) else {
        return;
    };
    for (name, value) in servers {
        let Some(table) = value.as_table() else {
            continue;
        };
        if table.get("enabled").and_then(toml::Value::as_bool) == Some(false) {
            continue;
        }
        out.extend(entry(env, name, table));
    }
}

/// One server table, or `None` when it has neither a `command` nor a `url`. A `command` makes it
/// local, as Codex's stdio transport; otherwise a `url` makes it streamable HTTP.
fn entry(env: &Env<'_>, name: &str, table: &toml::Table) -> Option<Entry> {
    let mut problems = Vec::new();
    let kind = if let Some(command) = table.get("command") {
        let command = command.as_str()?;
        let resolved = command::resolve_or_problem(env, command, command, &mut problems);
        CandidateKind::Local {
            command: resolved,
            args: toml_list(table.get("args"), &mut problems),
            env: toml_pairs(table.get("env"), &mut problems),
        }
    } else {
        let url = table.get("url")?.as_str()?.to_owned();
        let mut headers = toml_pairs(table.get("http_headers"), &mut problems);
        for (header, var) in toml_pairs(table.get("env_http_headers"), &mut problems) {
            let value = if let Some(value) = path::var(env, &var) {
                value
            } else {
                problems.push(Problem::UnresolvedVariable { name: var.clone() });
                format!("${{{var}}}")
            };
            headers.push((header, value));
        }
        if let Some(var) = table
            .get("bearer_token_env_var")
            .and_then(toml::Value::as_str)
        {
            let value = if let Some(token) = path::var(env, var) {
                format!("Bearer {token}")
            } else {
                problems.push(Problem::UnresolvedVariable {
                    name: var.to_owned(),
                });
                format!("Bearer ${{{var}}}")
            };
            headers.push(("Authorization".to_owned(), value));
        }
        CandidateKind::Remote { url, headers }
    };
    Some(Entry {
        raw_name: name.to_owned(),
        source: Source::Codex,
        kind,
        problems,
    })
}

/// The string items of a TOML array. An item that is not a string is left out with
/// [`Problem::Unreadable`].
fn toml_list(value: Option<&toml::Value>, problems: &mut Vec<Problem>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        problems.push(Problem::Unreadable);
        return Vec::new();
    };
    let mut texts = Vec::new();
    for item in items {
        if let Some(text) = item.as_str() {
            texts.push(text.to_owned());
        } else {
            problems.push(Problem::Unreadable);
        }
    }
    texts
}

/// The string values of a TOML table, sorted by key. A value that is not a string is left out
/// with [`Problem::Unreadable`].
fn toml_pairs(value: Option<&toml::Value>, problems: &mut Vec<Problem>) -> Vec<(String, String)> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(table) = value.as_table() else {
        problems.push(Problem::Unreadable);
        return Vec::new();
    };
    let mut pairs = Vec::new();
    for (key, value) in table {
        if let Some(text) = value.as_str() {
            pairs.push((key.clone(), text.to_owned()));
        } else {
            problems.push(Problem::Unreadable);
        }
    }
    pairs.sort();
    pairs
}
