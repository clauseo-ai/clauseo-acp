//! S18's plan, found login, login source and sign-in source in the wire's words, for
//! [`super::wire`].

use clauseo_agents::{Found, LoginFrom, Source};
use clauseo_protocol::{AgentId, AgentKind, FoundLogin, PlanLabel, SignInSource};

/// The plan's words for `agent`'s raw plan string, as `limits` names it; `None` for a plan or
/// an agent it does not know.
pub(super) fn plan_words(agent: &AgentId, plan: &str) -> Option<PlanLabel> {
    let words = match AgentKind::of(agent)? {
        AgentKind::Claude => clauseo_limits::claude_plan(plan),
        AgentKind::Codex => clauseo_limits::codex_plan(plan),
    };
    words.map(|words| PlanLabel(words.to_owned()))
}

pub(super) fn found_login(found: Found) -> Option<FoundLogin> {
    Some(match found {
        Found::ClaudeSettings => FoundLogin::ClaudeSettings,
        Found::ClaudeWindowsSandbox => FoundLogin::ClaudeWindowsSandbox,
        Found::CodexKeyring => FoundLogin::CodexKeyring,
        Found::ReusedFailed => FoundLogin::ReusedFailed,
        Found::ReusedSignedOut => FoundLogin::ReusedSignedOut,
        Found::ReusedChanged => FoundLogin::ReusedChanged,
        Found::CodexLinkSplit => FoundLogin::CodexLinkSplit,
        Found::NoLogin => FoundLogin::NoneFound,
        Found::ReuseOff => FoundLogin::ReuseOff,
        _ => return None,
    })
}

pub(super) fn login_from(from: LoginFrom) -> Option<clauseo_protocol::LoginFrom> {
    Some(match from {
        LoginFrom::ClaudeCode => clauseo_protocol::LoginFrom::ClaudeCode,
        LoginFrom::Codex => clauseo_protocol::LoginFrom::Codex,
        _ => return None,
    })
}

pub(super) fn sign_in_source(source: Source) -> Option<SignInSource> {
    Some(match source {
        Source::Reused => SignInSource::Reused,
        Source::Own => SignInSource::Own,
        _ => return None,
    })
}
