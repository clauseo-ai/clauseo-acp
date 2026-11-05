//! What waits on an agent's next resting S18 state ([`Waiter`]), at most one per agent, each
//! with its own [`Token`]: whoever put a waiter takes back only its own, never one put after it
//! (review H1). A waiter is filled only by a state S18 sent after it was put: when the pump
//! had not yet taken S18's latest state at the put, that state and everything before it are
//! stale, and the waiter is filled only by what comes after it.

use std::collections::BTreeMap;
use std::sync::Mutex;

use clauseo_agents::SigninState;
use clauseo_protocol::AgentId;
use futures::channel::oneshot;

use super::super::compose::is_running;
use super::super::readers::ReadHold;
use crate::bridge::lock;

/// What waits on an agent's next resting S18 state.
pub(crate) enum Waiter {
    /// The leave order, for `stop_using_login`'s end.
    Leave(oneshot::Sender<SigninState>),
    /// `UseFoundLogin`: the hold on the reads, dropped once its end has been handled.
    Found(ReadHold),
}

/// Names the one put that a [`Waiters::take`] may undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Token(u64);

/// A waiter put, taken back when this drops unless the pump filled it first: a leave that
/// ends, or is dropped, early leaves no waiter behind.
pub(crate) struct Put<'a> {
    pub(crate) waiters: &'a Waiters,
    pub(crate) agent: AgentId,
    pub(crate) token: Token,
}

impl Drop for Put<'_> {
    fn drop(&mut self) {
        drop(self.waiters.take(&self.agent, self.token));
    }
}

/// Each agent's [`Waiter`], at most one, and the S18 state the pump last took.
#[derive(Default)]
pub(crate) struct Waiters(Mutex<Inner>);

#[derive(Default)]
struct Inner {
    next: u64,
    waiting: BTreeMap<AgentId, Entry>,
    /// The S18 state the pump last took for each agent, from the moment it took it.
    taken: BTreeMap<AgentId, SigninState>,
}

struct Entry {
    token: Token,
    waiter: Waiter,
    /// S18's latest state at the put, while the pump has not taken it: until it does, every
    /// state the pump takes is from before the put.
    after: Option<SigninState>,
}

impl Waiters {
    /// Waits on `agent`'s next resting state; `latest` is S18's latest state, read just
    /// before. Refused (`None`) while a leave waits for the agent; a `Found` waiter put before
    /// is replaced, its hold dropped.
    pub(crate) fn put(
        &self,
        agent: &AgentId,
        waiter: Waiter,
        latest: Option<SigninState>,
    ) -> Option<Token> {
        let mut inner = lock(&self.0);
        if matches!(
            inner.waiting.get(agent),
            Some(Entry {
                waiter: Waiter::Leave(_),
                ..
            })
        ) {
            return None;
        }
        let after = latest.filter(|latest| inner.taken.get(agent) != Some(latest));
        inner.next += 1;
        let token = Token(inner.next);
        let entry = Entry {
            token,
            waiter,
            after,
        };
        inner.waiting.insert(agent.clone(), entry);
        Some(token)
    }

    /// Takes back the waiter `token` names, if it still waits: never one put after it.
    pub(crate) fn take(&self, agent: &AgentId, token: Token) -> Option<Waiter> {
        let mut inner = lock(&self.0);
        if inner.waiting.get(agent)?.token != token {
            return None;
        }
        inner.waiting.remove(agent).map(|entry| entry.waiter)
    }

    /// The pump took `state` for `agent`: the waiter it fills, if any. A running state fills
    /// none, and neither does a state from before the waiter was put.
    pub(crate) fn deliver(&self, agent: &AgentId, state: &SigninState) -> Option<Waiter> {
        let mut inner = lock(&self.0);
        inner.taken.insert(agent.clone(), state.clone());
        let entry = inner.waiting.get_mut(agent)?;
        if let Some(after) = &entry.after {
            if after == state {
                entry.after = None;
            }
            return None;
        }
        if is_running(state) {
            return None;
        }
        inner.waiting.remove(agent).map(|entry| entry.waiter)
    }
}

#[cfg(test)]
mod tests {
    use clauseo_agents::Found;

    use super::*;

    fn codex() -> AgentId {
        AgentId::new("codex-acp").unwrap()
    }

    fn leave() -> (Waiter, oneshot::Receiver<SigninState>) {
        let (tell, told) = oneshot::channel();
        (Waiter::Leave(tell), told)
    }

    const OUT: SigninState = SigninState::SignedOut { found: None };

    #[test]
    fn a_take_removes_only_the_callers_own_waiter_and_a_second_leave_is_refused() {
        let waiters = Waiters::default();
        let (first, _told) = leave();
        let token = waiters.put(&codex(), first, None).unwrap();
        let (second, _second_told) = leave();
        assert!(waiters.put(&codex(), second, None).is_none());
        assert!(waiters.take(&codex(), Token(token.0 + 1)).is_none());
        assert!(waiters.take(&codex(), token).is_some());
        assert!(waiters.take(&codex(), token).is_none(), "taken once");
    }

    #[test]
    fn a_state_queued_before_the_put_never_fills_the_waiter() {
        let waiters = Waiters::default();
        let split = SigninState::SignedOut {
            found: Some(Found::CodexLinkSplit),
        };
        // S18 sent `OUT`, then `split`; the pump has taken neither when the leave waits.
        let (waiter, _told) = leave();
        let latest = Some(split.clone());
        let _token = waiters.put(&codex(), waiter, latest).unwrap();
        assert!(waiters.deliver(&codex(), &OUT).is_none(), "stale");
        assert!(
            waiters.deliver(&codex(), &split).is_none(),
            "S18's latest at the put"
        );
        assert!(matches!(
            waiters.deliver(&codex(), &OUT),
            Some(Waiter::Leave(_))
        ));
    }

    #[test]
    fn with_the_pump_caught_up_the_next_resting_state_fills_it() {
        let waiters = Waiters::default();
        assert!(waiters.deliver(&codex(), &OUT).is_none());
        let (waiter, _told) = leave();
        let _token = waiters.put(&codex(), waiter, Some(OUT)).unwrap();
        assert!(waiters.deliver(&codex(), &SigninState::Queued).is_none());
        assert!(waiters.deliver(&codex(), &OUT).is_some());
    }
}
