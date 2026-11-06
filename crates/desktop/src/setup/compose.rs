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
//!    `ApiKey` or `Firm` line, and a failure is shown. An automatic leave that did not finish
//!    (`leave/auto.rs`) shows `Switching { since, stuck }` between the two: a running sign-in
//!    still shows, and a failure of the leave's own S18 call never does.
//! 2. Otherwise, with policy unreadable and the `firm-policy` marker there (`firm_policy.rs`),
//!    `Failed { code: signin.policy_unreadable }`: the agent is never sent to the plan login.
//! 3. Otherwise, while the agent's stored key could not be read (`setup/keys.rs`), `Failed {
//!    code: agent.key_unreadable }`: its launches are refused until a read succeeds. It bars no
//!    sign-in: a key that may be there is no key in use (the desktop review's item 1).
//! 4. Otherwise `SignedIn { method: Firm }` while the agent has an endpoint or a provider
//!    (`plans.<id>`, the lawyer's or the firm's policy's);
//! 5. else `SignedIn { method: ApiKey }` while a key is in memory (read, or just stored);
//! 6. else S18's state, with `method: Plan` and the plan in words (`limits`' `claude_plan` and
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

use clauseo_agents::{Clock, Found, LoginFrom, SigninCode, SigninState, Source};
use clauseo_protocol::{
    AgentId, AgentKind, ErrorCode, FoundLogin, PlanLabel, SetupEvent, SignIn, SignInMethod,
    SignInSource, Timestamp,
};

use super::SetupEvents;
use super::admit::{SignInBar, sign_in_bar};
use super::keys::KeyCache;
use crate::bridge::lock;

/// What desktop knows of one agent's sign-in besides S18's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Facts {
    /// Since when the agent has an endpoint or a provider, if it has one.
    pub(crate) firm: Option<SystemTime>,
    /// Since when a key is in memory, if one is.
    pub(crate) key: Option<SystemTime>,
    /// Since when the agent's stored key could not be read, if it cannot.
    pub(crate) unreadable: Option<SystemTime>,
    /// Since when policy is unreadable with the `firm-policy` marker there, if it is.
    pub(crate) blocked: Option<SystemTime>,
    /// Since when a firm policy sets the agent's endpoint or provider, if it does.
    pub(crate) managed: Option<SystemTime>,
    /// Since when an automatic leave has been trying again, if one is (its first try's end).
    pub(crate) switching: Option<SystemTime>,
    /// Whether that leave has been trying for 2 minutes (`leave::hold::STUCK_AFTER`).
    pub(crate) stuck: bool,
}

/// The rule: what the windows are told of `agent`'s sign-in, from S18's latest state (`None`
/// before any) and the other facts. `None` when there is nothing to say yet.
pub(crate) fn compose(agent: &AgentId, s18: Option<&SigninState>, facts: Facts) -> Option<SignIn> {
    if let Some(state) = s18
        && is_running(state)
    {
        return wire(agent, state);
    }
    if let Some(since) = facts.switching {
        return Some(SignIn::Switching {
            since: timestamp(since),
            stuck: facts.stuck,
        });
    }
    if let Some(state) = s18
        && matches!(state, SigninState::Failed { .. })
    {
        return wire(agent, state);
    }
    if let Some(since) = facts.blocked {
        return Some(SignIn::Failed {
            code: ErrorCode::SigninPolicyUnreadable,
            at: timestamp(since),
        });
    }
    if let Some(since) = facts.unreadable {
        return Some(SignIn::Failed {
            code: ErrorCode::AgentKeyUnreadable,
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
    /// Whether a job that was being ended was then replaced by a new one before its end came
    /// in: that end, `Failed { Cancelled }`, may still be on its way from before the new job's
    /// first state, and is dropped (not shown, and no starting mark ended), so it never shows
    /// over the new sign-in. The new job's first state ends the watch.
    stale: bool,
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

    /// The clock's monotonic reading, which times the pump's counted runs (`backend/count.rs`).
    pub(crate) fn monotonic(&self) -> std::time::Duration {
        self.clock.monotonic()
    }

    /// S18's new `state` for `agent`. A resting one (any state but a running one) ends a
    /// `StartSignIn`'s starting mark (`SetupEvents::end_start`), even when the composed
    /// state is unchanged and nothing is sent: a reserved job that was cancelled, or replaced
    /// by a `FindLogin` or a `SignOut`, never reports a running state.
    pub(crate) fn signin(&self, agent: &AgentId, state: SigninState) {
        if self.stale_end(agent, &state) {
            tracing::info!(%agent, "a replaced sign-in's late end is dropped");
            return;
        }
        if !is_running(&state) {
            self.events.end_start(agent);
        }
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

    /// `agent`'s job was replaced by a new one (a sign-in, a sign-out or a search). An end
    /// [`Compose::expect_end`] was waiting for is now a stale one, if it comes at all.
    pub(crate) fn replaced(&self, agent: &AgentId) {
        if let Some(known) = lock(&self.agents).get_mut(agent) {
            known.stale = std::mem::take(&mut known.ending);
        }
    }

    /// The lawyer cancels `agent`'s sign-in: its `Failed { Cancelled }` is theirs, never a
    /// stale one.
    pub(crate) fn cancelling(&self, agent: &AgentId) {
        if let Some(known) = lock(&self.agents).get_mut(agent) {
            known.stale = false;
        }
    }

    /// Whether `state` is a replaced job's late end (see `Known::stale`). Any state ends the
    /// watch: the new job's first one comes after the old job's end, if that came at all.
    fn stale_end(&self, agent: &AgentId, state: &SigninState) -> bool {
        let mut agents = lock(&self.agents);
        let Some(known) = agents.get_mut(agent).filter(|known| known.stale) else {
            return false;
        };
        known.stale = false;
        matches!(
            state,
            SigninState::Failed {
                code: SigninCode::Cancelled,
                ..
            }
        )
    }

    /// Whether `state`, the next state S18 reports for `agent`, is an end [`Compose::signin`]
    /// takes as the backend's and not as a failure: a failure of the sign-in a key or an
    /// endpoint ended ([`Compose::expect_end`]), or a replaced job's late `Cancelled`. Read
    /// before `signin` takes the state in.
    pub(crate) fn is_expected_end(&self, agent: &AgentId, state: &SigninState) -> bool {
        let agents = lock(&self.agents);
        let Some(known) = agents.get(agent) else {
            return false;
        };
        let failed = matches!(state, SigninState::Failed { .. });
        let cancelled = matches!(
            state,
            SigninState::Failed {
                code: SigninCode::Cancelled,
                ..
            }
        );
        (known.ending && failed) || (known.stale && cancelled)
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

    /// Whether memory holds a key for `agent`, and whether its stored key could not be read:
    /// one change, so the windows never see one without the other.
    pub(crate) fn set_key_read(&self, agent: &AgentId, in_memory: bool, unreadable: bool) {
        self.change(agent, |known, now| {
            known.facts.key = in_memory.then(|| known.facts.key.unwrap_or(now));
            known.facts.unreadable = unreadable.then(|| known.facts.unreadable.unwrap_or(now));
        });
    }

    /// [`Compose::set_key_read`] as `keys` holds `agent`'s key now: in memory, or unreadable.
    /// Never [`KeyCache::has`], which counts an unreadable key as stored for the launches.
    pub(crate) fn show_key(&self, agent: &AgentId, keys: &KeyCache) {
        self.set_key_read(agent, keys.in_memory(agent), keys.unreadable(agent));
    }

    /// Whether a key is in memory for `agent`, none unreadable: the tests' shorthand.
    #[cfg(test)]
    pub(crate) fn set_key(&self, agent: &AgentId, stored: bool) {
        self.set_key_read(agent, stored, false);
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

    /// An automatic leave of `agent`'s login did not finish and tries again: `Switching`, from
    /// now, unless it shows already (a later try never moves `since`).
    pub(crate) fn start_switching(&self, agent: &AgentId) {
        self.change(agent, |known, now| {
            known.facts.switching.get_or_insert(now);
        });
    }

    /// `agent`'s automatic leave has tried for 2 minutes: `stuck`, while it still shows.
    pub(crate) fn set_stuck(&self, agent: &AgentId) {
        self.change(agent, |known, _| {
            known.facts.stuck = known.facts.switching.is_some();
        });
    }

    /// `agent`'s automatic leave ended (a try succeeded, or a new sign-in arrived).
    pub(crate) fn end_switching(&self, agent: &AgentId) {
        self.change(agent, |known, _| {
            known.facts.switching = None;
            known.facts.stuck = false;
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
