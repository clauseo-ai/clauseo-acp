//! A [`ConnectivityWatch`] the test drives: [`FakeWatch::change`] sends a hint to each live
//! subscription, as the OS would at a change toward connected.

use std::sync::{Arc, Mutex, PoisonError};

use crate::{ConnectivityWatch, Hint, Subscription};

/// A watch whose hints come from the test.
#[derive(Debug, Clone, Default)]
pub struct FakeWatch {
    shared: Arc<Mutex<Subscribers>>,
}

#[derive(Debug, Default)]
struct Subscribers {
    next: u64,
    live: Vec<(u64, Hint)>,
    /// Subscriptions made so far.
    made: usize,
}

impl FakeWatch {
    /// A watch with no subscription.
    pub fn new() -> FakeWatch {
        FakeWatch::default()
    }

    /// Sends a hint to every live subscription; how many it reached.
    pub fn change(&self) -> usize {
        let subscribers = self.lock();
        subscribers
            .live
            .iter()
            .filter(|(_, hint)| hint.send())
            .count()
    }

    /// The live subscriptions.
    pub fn live(&self) -> usize {
        self.lock().live.len()
    }

    /// The subscriptions made so far, ended ones included.
    pub fn made(&self) -> usize {
        self.lock().made
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Subscribers> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ConnectivityWatch for FakeWatch {
    fn subscribe(&self, hint: Hint) -> Option<Subscription> {
        let id = {
            let mut subscribers = self.lock();
            let id = subscribers.next;
            subscribers.next += 1;
            subscribers.made += 1;
            subscribers.live.push((id, hint));
            id
        };
        Some(Subscription::new(Unsubscribe {
            shared: Arc::clone(&self.shared),
            id,
        }))
    }
}

/// Removes its subscription when dropped.
struct Unsubscribe {
    shared: Arc<Mutex<Subscribers>>,
    id: u64,
}

impl Drop for Unsubscribe {
    fn drop(&mut self) {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .live
            .retain(|(id, _)| *id != self.id);
    }
}
