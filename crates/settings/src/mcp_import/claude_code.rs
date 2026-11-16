//! Claude Code's `.claude.json` (user and per-project servers) and each project's `.mcp.json`.
//!
//! The user's own text is expanded (the user scope). Project text is kept as written: a
//! `projects.<folder>.mcpServers` entry and a repository's `.mcp.json` are not the lawyer's to
//! expand. A `.mcp.json` server is imported only when Claude Code has approved it for the project
//! (`enabledMcpjsonServers`, or `enableAllProjectMcpServers`, and `hasTrustDialogAccepted`); a
//! server in `disabledMcpjsonServers` is left out.

use super::Entries;
use super::path::{self, join};
use super::{Env, MAX_CLAUDE_JSON_BYTES, MAX_CONFIG_BYTES, Problem, Source, json_entry};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// Adds Claude Code's servers to `out`: the user-wide `mcpServers`, each `projects.<folder>`
/// entry's `mcpServers`, and each listed folder's `.mcp.json` that exists.
pub(crate) fn read(env: &Env<'_>, out: &mut Entries) {
    let config = join(env.os, &path::home(env), ".claude.json");
    let Some(doc) = json_entry::read_json(env, &config, MAX_CLAUDE_JSON_BYTES) else {
        return;
    };
    json_entry::servers_in(env, &doc, &Source::ClaudeCodeUser, true, out);
    let Some(projects) = doc.get("projects").and_then(Value::as_object) else {
        return;
    };
    out.left_out += projects.len().saturating_sub(super::MAX_ENTRIES_PER_SOURCE);
    for (folder, project) in projects.iter().take(super::MAX_ENTRIES_PER_SOURCE) {
        // A project key must be a local absolute folder; a relative or network key is skipped.
        if !path::is_local_absolute(env.os, folder) {
            continue;
        }
        let source = Source::ClaudeCodeProject {
            folder: PathBuf::from(folder),
        };
        json_entry::servers_in(env, project, &source, false, out);
        add_mcp_json(env, folder, project, &source, out);
    }
}

/// Adds the servers of the project's `.mcp.json` to `out` (verbatim text), leaving out those it
/// disables and marking those it has not approved.
fn add_mcp_json(env: &Env<'_>, folder: &str, project: &Value, source: &Source, out: &mut Entries) {
    let mcp_json = join(env.os, folder, ".mcp.json");
    if !env.fs.is_file(Path::new(&mcp_json)) {
        return;
    }
    let Some(file) = json_entry::read_json(env, &mcp_json, MAX_CONFIG_BYTES) else {
        return;
    };
    let approval = Approval::of(project);
    let mut found = Entries::default();
    json_entry::servers_in(env, &file, source, false, &mut found);
    out.left_out += found.left_out;
    for mut entry in found.list {
        if approval.disabled.contains(&entry.raw_name) {
            continue;
        }
        if !approval.approves(&entry.raw_name) {
            entry.problems.push(Problem::NotApproved);
        }
        out.push(entry);
    }
}

/// Claude Code's approval keys for one project, as `~/.claude.json` holds them.
struct Approval {
    enabled: Vec<String>,
    disabled: Vec<String>,
    all: bool,
    trusted: bool,
}

impl Approval {
    fn of(project: &Value) -> Approval {
        let object = project.as_object();
        Approval {
            enabled: names_in(object, "enabledMcpjsonServers"),
            disabled: names_in(object, "disabledMcpjsonServers"),
            all: flag(object, "enableAllProjectMcpServers"),
            trusted: flag(object, "hasTrustDialogAccepted"),
        }
    }

    /// Whether a `.mcp.json` server of this name is approved: the project is trusted, and the
    /// server is listed or every project server is enabled.
    fn approves(&self, name: &str) -> bool {
        self.trusted && (self.all || self.enabled.iter().any(|listed| listed == name))
    }
}

/// The string items of the array at `key`, or none.
fn names_in(object: Option<&Map<String, Value>>, key: &str) -> Vec<String> {
    object
        .and_then(|object| object.get(key))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the boolean at `key` is `true`.
fn flag(object: Option<&Map<String, Value>>, key: &str) -> bool {
    object
        .and_then(|object| object.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}
