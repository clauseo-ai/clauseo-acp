//! Windows: Claude's fixed `Bash(…)` and `PowerShell(…)` deny rules for Clauseo's secret places
//! under the data folder ([`TARGETS`]), the shell rules a Windows launch sends (decision record
//! 0030, "Claude's permission mode"). Claude runs in `auto` there with no sandbox, so a command
//! goes to Claude Code's classifier unless a rule decides it first. The list does not grow: it
//! holds the same rules whatever the data folder holds (matters, conversations, run folders) and
//! whatever folders the launch walked into; only the bytes of each rule grow with the path.
//!
//! The rules, each once per shell: for each way of writing `<data>` ([`leads`], at most two),
//! each separator after it (`\`, `/`, the lead written with the same one throughout), and each
//! target, Claude's `*`, the lead, the separator and the target's names with Claude's `*` between
//! them (any run or mix of separators, `.\` steps), then `*`: the target and everything under it,
//! and a longer name that starts with it (`settings.json::$DATA`, `state.sqlite-wal`). They are
//! rendered for Claude Code's two readings of a rule as the walked list's are
//! ([`super::render`]).
//!
//! What a command names elsewhere under `<data>` (other matters' notes, other conversations'
//! work folders, other projects' tool results and transcripts, the logs) is in no shell rule: it
//! is Claude Code's classifier's to judge, under the `Read` and `Edit` denies
//! ([`super::super::claude_deny_rules`]) and the host's auto-deny. Not caught either: a
//! configured data folder written through a variable (`%APPDATA%\Clauseo`) or a share path, an
//! 8.3 short name, a `cd` first, a path built at run time.

use super::{Piece, join, render, text};
use crate::agent::data_fence::LaunchDeny;

/// The secret places under `<data>`, as `/`-separated names (a `*` is any text), with why:
/// - `agents/codex-acp`: Codex's home, with its login (`auth.json`) and transcripts.
/// - `agents/signin`: the sign-in shim and its state.
/// - `agents/claude-acp/login-source`: which login Claude's home was given.
/// - `agents/claude-acp/home/.credentials.json`: Claude's login tokens.
/// - `agents/claude-acp/home/.claude.json`: Claude's account state, with an API key when one
///   is set.
/// - `run/*/relay-token`: the case-law relay's token (R24), which `clauseo-mcp` reads itself.
/// - `run/*/proxy-secret`: the session proxy's password file (ADR 0053).
/// - `launch-settings`: every session's Claude settings, with their rules
///   (`agent::launch_settings`).
/// - `settings.json`: Clauseo's settings (`clauseo_settings::SETTINGS_FILE`).
/// - `state.sqlite`: the host's store, every matter's conversations.
/// - `search.sqlite`: the search index, every matter's prompts and answers.
///
/// Not Claude's home whole (its plans and its own project's tool results are its own), nor the
/// agents' runtime (Clauseo's Python is run by name on the `PATH`).
pub(crate) const TARGETS: [&str; 11] = [
    "agents/codex-acp",
    "agents/signin",
    "agents/claude-acp/login-source",
    "agents/claude-acp/home/.credentials.json",
    "agents/claude-acp/home/.claude.json",
    "run/*/relay-token",
    "run/*/proxy-secret",
    crate::agent::launch_settings::FOLDER,
    clauseo_settings::SETTINGS_FILE,
    "state.sqlite",
    "search.sqlite",
];

/// The separators after a lead, each also the one the lead is written with.
const SEPARATORS: [char; 2] = ['\\', '/'];

/// The ways a command writes `deny`'s spellings of `<data>`, at most two, with `\`: Clauseo's
/// own folder name alone (`clauseo_settings::IDENTIFIER`) for the default folder, which every
/// spelling of it ends in (a drive, `%LOCALAPPDATA%`, `~`, a user folder's 8.3 name); otherwise
/// the path in its drive form. A spelling that is not UTF-8 gets none.
pub(crate) fn leads(deny: &LaunchDeny) -> Vec<String> {
    let mut leads = Vec::new();
    for spelling in deny.spellings.iter().filter_map(|path| path.to_str()) {
        let back = crate::network_paths::resolve::drive_form(spelling)
            .unwrap_or_else(|| spelling.replace('/', "\\"));
        let back = back.trim_end_matches('\\');
        let lead = if back.rsplit('\\').next() == Some(clauseo_settings::IDENTIFIER) {
            clauseo_settings::IDENTIFIER
        } else {
            back
        };
        if !lead.is_empty() && !leads.iter().any(|known| known == lead) && leads.len() < 2 {
            leads.push(lead.to_owned());
        }
    }
    leads
}

/// The fixed rules (the module doc) for `deny`'s spellings of `<data>`: at most
/// 2 leads × 2 separators × [`TARGETS`] × 2 shells.
pub(crate) fn windows_fixed_shell_deny_rules(deny: &LaunchDeny) -> Vec<String> {
    let mut contents = Vec::new();
    for lead in leads(deny) {
        for separator in SEPARATORS {
            let lead = lead.replace(['\\', '/'], &separator.to_string());
            for target in TARGETS {
                let mut pieces = join(&[&[Piece::Any], &text(&format!("{lead}{separator}"))]);
                for (n, name) in target.split('/').enumerate() {
                    if n > 0 {
                        pieces.push(Piece::Any);
                    }
                    for (m, part) in name.split('*').enumerate() {
                        if m > 0 {
                            pieces.push(Piece::Any);
                        }
                        if !part.is_empty() {
                            pieces.extend(text(part));
                        }
                    }
                }
                let content = render(&pieces, false);
                if !contents.contains(&content) {
                    contents.push(content);
                }
            }
        }
    }
    ["PowerShell", "Bash"]
        .into_iter()
        .flat_map(|tool| {
            contents
                .iter()
                .map(move |content| format!("{tool}({content})"))
        })
        .collect()
}

#[cfg(test)]
#[path = "fixed_shell_deny_tests.rs"]
mod tests;
