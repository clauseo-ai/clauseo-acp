//! Claude Code's `.claude.json` (user and per-project servers) and each project's `.mcp.json`.

use super::json_entry;
use super::path::{self, join};
use super::{Entry, Env, Source};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Adds Claude Code's servers to `out`: the user-wide `mcpServers`, each `projects.<folder>`
/// entry's `mcpServers`, and each listed folder's `.mcp.json` that exists.
pub(crate) fn read(env: &Env<'_>, out: &mut Vec<Entry>) {
    let config = join(env.os, &path::home(env), ".claude.json");
    let Some(doc) = json_entry::read_json(env, &config) else {
        return;
    };
    json_entry::servers_in(env, &doc, &Source::ClaudeCodeUser, true, out);
    let Some(projects) = doc.get("projects").and_then(Value::as_object) else {
        return;
    };
    for (folder, project) in projects {
        if folder.is_empty() {
            continue;
        }
        let source = Source::ClaudeCodeProject {
            folder: PathBuf::from(folder),
        };
        json_entry::servers_in(env, project, &source, true, out);
        let mcp_json = join(env.os, folder, ".mcp.json");
        if !env.fs.is_file(Path::new(&mcp_json)) {
            continue;
        }
        if let Some(file) = json_entry::read_json(env, &mcp_json) {
            json_entry::servers_in(env, &file, &source, true, out);
        }
    }
}
