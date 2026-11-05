//! The login splits' channel (decision record 0048, P4b; the host code owner's review of #440).
//! It keeps the latest split per agent: a split for an agent whose last one is still unread
//! replaces it in its place, so nothing is lost and nothing backs up however slowly desktop
//! reads. Splits of different agents come out in the order each agent's first unread one
//! went in. By the time a split is sent the host has set its own reused flag for the agent to
//! false, so the latest split is all desktop needs to act on.

use std::sync::{Arc, Mutex, PoisonError};

pub use async_channel::{RecvError, TryRecvError};

use super::login_link::LoginSplit;

/// The unread splits, at most one per agent.
type Pending = Arc<Mutex<Vec<LoginSplit>>>;

/// The sending end, on the actor.
#[derive(Debug, Clone)]
pub(crate) struct SplitSender {
    pending: Pending,
    /// One token while a split may be unread; readers wait on it.
    wake: async_channel::Sender<()>,
}

/// The receiving end, [`HostHandle::login_splits`](crate::HostHandle::login_splits): the
/// latest split per agent. Every clone takes from the same splits, so one reader takes each.
#[derive(Debug, Clone)]
pub struct LoginSplits {
    pending: Pending,
    wake: async_channel::Receiver<()>,
}

/// A new channel's two ends.
pub(crate) fn channel() -> (SplitSender, LoginSplits) {
    let pending = Pending::default();
    let (wake, woken) = async_channel::bounded(1);
    let sender = SplitSender {
        pending: Arc::clone(&pending),
        wake,
    };
    (
        sender,
        LoginSplits {
            pending,
            wake: woken,
        },
    )
}

impl SplitSender {
    /// Sends `split` without waiting, in place of its agent's unread one if there is one.
    pub(crate) fn send(&self, split: LoginSplit) {
        {
            let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
            match pending
                .iter_mut()
                .find(|unread| unread.agent == split.agent)
            {
                Some(unread) => *unread = split,
                None => pending.push(split),
            }
        }
        // Full: a token already waits, and its reader finds this split too.
        let _woken = self.wake.try_send(());
    }
}

impl LoginSplits {
    /// The oldest agent's latest split, waiting for one. Fails once the host is gone and no
    /// split is unread.
    pub async fn recv(&self) -> Result<LoginSplit, RecvError> {
        loop {
            if let Some(split) = self.take() {
                return Ok(split);
            }
            if self.wake.recv().await.is_err() {
                return self.take().ok_or(RecvError);
            }
        }
    }

    /// The oldest agent's latest split, without waiting: `Empty` when none is unread,
    /// `Closed` when the host is gone too.
    pub fn try_recv(&self) -> Result<LoginSplit, TryRecvError> {
        match self.take() {
            Some(split) => Ok(split),
            None if self.wake.is_closed() => Err(TryRecvError::Closed),
            None => Err(TryRecvError::Empty),
        }
    }

    fn take(&self) -> Option<LoginSplit> {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        (!pending.is_empty()).then(|| pending.remove(0))
    }
}

#[cfg(test)]
mod tests;
