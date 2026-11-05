//! Each agent's `SignInChanged`, composed (decision record 0048, ruling 3): S18 blanks the key
//! variables, so its status sees only the plan login, and desktop says how the agent is signed
//! in. Every `SignInChanged` goes out from [`Compose`], under its one lock, so the windows get
//! them in the order the facts changed (0048's order).
//!
//! The rule is one function, [`compose`]:
//!
//! 1. S18's running states (`Queued`, `Waiting`, `DeviceCode`) and `Failed` pass through as they
//!    are, whatever else holds, so a running sign-in can still be joined (`Setup::start_sign_in`
//!    joins only while the latest state is a running one) and is never restarted under an
//!    `ApiKey` or `Firm` line, and a failure is shown.
//! 2. Otherwise, with policy unreadable and the `firm-policy` marker there (`firm_policy.rs`),
//!    `Failed { code: signin.policy_unreadable }`: the agent is never sent to the plan login.
//! 3. Otherwise `SignedIn { method: Firm }` while the agent has an endpoint or a provider
//!    (`plans.<id>`, the lawyer's or the firm's policy's);
//! 4. else `SignedIn { method: ApiKey }` while a key is stored;
//! 5. else S18's state, with `method: Plan` and the plan in words (`limits`' `claude_plan` and
//!    `codex_plan`, so the line matches `AccountLimits.plan`).
//!
//! Rule 1 is pending the orchestrator's confirmation (the plan's "Disagreements" 7); it is here
//! alone, so a ruling changes one function.
//!
//! A sign-in the backend ends because a key or an endpoint took over ([`Compose::expect_end`])
//! ends without an error: S18's `Failed { code: Cancelled }` for it is taken as `SignedOut`, so
//! the window gets the `ApiKey` or `Firm` line, never "cancelled in the browser". Its running
//! states pass through until then, as rule 1 says.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use clauseo_agents::{Clock, Found, LoginFrom, SigninState, Source};
use clauseo_protocol::{
    AgentId, AgentKind, ErrorCode, FoundLogin, PlanLabel, SetupEvent, SignIn, SignInMethod,
    SignInSource, Timestamp,
};

use super::SetupEvents;
use super::admit::{SignInBar, sign_in_bar};
use crate::bridge::lock;

/// What desktop knows of one agent's sign-in besides S18's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Facts {
    /// Since when the agent has an endpoint or a provider, if it has one.
    pub(crate) firm: Option<SystemTime>,
    /// Since when a key is stored, if one is.
    pub(crate) key: Option<SystemTime>,
    /// Since when policy is unreadable with the `firm-policy` marker there, if it is.
    pub(crate) blocked: Option<SystemTime>,
    /// Since when a firm policy sets the agent's endpoint or provider, if it does.
    pub(crate) managed: Option<SystemTime>,
}

/// The rule: what the windows are told of `agent`'s sign-in, from S18's latest state (`None`
/// before any) and the other facts. `None` when there is nothing to say yet.
pub(crate) fn compose(agent: &AgentId, s18: Option<&SigninState>, facts: Facts) -> Option<SignIn> {
    if let Some(state) = s18
        && is_running_or_failed(state)
    {
        return wire(agent, state);
    }
    if let Some(since) = facts.blocked {
        return Some(SignIn::Failed {
            code: ErrorCode::SigninPolicyUnreadable,
            at: timestamp(since),
        });
    }
    if let Some(since) = facts.managed.or(facts.firm) {
        return Some(signed_in_by(SignInMethod::Firm, since));
    }
    if let Some(since) = facts.key {
        return Some(signed_in_by(SignInMethod::ApiKey, since));
    }
    s18.and_then(|state| wire(agent, state))
}

/// Whether S18's `state` passes through whatever else holds (rule 1).
fn is_running_or_failed(state: &SigninState) -> bool {
    is_running(state) || matches!(state, SigninState::Failed { .. })
}

/// Whether S18's `state` is a running one: `Queued`, `Waiting` or `DeviceCode`.
pub(crate) fn is_running(state: &SigninState) -> bool {
    matches!(
        state,
        SigninState::Queued | SigninState::Waiting { .. } | SigninState::DeviceCode { .. }
    )
}

/// `SignedIn` by a key or the firm's endpoint: no email or plan, Clauseo's own.
fn signed_in_by(method: SignInMethod, since: SystemTime) -> SignIn {
    SignIn::SignedIn {
        email: None,
        plan: None,
        method,
        source: SignInSource::Own,
        at: timestamp(since),
    }
}

/// S18's `state` on the wire, a sign-in with `method: Plan`; `None` for a state this version
/// does not know.
pub(crate) fn wire(agent: &AgentId, state: &SigninState) -> Option<SignIn> {
    Some(match state {
        SigninState::SignedOut { found } => SignIn::SignedOut {
            found: found.and_then(found_login),
        },
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
        Found::ReusedChanged => FoundLogin::ReusedChanged,
        Found::CodexLinkSplit => FoundLogin::CodexLinkSplit,
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

/// `time` as the wire's milliseconds since the epoch; the epoch for a time it cannot carry.
pub(crate) fn timestamp(time: SystemTime) -> Timestamp {
    let millis = match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_millis()).ok(),
        Err(before) => i64::try_from(before.duration().as_millis())
            .ok()
            .map(|millis| -millis),
    };
    millis
        .and_then(|millis| Timestamp::new(millis).ok())
        .unwrap_or(Timestamp::ZERO)
}

/// Each agent's facts and S18 state, and where their composed state goes out.
pub(crate) struct Compose {
    events: Arc<SetupEvents>,
    clock: Arc<dyn Clock>,
    agents: Mutex<BTreeMap<AgentId, Known>>,
}

/// One agent's S18 state, facts and the last state sent.
#[derive(Default)]
struct Known {
    s18: Option<SigninState>,
    facts: Facts,
    sent: Option<SignIn>,
    /// Whether the backend is ending the running sign-in for a key or an endpoint.
    ending: bool,
}

impl Compose {
    /// Composes onto `events`, timing a key or an endpoint set by `clock`.
    pub(crate) fn new(events: Arc<SetupEvents>, clock: Arc<dyn Clock>) -> Self {
        Self {
            events,
            clock,
            agents: Mutex::new(BTreeMap::new()),
        }
    }

    /// S18's new `state` for `agent`.
    pub(crate) fn signin(&self, agent: &AgentId, state: SigninState) {
        self.change(agent, |known, _| {
            let mut state = state;
            if known.ending && !is_running(&state) {
                known.ending = false;
                if matches!(state, SigninState::Failed { .. }) {
                    tracing::info!(%agent, "a sign-in ended: a key or an endpoint took over");
                    state = SigninState::SignedOut { found: None };
                }
            }
            known.s18 = Some(state);
        });
    }

    /// The backend is about to end `agent`'s running sign-in for a key or an endpoint: the
    /// end S18 reports for it is no failure. Undone with [`Compose::no_end`] when the cancel
    /// is refused.
    pub(crate) fn expect_end(&self, agent: &AgentId) {
        lock(&self.agents).entry(agent.clone()).or_default().ending = true;
    }

    /// Undoes [`Compose::expect_end`].
    pub(crate) fn no_end(&self, agent: &AgentId) {
        if let Some(known) = lock(&self.agents).get_mut(agent) {
            known.ending = false;
        }
    }

    /// Whether a firm policy sets `agent`'s endpoint or provider.
    pub(crate) fn set_firm_policy(&self, agent: &AgentId, managed: bool) {
        self.change(agent, |known, now| {
            known.facts.managed = managed.then(|| known.facts.managed.unwrap_or(now));
        });
    }

    /// What bars `agent`'s sign-in now, if anything (`admit::sign_in_bar`).
    pub(crate) fn sign_in_bar(&self, agent: &AgentId) -> Option<SignInBar> {
        let agents = lock(&self.agents);
        let facts = agents
            .get(agent)
            .map(|known| known.facts)
            .unwrap_or_default();
        sign_in_bar(
            facts.managed.is_some(),
            facts.key.is_some(),
            facts.firm.is_some(),
        )
    }

    /// S18's latest state for `agent`, as the composer last heard it.
    pub(crate) fn s18(&self, agent: &AgentId) -> Option<SigninState> {
        lock(&self.agents)
            .get(agent)
            .and_then(|known| known.s18.clone())
    }

    /// Whether a key is stored for `agent`.
    pub(crate) fn set_key(&self, agent: &AgentId, stored: bool) {
        self.change(agent, |known, now| {
            known.facts.key = stored.then(|| known.facts.key.unwrap_or(now));
        });
    }

    /// Whether `agent` has an endpoint or a provider.
    pub(crate) fn set_firm(&self, agent: &AgentId, firm: bool) {
        self.change(agent, |known, now| {
            known.facts.firm = firm.then(|| known.facts.firm.unwrap_or(now));
        });
    }

    /// Whether `agent`'s sign-in is refused for an unreadable policy (`firm_policy.rs`).
    pub(crate) fn set_blocked(&self, agent: &AgentId, blocked: bool) {
        self.change(agent, |known, now| {
            known.facts.blocked = blocked.then(|| known.facts.blocked.unwrap_or(now));
        });
    }

    /// Whether `agent`'s sign-in is refused for an unreadable policy.
    pub(crate) fn blocked(&self, agent: &AgentId) -> bool {
        lock(&self.agents)
            .get(agent)
            .is_some_and(|known| known.facts.blocked.is_some())
    }

    /// Changes `agent`'s facts with `change`, then sends its composed state when it differs
    /// from the last one sent: all under the one lock, so states go out in the order they
    /// changed.
    fn change(&self, agent: &AgentId, change: impl FnOnce(&mut Known, SystemTime)) {
        let now = self.clock.now();
        let mut agents = lock(&self.agents);
        let known = agents.entry(agent.clone()).or_default();
        change(known, now);
        let state = compose(agent, known.s18.as_ref(), known.facts);
        if state.is_none() || state == known.sent {
            return;
        }
        known.sent.clone_from(&state);
        if let Some(state) = state {
            self.events.publish(SetupEvent::SignInChanged {
                agent: agent.clone(),
                state,
            });
        }
    }
}

#[cfg(test)]
mod tests;
