//! The feeds of no stream a window follows ([`Feed`]): each one's current restart id, and the
//! restarts a pause's end or a watchdog fire made (`WindowState::restart_all`).

use std::collections::BTreeMap;

/// A host feed that belongs to no stream and has no position: restarted, it starts again with
/// the current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Feed {
    /// The app-wide events (`subscribe_app`), which the window asks for.
    App,
    /// Every agent account's limits (`HostHandle::subscribe_limits`), followed from
    /// `open_stream` on.
    Limits,
    /// The install and sign-in events (`setup.rs`, 0048), desktop's own, followed with the
    /// app feed by the windows that may send setup commands.
    Setup,
}

/// A followed feed: its current restart id, and whether a restart is untaken.
#[derive(Debug, Clone, Copy)]
struct Followed {
    restart_id: u64,
    restart_due: bool,
}

/// The feeds a window follows.
#[derive(Debug, Default)]
pub(crate) struct Feeds(BTreeMap<Feed, Followed>);

impl Feeds {
    /// Follows `feed` (or restarts it) under `restart_id`.
    pub(super) fn follow(&mut self, feed: Feed, restart_id: u64) {
        let followed = Followed {
            restart_id,
            restart_due: false,
        };
        self.0.insert(feed, followed);
    }

    /// Stops following `feed` when `restart_id` (one whose host subscription failed) is still
    /// current. Returns whether it did.
    pub(crate) fn unfollow_restart(&mut self, feed: Feed, restart_id: u64) -> bool {
        self.is_current(feed, restart_id) && self.0.remove(&feed).is_some()
    }

    /// Whether `restart_id` is `feed`'s current subscription.
    pub(crate) fn is_current(&self, feed: Feed, restart_id: u64) -> bool {
        self.0
            .get(&feed)
            .is_some_and(|followed| followed.restart_id == restart_id)
    }

    /// `feed`'s current restart id; `None` when the window does not follow it.
    #[cfg(test)]
    pub(crate) fn restart_id(&self, feed: Feed) -> Option<u64> {
        self.0.get(&feed).map(|followed| followed.restart_id)
    }

    /// Each feed a pause's end or the watchdog restarted since the last call, with its restart
    /// id, once: the driver subscribes again, and the feed starts with the current state.
    pub(crate) fn take_restarts(&mut self) -> Vec<(Feed, u64)> {
        let mut due = Vec::new();
        for (feed, followed) in &mut self.0 {
            if followed.restart_due {
                followed.restart_due = false;
                due.push((*feed, followed.restart_id));
            }
        }
        due
    }

    /// Gives every feed a new restart id, counting up from `last_restart_id`, and marks its
    /// restart due.
    pub(super) fn restart_all(&mut self, last_restart_id: &mut u64) {
        for followed in self.0.values_mut() {
            *last_restart_id += 1;
            followed.restart_id = *last_restart_id;
            followed.restart_due = true;
        }
    }
}
