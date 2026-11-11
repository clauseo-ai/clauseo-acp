//! S18's states on the wire (`SignIn` with `method: Plan`), for [`super::compose`].

use clauseo_agents::{Found, LoginFrom, SigninState, Source};
use clauseo_protocol::{
    AgentId, AgentKind, ErrorCode, FoundLogin, PlanLabel, SignIn, SignInMethod, SignInSource,
};

use super::timestamp;

/// S18's `state` on the wire, a sign-in with `method: Plan`; `None` for a state this version
/// does not know.
pub(crate) fn wire(agent: &AgentId, state: &SigninState) -> Option<SignIn> {
    Some(match state {
        SigninState::SignedOut { found } => SignIn::SignedOut {
            found: found.and_then(found_login),
        },
        SigninState::Checking => SignIn::Checking,
        SigninState::Queued => SignIn::Queued,
        SigninState::Waiting { since, nudge } => SignIn::Waiting {
            since: timestamp(*since),
            nudge: *nudge,
        },
        SigninState::DeviceCode {
            url,
            code,
            expires_at,
        } => SignIn::DeviceCode {
            url: url.clone(),
            code: code.clone(),
            expires_at: timestamp(*expires_at),
        },
        SigninState::LoginFound { from, email } => SignIn::LoginFound {
            from: login_from(*from)?,
            email: email.clone(),
        },
        SigninState::SignedIn {
            email,
            plan,
            source,
            at,
        } => SignIn::SignedIn {
            email: email.clone(),
            plan: plan.as_deref().and_then(|plan| plan_words(agent, plan)),
            method: SignInMethod::Plan,
            source: sign_in_source(*source)?,
            at: timestamp(*at),
            refused: None,
        },
        SigninState::Failed { code, at } => SignIn::Failed {
            code: ErrorCode::from_str_lossy(code.code()),
            at: timestamp(*at),
        },
        _ => return None,
    })
}

/// The plan's words for `agent`'s raw plan string, as `limits` names it; `None` for a plan or
/// an agent it does not know.
fn plan_words(agent: &AgentId, plan: &str) -> Option<PlanLabel> {
    let words = match AgentKind::of(agent)? {
        AgentKind::Claude => clauseo_limits::claude_plan(plan),
        AgentKind::Codex => clauseo_limits::codex_plan(plan),
    };
    words.map(|words| PlanLabel(words.to_owned()))
}

fn found_login(found: Found) -> Option<FoundLogin> {
    Some(match found {
        Found::ClaudeSettings => FoundLogin::ClaudeSettings,
        Found::ClaudeWindowsSandbox => FoundLogin::ClaudeWindowsSandbox,
        Found::CodexKeyring => FoundLogin::CodexKeyring,
        Found::ReusedFailed => FoundLogin::ReusedFailed,
        Found::ReusedSignedOut => FoundLogin::ReusedSignedOut,
        Found::CodexLinkSplit => FoundLogin::CodexLinkSplit,
        Found::NoLogin => FoundLogin::NoneFound,
        Found::ReuseOff => FoundLogin::ReuseOff,
        _ => return None,
    })
}

fn login_from(from: LoginFrom) -> Option<clauseo_protocol::LoginFrom> {
    Some(match from {
        LoginFrom::ClaudeCode => clauseo_protocol::LoginFrom::ClaudeCode,
        LoginFrom::Codex => clauseo_protocol::LoginFrom::Codex,
        _ => return None,
    })
}

fn sign_in_source(source: Source) -> Option<SignInSource> {
    Some(match source {
        Source::Reused => SignInSource::Reused,
        Source::Own => SignInSource::Own,
        _ => return None,
    })
}
