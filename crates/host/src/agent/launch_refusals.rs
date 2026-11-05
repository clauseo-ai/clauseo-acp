//! The refusals desktop puts on an agent id's new launches (decision record 0048,
//! `HostHandle::set_launch_refused` and `HostConfig::with_launch_refused`), in memory only.
//!
//! - The codes come from [`LAUNCH_REFUSALS`]: `signin.policy_unreadable` while an unreadable
//!   policy blocks the agent, and `agent.keys_loading` while desktop has not finished loading
//!   the stored API keys (ruling 4: no agent launches without its key). Another code is refused
//!   with `command.invalid` and changes nothing.
//! - A refusal applies to new launches; a running session ends normally.
//! - `agent.keys_loading` is waited out: a launch it meets waits at most [`KEYS_WAIT`] on the
//!   host's clock ([`LaunchRefusals::wait`]) and is woken as soon as the refusal is lifted or
//!   changed, then the launch checks the refusals again. Desktop holds both facts and sends the
//!   one in force: a policy refusal is never lifted by the keys' load.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use clauseo_protocol::{AgentId, ErrorCode};
use futures::channel::oneshot;
use futures::future::select;

use crate::error::Error;

/// The codes `HostHandle::set_launch_refused` takes: the refusals desktop may put on new
/// launches.
pub(crate) const LAUNCH_REFUSALS: [ErrorCode; 2] = [
    ErrorCode::SigninPolicyUnreadable,
    ErrorCode::AgentKeysLoading,
];

/// How long a launch waits for an `agent.keys_loading` refusal to be lifted.
pub(crate) const KEYS_WAIT: Duration = Duration::from_secs(2);

/// Whether `code` is one a launch may be refused with.
pub(crate) fn taken(code: ErrorCode) -> bool {
    LAUNCH_REFUSALS.contains(&code)
}

/// Each agent's refusal, and the launches waiting for an `agent.keys_loading` one to lift.
#[derive(Default)]
pub(crate) struct LaunchRefusals {
    codes: BTreeMap<AgentId, ErrorCode>,
    waiting: BTreeMap<AgentId, Vec<oneshot::Sender<()>>>,
}

/// A copy holds the same refusals and no waiting launch: those belong to the actor's own.
impl Clone for LaunchRefusals {
    fn clone(&self) -> Self {
        Self {
            codes: self.codes.clone(),
            waiting: BTreeMap::new(),
        }
    }
}

impl std::fmt::Debug for LaunchRefusals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchRefusals")
            .field("codes", &self.codes)
            .field("waiting", &self.waiting.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl LaunchRefusals {
    /// The code `agent`'s new launches are refused with, if any.
    pub(crate) fn get(&self, agent: &AgentId) -> Option<ErrorCode> {
        self.codes.get(agent).copied()
    }

    /// Refuses `agent`'s new launches with `code`, or with `None` lets them start. A code not
    /// on [`LAUNCH_REFUSALS`] is `command.invalid` and changes nothing. Unless the refusal is
    /// `agent.keys_loading` after the call, every launch waiting on it is woken.
    pub(crate) fn set(&mut self, agent: AgentId, code: Option<ErrorCode>) -> Result<(), Error> {
        if let Some(code) = code.filter(|code| !taken(*code)) {
            return Err(Error::refused(
                ErrorCode::CommandInvalid,
                format!("a launch is not refused with {}", code.as_str()),
            ));
        }
        if code != Some(ErrorCode::AgentKeysLoading) {
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

    /// A future that resolves when `agent`'s `agent.keys_loading` refusal is lifted or
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
