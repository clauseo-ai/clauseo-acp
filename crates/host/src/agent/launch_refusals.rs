//! The refusals desktop puts on an agent id's new launches (decision record 0048,
//! `HostHandle::set_launch_refused` and `HostConfig::with_launch_refused`), in memory only.
//!
//! - The codes come from [`LAUNCH_REFUSALS`]: `signin.policy_unreadable` while an unreadable
//!   policy blocks the agent, and `agent.keys_loading` while desktop has not finished loading
//!   the stored API keys (ruling 4: no agent launches without its key). Another code is refused
//!   with `command.invalid` and changes nothing.
//! - A refusal applies to new launches; a running session ends normally.
//!   `agent.key_unreadable` once this agent's stored key was tried and its read failed or did
//!   not answer in time.
//! - `agent.identity_checking` while desktop confirms which account the agent's login belongs
//!   to, `agent.identity_unknown` once that check failed or gave no account, and
//!   `signin.account_changed` once the login belongs to another account than the one signed
//!   in through Clauseo, so its agents were stopped (decision record 0048, R17).
//! - `agent.keys_loading`, `agent.key_unreadable` and `agent.identity_checking` are waited out:
//!   a launch either meets waits at most [`KEYS_WAIT`] on the host's clock
//!   ([`LaunchRefusals::wait`]) and is woken as soon as the refusal is lifted or changed, then
//!   the launch checks the refusals again. Desktop holds the facts and sends the one in force:
//!   a policy refusal is never lifted by the keys' load.
//! - A launch meeting a refusal desktop can act on asks it to, with the code
//!   ([`LaunchRefusals::retry`], `HostConfig::with_launch_retry`), before its wait (R17
//!   generalised it): `agent.key_unreadable`, and desktop reads that key again; a read that
//!   succeeds lifts the refusal, so the launch, or the next, goes on with the key.
//!   `agent.identity_checking`, and desktop runs the identity check when it is due (a
//!   Keychain-only login whose last check is stale, say); the same account lifts it.
//! - `agent.keys_loading` has a bound ([`KEYS_LOAD_BOUND`] from the host's start): past it,
//!   one still in place counts as `agent.key_unreadable`, so a setup thread that never loads
//!   the keys (it did not start, or died) never leaves launches "still starting" for good.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{AgentId, ErrorCode};
use futures::channel::oneshot;
use futures::future::select;

use crate::error::Error;

/// The codes `HostHandle::set_launch_refused` takes: the refusals desktop may put on new
/// launches.
pub(crate) const LAUNCH_REFUSALS: [ErrorCode; 6] = [
    ErrorCode::SigninPolicyUnreadable,
    ErrorCode::AgentKeysLoading,
    ErrorCode::AgentKeyUnreadable,
    ErrorCode::AgentIdentityChecking,
    ErrorCode::AgentIdentityUnknown,
    ErrorCode::SigninAccountChanged,
];

/// The refusals a launch waits out, at most [`KEYS_WAIT`], before it fails with them.
pub(crate) const WAITED_OUT: [ErrorCode; 3] = [
    ErrorCode::AgentKeysLoading,
    ErrorCode::AgentKeyUnreadable,
    ErrorCode::AgentIdentityChecking,
];

/// How long `agent.keys_loading` may stand from the host's start before it counts as
/// `agent.key_unreadable`: desktop's own 10 s bound on the first load, and a margin, since
/// desktop's setup thread starts a little after the host.
pub(crate) const KEYS_LOAD_BOUND: Duration = Duration::from_secs(12);

/// Desktop's hook for a launch that met one of [`RETRIED`], with the agent and the code:
/// read that agent's key again, or run its identity check. Called on the host's actor, so it
/// must not block.
pub(crate) type LaunchRetry = Arc<dyn Fn(&AgentId, ErrorCode) + Send + Sync>;

/// The waited-out refusals a launch asks desktop to act on before its wait.
pub(crate) const RETRIED: [ErrorCode; 2] = [
    ErrorCode::AgentKeyUnreadable,
    ErrorCode::AgentIdentityChecking,
];

/// How long a launch waits for an `agent.keys_loading` refusal to be lifted.
pub(crate) const KEYS_WAIT: Duration = Duration::from_secs(2);

/// Whether `code` is one a launch may be refused with.
pub(crate) fn taken(code: ErrorCode) -> bool {
    LAUNCH_REFUSALS.contains(&code)
}

/// Each agent's refusal, the launches waiting for a waited-out one to lift, desktop's key
/// retry, and when `agent.keys_loading` stops being waited for.
#[derive(Default)]
pub(crate) struct LaunchRefusals {
    codes: BTreeMap<AgentId, ErrorCode>,
    waiting: BTreeMap<AgentId, Vec<oneshot::Sender<()>>>,
    retry: Option<LaunchRetry>,
    /// The host clock's monotonic time past which `agent.keys_loading` counts as
    /// `agent.key_unreadable`; none before the host starts.
    loading_until: Option<Duration>,
}

/// A copy holds the same refusals and no waiting launch: those belong to the actor's own.
impl Clone for LaunchRefusals {
    fn clone(&self) -> Self {
        Self {
            codes: self.codes.clone(),
            waiting: BTreeMap::new(),
            retry: self.retry.clone(),
            loading_until: self.loading_until,
        }
    }
}

impl std::fmt::Debug for LaunchRefusals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchRefusals")
            .field("codes", &self.codes)
            .field("waiting", &self.waiting.keys().collect::<Vec<_>>())
            .field("retry", &self.retry.is_some())
            .field("loading_until", &self.loading_until)
            .finish()
    }
}

impl LaunchRefusals {
    /// The code `agent`'s new launches are refused with, if any.
    pub(crate) fn get(&self, agent: &AgentId) -> Option<ErrorCode> {
        self.codes.get(agent).copied()
    }

    /// The code `agent`'s new launches are refused with at `now` (the host clock's monotonic
    /// time): [`LaunchRefusals::get`], with `agent.keys_loading` past [`KEYS_LOAD_BOUND`]
    /// counted as `agent.key_unreadable`.
    pub(crate) fn in_force(&self, agent: &AgentId, now: Duration) -> Option<ErrorCode> {
        let code = self.get(agent)?;
        let past = self.loading_until.is_some_and(|until| now >= until);
        if code == ErrorCode::AgentKeysLoading && past {
            return Some(ErrorCode::AgentKeyUnreadable);
        }
        Some(code)
    }

    /// Starts [`KEYS_LOAD_BOUND`] at `now`, the host's start on its clock.
    pub(crate) fn start_load_bound(&mut self, now: Duration) {
        self.loading_until = Some(now.saturating_add(KEYS_LOAD_BOUND));
    }

    /// Desktop's launch retry, set before the host starts.
    pub(crate) fn set_retry(&mut self, retry: LaunchRetry) {
        self.retry = Some(retry);
    }

    /// Asks desktop to act on `agent`'s refusal `code`, if it gave a retry and `code` is one
    /// of [`RETRIED`]; any other code asks nothing.
    pub(crate) fn retry(&self, agent: &AgentId, code: ErrorCode) {
        if let Some(retry) = self.retry.as_ref().filter(|_| RETRIED.contains(&code)) {
            retry(agent, code);
        }
    }

    /// Refuses `agent`'s new launches with `code`, or with `None` lets them start. A code not
    /// on [`LAUNCH_REFUSALS`] is `command.invalid` and changes nothing. Every launch waiting
    /// on the refusal is woken, unless it is the same waited-out code after the call.
    pub(crate) fn set(&mut self, agent: AgentId, code: Option<ErrorCode>) -> Result<(), Error> {
        if let Some(code) = code.filter(|code| !taken(*code)) {
            return Err(Error::refused(
                ErrorCode::CommandInvalid,
                format!("a launch is not refused with {}", code.as_str()),
            ));
        }
        let same = code.is_some_and(|code| WAITED_OUT.contains(&code))
            && code == self.codes.get(&agent).copied();
        if !same {
            for waiter in self.waiting.remove(&agent).unwrap_or_default() {
                // A launch that stopped waiting is gone already.
                let _gone = waiter.send(());
            }
        }
        match code {
            Some(code) => self.codes.insert(agent, code),
            None => self.codes.remove(&agent),
        };
        Ok(())
    }

    /// A future that resolves when `agent`'s waited-out refusal is lifted or
    /// changed, or when `timer` (the host clock's [`KEYS_WAIT`], armed by the caller) fires,
    /// whichever comes first.
    pub(crate) fn wait(
        &mut self,
        agent: &AgentId,
        timer: Pin<Box<dyn Future<Output = ()> + Send>>,
    ) -> impl Future<Output = ()> + Send + use<> {
        let (tx, rx) = oneshot::channel();
        let waiters = self.waiting.entry(agent.clone()).or_default();
        waiters.retain(|waiter| !waiter.is_canceled());
        waiters.push(tx);
        async move {
            // Woken, cancelled or timed out: the launch checks the refusals again either way.
            let _first = select(rx, timer).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::task::{Context, Poll};

    use futures::FutureExt as _;
    use futures::task::noop_waker;

    use super::*;

    fn fake() -> AgentId {
        AgentId::new("fake-acp").unwrap()
    }

    fn poll_once(future: &mut Pin<Box<dyn Future<Output = ()> + Send>>) -> Poll<()> {
        let waker = noop_waker();
        future.poll_unpin(&mut Context::from_waker(&waker))
    }

    #[test]
    fn both_codes_are_taken_and_an_unlisted_one_is_command_invalid_changing_nothing() {
        let mut refusals = LaunchRefusals::default();
        refusals
            .set(fake(), Some(ErrorCode::AgentKeysLoading))
            .unwrap();
        assert_eq!(refusals.get(&fake()), Some(ErrorCode::AgentKeysLoading));
        refusals
            .set(fake(), Some(ErrorCode::SigninPolicyUnreadable))
            .unwrap();
        assert_eq!(
            refusals.get(&fake()),
            Some(ErrorCode::SigninPolicyUnreadable)
        );
        let error = refusals
            .set(fake(), Some(ErrorCode::AgentLaunchFailed))
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::CommandInvalid);
        assert_eq!(
            refusals.get(&fake()),
            Some(ErrorCode::SigninPolicyUnreadable)
        );
        refusals.set(fake(), None).unwrap();
        assert_eq!(refusals.get(&fake()), None);
    }

    #[test]
    fn key_unreadable_is_taken_and_an_unlisted_code_is_still_command_invalid() {
        let mut refusals = LaunchRefusals::default();
        refusals
            .set(fake(), Some(ErrorCode::AgentKeyUnreadable))
            .unwrap();
        assert_eq!(refusals.get(&fake()), Some(ErrorCode::AgentKeyUnreadable));
        let error = refusals.set(fake(), Some(ErrorCode::Unknown)).unwrap_err();
        assert_eq!(error.code(), ErrorCode::CommandInvalid);
        assert_eq!(refusals.get(&fake()), Some(ErrorCode::AgentKeyUnreadable));
    }

    #[test]
    fn identity_codes_are_taken_and_an_unlisted_code_is_still_command_invalid() {
        let mut refusals = LaunchRefusals::default();
        for code in [
            ErrorCode::AgentIdentityChecking,
            ErrorCode::AgentIdentityUnknown,
            ErrorCode::SigninAccountChanged,
        ] {
            refusals.set(fake(), Some(code)).unwrap();
            assert_eq!(refusals.get(&fake()), Some(code));
        }
        let error = refusals
            .set(fake(), Some(ErrorCode::AgentLaunchFailed))
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::CommandInvalid);
        assert_eq!(refusals.get(&fake()), Some(ErrorCode::SigninAccountChanged));
    }

    #[test]
    fn the_launch_retry_is_asked_with_its_code_for_key_unreadable_and_identity_checking_alone() {
        let mut refusals = LaunchRefusals::default();
        // No retry given: nothing to ask, and nothing breaks.
        refusals.retry(&fake(), ErrorCode::AgentKeyUnreadable);
        let asked: Arc<std::sync::Mutex<Vec<(AgentId, ErrorCode)>>> = Arc::default();
        let into = Arc::clone(&asked);
        refusals.set_retry(Arc::new(move |agent: &AgentId, code: ErrorCode| {
            into.lock().unwrap().push((agent.clone(), code));
        }));
        for code in [
            ErrorCode::AgentKeyUnreadable,
            ErrorCode::AgentIdentityChecking,
            ErrorCode::AgentKeysLoading,
            ErrorCode::AgentIdentityUnknown,
            ErrorCode::SigninAccountChanged,
            ErrorCode::SigninPolicyUnreadable,
        ] {
            refusals.retry(&fake(), code);
        }
        assert_eq!(
            *asked.lock().unwrap(),
            [
                (fake(), ErrorCode::AgentKeyUnreadable),
                (fake(), ErrorCode::AgentIdentityChecking),
            ]
        );
    }

    #[test]
    fn keys_loading_counts_as_key_unreadable_past_the_load_bound() {
        let mut refusals = LaunchRefusals::default();
        refusals
            .set(fake(), Some(ErrorCode::AgentKeysLoading))
            .unwrap();
        let start = Duration::from_secs(5);
        // No bound before the host starts.
        assert_eq!(
            refusals.in_force(&fake(), start + KEYS_LOAD_BOUND),
            Some(ErrorCode::AgentKeysLoading)
        );
        refusals.start_load_bound(start);
        let almost = start + KEYS_LOAD_BOUND.saturating_sub(Duration::from_millis(1));
        assert_eq!(
            refusals.in_force(&fake(), almost),
            Some(ErrorCode::AgentKeysLoading)
        );
        assert_eq!(
            refusals.in_force(&fake(), start + KEYS_LOAD_BOUND),
            Some(ErrorCode::AgentKeyUnreadable)
        );
        // Another refusal is as it was; a lifted one is gone.
        refusals
            .set(fake(), Some(ErrorCode::SigninPolicyUnreadable))
            .unwrap();
        assert_eq!(
            refusals.in_force(&fake(), start + KEYS_LOAD_BOUND),
            Some(ErrorCode::SigninPolicyUnreadable)
        );
        refusals.set(fake(), None).unwrap();
        assert_eq!(refusals.in_force(&fake(), start + KEYS_LOAD_BOUND), None);
    }

    #[test]
    fn a_launch_waiting_on_keys_loading_wakes_when_it_turns_key_unreadable() {
        let mut refusals = LaunchRefusals::default();
        refusals
            .set(fake(), Some(ErrorCode::AgentKeyUnreadable))
            .unwrap();
        let mut waiting: Pin<Box<dyn Future<Output = ()> + Send>> =
            Box::pin(refusals.wait(&fake(), Box::pin(futures::future::pending())));
        refusals
            .set(fake(), Some(ErrorCode::AgentKeyUnreadable))
            .unwrap();
        assert_eq!(
            poll_once(&mut waiting),
            Poll::Pending,
            "the same code: still waiting"
        );
        refusals
            .set(fake(), Some(ErrorCode::AgentKeysLoading))
            .unwrap();
        assert_eq!(
            poll_once(&mut waiting),
            Poll::Ready(()),
            "a change wakes it"
        );
    }

    #[test]
    fn a_waiting_launch_wakes_when_the_keys_refusal_lifts_and_not_before() {
        let mut refusals = LaunchRefusals::default();
        refusals
            .set(fake(), Some(ErrorCode::AgentKeysLoading))
            .unwrap();
        let mut waiting: Pin<Box<dyn Future<Output = ()> + Send>> =
            Box::pin(refusals.wait(&fake(), Box::pin(futures::future::pending())));
        assert_eq!(poll_once(&mut waiting), Poll::Pending);
        // Set again to the same code: still waiting.
        refusals
            .set(fake(), Some(ErrorCode::AgentKeysLoading))
            .unwrap();
        assert_eq!(poll_once(&mut waiting), Poll::Pending);
        refusals.set(fake(), None).unwrap();
        assert_eq!(poll_once(&mut waiting), Poll::Ready(()));
    }

    #[test]
    fn a_waiting_launch_wakes_when_the_identity_checking_refusal_lifts_and_not_before() {
        let mut refusals = LaunchRefusals::default();
        refusals
            .set(fake(), Some(ErrorCode::AgentIdentityChecking))
            .unwrap();
        let mut waiting: Pin<Box<dyn Future<Output = ()> + Send>> =
            Box::pin(refusals.wait(&fake(), Box::pin(futures::future::pending())));
        assert_eq!(poll_once(&mut waiting), Poll::Pending);
        // Set again to the same code: still waiting.
        refusals
            .set(fake(), Some(ErrorCode::AgentIdentityChecking))
            .unwrap();
        assert_eq!(poll_once(&mut waiting), Poll::Pending);
        refusals.set(fake(), None).unwrap();
        assert_eq!(poll_once(&mut waiting), Poll::Ready(()));
    }

    #[test]
    fn a_waiting_launch_wakes_when_its_timer_fires_or_the_refusal_changes() {
        let mut refusals = LaunchRefusals::default();
        let mut timed: Pin<Box<dyn Future<Output = ()> + Send>> =
            Box::pin(refusals.wait(&fake(), Box::pin(std::future::ready(()))));
        assert_eq!(poll_once(&mut timed), Poll::Ready(()));
        let mut changed: Pin<Box<dyn Future<Output = ()> + Send>> =
            Box::pin(refusals.wait(&fake(), Box::pin(futures::future::pending())));
        refusals
            .set(fake(), Some(ErrorCode::SigninPolicyUnreadable))
            .unwrap();
        assert_eq!(poll_once(&mut changed), Poll::Ready(()));
    }
}
