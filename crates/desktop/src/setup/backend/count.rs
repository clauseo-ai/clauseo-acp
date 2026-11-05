//! The usage counts the pump makes (decision record 0048, ruling 9): `agent_install` when an
//! install this session ran ends, and `sign_in` when a sign-in run ends. [`Tally`] follows
//! `Agents`' states and says when one is due; it goes to [`SetupCounts`], which is the app's
//! `CountsState` (nothing when the counts are absent, `counts::AppCounts`).
//!
//! - **`agent_install`**: `Ready` or `Failed` after a `Downloading` or `Installing` this session.
//!   The `Ready` a receipt gives at start, or an install found in place, follows neither, so it
//!   is not counted. A pause, or any other resting state, forgets the run; `Waiting` (offline)
//!   is part of it. `took` runs from the run's first `Downloading` or `Installing`; `tries` is
//!   `Agents::install_tries`, kept to 1 to 3.
//! - **`sign_in`**: `SignedIn` or `Failed` after `Queued`, `Waiting` or `DeviceCode`, billed as
//!   `plan` (every S18 sign-in run is the vendor's plan login). A cancel is `cancelled`. A
//!   status check's `SignedIn`, or a reuse's, follows no running state and is not counted.
//!
//! Every field is a closed value: no key, email, path, account or agent id goes into a count.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use clauseo_agents::{FailureKind, InstallState, SigninCode, SigninState};
use clauseo_counts::schema::{
    Agent, AgentInstall, Billing, Event, InstallResult, SignIn, SignInResult, Took, Tries,
};
use clauseo_protocol::{AgentId, AgentKind};

use crate::setup::compose::is_running;

/// Where the pump's counts go.
pub(crate) trait SetupCounts: Send + Sync {
    /// Counts `event` once, or nothing when the counts are absent.
    fn bump(&self, event: Event);
}

/// The runs under way that a count may end.
#[derive(Debug, Default)]
pub(crate) struct Tally {
    /// Each agent's install run this session: the monotonic reading at its first
    /// `Downloading` or `Installing`.
    installs: BTreeMap<AgentId, Duration>,
    /// The agents whose sign-in run is going.
    signing_in: BTreeSet<AgentId>,
}

impl Tally {
    /// Takes in `agent`'s install `state`, come at monotonic reading `now`, and sends the
    /// `agent_install` count to `counts` when it ends a run. `tries` is asked only then.
    pub(crate) fn install(
        &mut self,
        counts: &dyn SetupCounts,
        agent: &AgentId,
        state: &InstallState,
        now: Duration,
        tries: impl FnOnce() -> Option<u32>,
    ) {
        if let Some(event) = self.install_event(agent, state, now, tries) {
            counts.bump(event);
        }
    }

    /// Takes in `agent`'s sign-in `state`, and sends the `sign_in` count to `counts` when it
    /// ends a run.
    pub(crate) fn signin(
        &mut self,
        counts: &dyn SetupCounts,
        agent: &AgentId,
        state: &SigninState,
    ) {
        if let Some(event) = self.signin_event(agent, state) {
            counts.bump(event);
        }
    }

    /// [`Tally::install`]'s count, if one is due.
    fn install_event(
        &mut self,
        agent: &AgentId,
        state: &InstallState,
        now: Duration,
        tries: impl FnOnce() -> Option<u32>,
    ) -> Option<Event> {
        let result = match state {
            InstallState::Downloading { .. } | InstallState::Installing => {
                self.installs.entry(agent.clone()).or_insert(now);
                return None;
            }
            InstallState::Waiting { .. } => return None,
            InstallState::Ready { .. } => InstallResult::Done,
            InstallState::Failed(failure) => match failure.kind {
                FailureKind::Offline => InstallResult::Offline,
                FailureKind::Blocked => InstallResult::Blocked,
                FailureKind::DiskFull => InstallResult::DiskFull,
                _ => InstallResult::Failed,
            },
            _ => {
                self.installs.remove(agent);
                return None;
            }
        };
        let since = self.installs.remove(agent)?;
        let took = u64::try_from(now.saturating_sub(since).as_millis()).unwrap_or(u64::MAX);
        let tries = tries().unwrap_or(1).clamp(1, 3);
        Some(Event::AgentInstall(AgentInstall {
            agent: counted(agent)?,
            result,
            tries: Tries::new(u8::try_from(tries).ok()?)?,
            took: Took::from_millis(took),
        }))
    }

    /// [`Tally::signin`]'s count, if one is due.
    fn signin_event(&mut self, agent: &AgentId, state: &SigninState) -> Option<Event> {
        if is_running(state) {
            self.signing_in.insert(agent.clone());
            return None;
        }
        let was_running = self.signing_in.remove(agent);
        let result = match state {
            SigninState::SignedIn { .. } => SignInResult::Done,
            SigninState::Failed {
                code: SigninCode::Cancelled,
                ..
            } => SignInResult::Cancelled,
            SigninState::Failed { .. } => SignInResult::Failed,
            _ => return None,
        };
        if !was_running {
            return None;
        }
        Some(Event::SignIn(SignIn {
            agent: counted(agent)?,
            billing: Billing::Plan,
            result,
        }))
    }
}

/// The counts' agent for `agent`; none for an agent the counts do not know.
fn counted(agent: &AgentId) -> Option<Agent> {
    match AgentKind::of(agent)? {
        AgentKind::Claude => Some(Agent::Claude),
        AgentKind::Codex => Some(Agent::Chatgpt),
    }
}
