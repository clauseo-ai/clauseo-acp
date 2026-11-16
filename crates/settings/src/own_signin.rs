//! Whether Claude Code would offer its own Anthropic sign-in, the lawyer's claude.ai OAuth token,
//! to a custom endpoint (decision record 0048, "A firm endpoint needs a key").
//!
//! Claude Code 2.1.293 (`isAnthropicAuthEnabled`) keeps its own sign-in on unless one of these
//! holds, and its client then sends that token as a Bearer to `ANTHROPIC_BASE_URL`, whatever host
//! it names. Re-check the list at each Claude Code bump (`hook-premise-kit/BUMP-RECHECK.md`):
//!
//! - bare mode (`--bare`): Clauseo never passes it;
//! - a unix socket for the API: Clauseo never sets one;
//! - a provider other than first-party: `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX` or
//!   `CLAUDE_CODE_USE_FOUNDRY` truthy (a plan's `provider` sets the first two);
//! - `ANTHROPIC_AUTH_TOKEN` set (a plan's `env` can never set it: `plans.rs`);
//! - an API key (`ANTHROPIC_API_KEY`, the stored key) or an `apiKeyHelper` in its settings.
//!
//! A custom `ANTHROPIC_BASE_URL` turns none of it off. Bare mode, a socket and `apiKeyHelper` are
//! not looked at here: missing them only refuses a launch that was safe.

use std::ffi::OsStr;

use crate::schema::{Plan, PlanProvider};

/// The variables that name a provider other than first-party when truthy.
const PROVIDER_VARS: [&str; 3] = [
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

/// Whether `plan` sends Claude to a custom endpoint with Claude Code's own sign-in still on: an
/// endpoint and the first-party provider (`anthropic` or none). Bedrock and Vertex use the cloud's
/// credentials and turn that sign-in off, so they launch with no key as before.
pub fn plan_keeps_own_signin(plan: &Plan) -> bool {
    plan.endpoint.is_some()
        && !matches!(
            plan.provider,
            Some(PlanProvider::Bedrock | PlanProvider::Vertex)
        )
}

/// Whether a Claude launch with the whole environment `env` would offer Claude Code's own sign-in
/// to a custom endpoint: `ANTHROPIC_BASE_URL` set and not empty, and nothing on the module's list
/// turns that sign-in off. Names match exactly, and in any ASCII case on Windows, as Node reads
/// them there.
pub fn own_signin_reaches_endpoint<'a>(
    env: impl IntoIterator<Item = (&'a OsStr, &'a OsStr)>,
) -> bool {
    let mut base_url = false;
    let mut off = false;
    for (name, value) in env {
        let Some(name) = name.to_str() else {
            continue;
        };
        let is = |wanted: &str| {
            if cfg!(windows) {
                name.eq_ignore_ascii_case(wanted)
            } else {
                name == wanted
            }
        };
        if is("ANTHROPIC_BASE_URL") {
            base_url = !value.is_empty();
        } else if is("ANTHROPIC_API_KEY") || is("ANTHROPIC_AUTH_TOKEN") {
            off |= !value.is_empty();
        } else if PROVIDER_VARS.iter().any(|var| is(var)) {
            off |= truthy(value);
        }
    }
    base_url && !off
}

/// Claude Code's `isEnvTruthy`: `1`, `true`, `yes` or `on`, in any case, spaces around ignored.
fn truthy(value: &OsStr) -> bool {
    value.to_str().is_some_and(|text| {
        let text = text.trim().to_ascii_lowercase();
        matches!(text.as_str(), "1" | "true" | "yes" | "on")
    })
}

#[cfg(test)]
mod tests;
