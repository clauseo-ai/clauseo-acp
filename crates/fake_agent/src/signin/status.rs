//! Claude's `auth status --json`.

use std::io;
use std::path::Path;

use serde_json::json;

use super::{marker, say};
use crate::EXIT_OK;

/// The variables that sign Claude Code in with no login in its folder (2.1.293's token source
/// `Fl` @207422572 takes `ANTHROPIC_AUTH_TOKEN`, then `CLAUDE_CODE_OAUTH_TOKEN`).
const TOKENS: [&str; 2] = ["ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"];

/// `auth status --json`: the signed-in line when the marker is there, else the signed-out line.
/// Both are printed exactly as the brief has them, with the keys in that order. The email is the
/// marker's, `test@example.com` unless a test planted another.
///
/// With no marker, a non-empty one of [`TOKENS`] gives the line Claude Code 2.1.293 prints for
/// it: `loggedIn: true`, `authMethod: "oauth_token"`, no email, exit 0. So a test can tell
/// whether an inherited token reached the run. The token's value is never printed.
pub(super) fn claude_status(folder: &Path) -> io::Result<i32> {
    let token = TOKENS
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|token| !token.is_empty()));
    let line = match marker(folder) {
        Some((method, email)) => format!(
            r#"{{"loggedIn":true,"authMethod":{},"apiProvider":"firstParty","email":{},"subscriptionType":"max"}}"#,
            json!(method),
            json!(email)
        ),
        None if token => {
            r#"{"loggedIn":true,"authMethod":"oauth_token","apiProvider":"firstParty"}"#.to_owned()
        }
        None => r#"{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}"#.to_owned(),
    };
    say(&line)?;
    Ok(EXIT_OK)
}
