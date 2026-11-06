//! What the key cache and the host tell the setup thread about stored keys (decision record
//! 0048, R20 P5b A4): a [`KeyNote::Read`] when an agent's key became readable or unreadable,
//! so its launch refusal follows; a [`KeyNote::Retry`] when a launch met a refusal it waits
//! out (`HostConfig::with_launch_retry`), with that code: `agent.key_unreadable` reads its key
//! again, `agent.identity_checking` runs its identity check (R17, `setup/refusal.rs`). The
//! channel is made before the host, which takes the retry hook, and its receiving end is taken
//! once by the thread that follows the host (`setup/refusal.rs`). A note names an agent only.

use std::sync::{Arc, Mutex};

use clauseo_protocol::{AgentId, ErrorCode};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::bridge::lock;

/// One note about an agent's stored key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KeyNote {
    /// Its key's unreadable mark changed: its launch refusal is set again.
    Read(AgentId),
    /// A launch met this waited-out refusal: `agent.key_unreadable` (its key is read again,
    /// unless a read runs) or `agent.identity_checking` (its identity check runs when due).
    Retry(AgentId, ErrorCode),
}

/// The host's launch retry: the agent and the code its launch met.
pub(crate) type LaunchRetry = Arc<dyn Fn(&AgentId, ErrorCode) + Send + Sync>;

/// The sending end, shared by the key cache's threads.
pub(crate) type NoteSender = Arc<Mutex<UnboundedSender<KeyNote>>>;

/// The notes' channel: its sending end, and its receiving end until taken.
pub(crate) struct KeyNotes {
    sender: NoteSender,
    receiver: Mutex<Option<UnboundedReceiver<KeyNote>>>,
}

impl KeyNotes {
    /// A new channel.
    pub(crate) fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self {
            sender: Arc::new(Mutex::new(sender)),
            receiver: Mutex::new(Some(receiver)),
        }
    }

    /// The sending end, for the key cache.
    pub(crate) fn sender(&self) -> NoteSender {
        Arc::clone(&self.sender)
    }

    /// The host's launch retry: a [`KeyNote::Retry`] for the agent and the code its launch
    /// met, without waiting.
    pub(crate) fn retry_hook(&self) -> LaunchRetry {
        let sender = self.sender();
        Arc::new(move |agent: &AgentId, code: ErrorCode| {
            send(&sender, KeyNote::Retry(agent.clone(), code));
        })
    }

    /// The receiving end, once.
    pub(crate) fn take(&self) -> Option<UnboundedReceiver<KeyNote>> {
        lock(&self.receiver).take()
    }
}

/// Sends `note` without waiting; with nobody left to read it, it is dropped.
pub(crate) fn send(sender: &NoteSender, note: KeyNote) {
    // Closed only once the setup thread's follower is gone.
    let _gone = lock(sender).unbounded_send(note);
}
