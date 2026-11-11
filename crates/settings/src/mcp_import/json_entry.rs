//! The `mcpServers` entries of a Claude config (Claude Code's `.claude.json` and `.mcp.json`, and
//! Claude Desktop's config file), read the way Claude writes them.

use super::{CandidateKind, Entry, Env, ImportSecret, Problem, Source, command, expand};
use serde_json::Value;

/// The JSON document at `path`, read up to `max` bytes, or `None` when it is missing, unreadable,
/// too big or not JSON. A parse error is never reported with the file's text.
pub(crate) fn read_json(env: &Env<'_>, path: &str, max: u64) -> Option<Value> {
    let text = super::read_config(env, path, max)?;
    serde_json::from_str(&text).ok()
}

/// Adds the entries under `mcpServers` in `doc` to `out`, each read from `source`. With `expand`,
/// `${VAR}` in its text is filled from the environment; without it, the text is kept as written.
pub(crate) fn servers_in(
    env: &Env<'_>,
    doc: &Value,
    source: &Source,
    expand: bool,
    out: &mut Vec<Entry>,
) {
    let Some(servers) = doc.get("mcpServers").and_then(Value::as_object) else {
        return;
    };
    for (name, value) in servers {
        out.extend(entry(env, name, value, source.clone(), expand));
    }
}

/// One entry, or `None` when it has nothing to import (not an object, or no command or URL).
///
/// `type` decides the transport: `stdio` and no type with a `command` are local; `http`, `sse`
/// and no type with a `url` are remote. Any other type is an [`Problem::UnsupportedTransport`].
fn entry(
    env: &Env<'_>,
    raw_name: &str,
    value: &Value,
    source: Source,
    expand: bool,
) -> Option<Entry> {
    let Value::Object(fields) = value else {
        return None;
    };
    let ty = match fields.get("type") {
        None => None,
        Some(Value::String(ty)) => Some(ty.as_str()),
        Some(_) => return None,
    };
    let text_of = |key: &str| fields.get(key).and_then(Value::as_str);
    let command_text = text_of("command");
    let url = text_of("url");
    let mut problems = Vec::new();
    if let Some(ty) = ty.filter(|ty| !matches!(*ty, "stdio" | "http")) {
        problems.push(Problem::UnsupportedTransport {
            kind: ty.to_owned(),
        });
    }
    let remote = match ty {
        Some("http" | "sse") => true,
        Some("stdio") => false,
        _ => url.is_some() && command_text.is_none(),
    };
    let kind = if remote {
        let url = url?;
        CandidateKind::Remote {
            url: ImportSecret::new(fill(url, env, expand, &mut problems)),
            headers: text_pairs(fields.get("headers"), env, expand, &mut problems),
        }
    } else {
        let command_text = command_text?;
        let command_now = fill(command_text, env, expand, &mut problems);
        let command = command::resolve_or_problem(env, command_text, &command_now, &mut problems);
        CandidateKind::Local {
            command,
            args: text_list(fields.get("args"), env, expand, &mut problems),
            env: text_pairs(fields.get("env"), env, expand, &mut problems),
        }
    };
    Some(Entry {
        raw_name: raw_name.to_owned(),
        source,
        kind,
        problems,
    })
}

/// `text`, with variables filled when `expand` is set.
fn fill(text: &str, env: &Env<'_>, expand: bool, problems: &mut Vec<Problem>) -> String {
    if expand {
        expand::expand(text, env.vars, problems)
    } else {
        text.to_owned()
    }
}

/// The string values of a JSON object, as secrets, sorted by key. A value that is not a string is
/// left out with [`Problem::Unreadable`].
fn text_pairs(
    value: Option<&Value>,
    env: &Env<'_>,
    expand: bool,
    problems: &mut Vec<Problem>,
) -> Vec<(String, ImportSecret)> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Value::Object(map) = value else {
        problems.push(Problem::Unreadable);
        return Vec::new();
    };
    let mut pairs = Vec::new();
    for (key, value) in map {
        match value.as_str() {
            Some(text) => pairs.push((
                key.clone(),
                ImportSecret::new(fill(text, env, expand, problems)),
            )),
            None => problems.push(Problem::Unreadable),
        }
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

/// The string items of a JSON array, as secrets. An item that is not a string is left out with
/// [`Problem::Unreadable`].
fn text_list(
    value: Option<&Value>,
    env: &Env<'_>,
    expand: bool,
    problems: &mut Vec<Problem>,
) -> Vec<ImportSecret> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Value::Array(items) = value else {
        problems.push(Problem::Unreadable);
        return Vec::new();
    };
    let mut texts = Vec::new();
    for item in items {
        match item.as_str() {
            Some(text) => texts.push(ImportSecret::new(fill(text, env, expand, problems))),
            None => problems.push(Problem::Unreadable),
        }
    }
    texts
}
