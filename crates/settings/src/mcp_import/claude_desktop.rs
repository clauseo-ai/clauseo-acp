//! Claude Desktop's config file.

use super::json_entry;
use super::path::{self, join};
use super::{Entry, Env, Os, Source};

/// Adds the servers of Claude Desktop's config to `out`. Its text is kept as written (no variable
/// expansion), as Claude Desktop reads it.
pub(crate) fn read(env: &Env<'_>, out: &mut Vec<Entry>) {
    let config = match env.os {
        Os::Mac => join(
            Os::Mac,
            &path::home(env),
            "Library/Application Support/Claude/claude_desktop_config.json",
        ),
        Os::Windows => {
            let Some(appdata) = path::var(env, "APPDATA") else {
                return;
            };
            join(Os::Windows, &appdata, "Claude/claude_desktop_config.json")
        }
    };
    let Some(doc) = json_entry::read_json(env, &config) else {
        return;
    };
    json_entry::servers_in(env, &doc, &Source::ClaudeDesktop, false, out);
}
