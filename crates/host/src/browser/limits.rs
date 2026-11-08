//! How many browsers run at once (S27, "Concurrency"). The limits each agent-browser gets (idle
//! timeout, default timeout, max output) are the guard's, set in `crates/mcp` alone.
//!
//! A session that got the `browser` entry holds a slot from its start until it ends: Chrome
//! closes after 10 minutes idle (the guard's `AGENT_BROWSER_IDLE_TIMEOUT_MS`), but the slot
//! stays, so a session never finds its browser gone. The host counts at session start
//! (`actor/turn/profile_loads.rs`) and frees the slot when the session's run folder goes
//! (`actor/turn/launch_checks.rs`). Revisit after G17 item 6 measures a session's memory.
//!
//! A slot is freed only after the ended session's removal has waited for its processes, which
//! takes a moment (on Windows at least the removal's short look at Chrome's port). A launch
//! that finds every slot taken while such a removal runs ([`BrowserSlots::releasing`]) waits
//! for it, up to `launch_checks::browser::SLOT_WAIT`, so a session started right after
//! another ended still gets the browser.

use std::collections::BTreeSet;

use crate::config::SessionKey;

/// Machines with this much memory or less get [`SMALL_MACHINE_SLOTS`] browsers at once.
pub const SMALL_MACHINE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Browsers at once on a machine with [`SMALL_MACHINE_BYTES`] or less, or of unknown memory.
pub const SMALL_MACHINE_SLOTS: usize = 2;

/// Browsers at once on a larger machine.
pub const LARGE_MACHINE_SLOTS: usize = 4;

/// Browsers allowed at once on a machine with `total_memory_bytes` of memory.
pub fn slot_limit(total_memory_bytes: u64) -> usize {
    if total_memory_bytes <= SMALL_MACHINE_BYTES {
        SMALL_MACHINE_SLOTS
    } else {
        LARGE_MACHINE_SLOTS
    }
}

/// The browser slots, owned by the actor: the limit and the sessions holding one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserSlots {
    limit: usize,
    held: BTreeSet<SessionKey>,
    /// The holders whose session ended and whose removal runs.
    releasing: BTreeSet<SessionKey>,
}

impl BrowserSlots {
    /// Slots for a machine of `memory` bytes; unknown memory gets the small machine's limit.
    pub(crate) fn new(memory: Option<u64>) -> Self {
        Self {
            limit: memory.map_or(SMALL_MACHINE_SLOTS, slot_limit),
            held: BTreeSet::new(),
            releasing: BTreeSet::new(),
        }
    }

    /// Whether `session` holds a slot or one is free. Takes nothing: [`Self::take`] does,
    /// once the entry is filled.
    pub(crate) fn free_for(&self, session: SessionKey) -> bool {
        self.held.contains(&session) || self.held.len() < self.limit
    }

    /// `session` holds a slot (again, for its `session/load` after a `session/new`).
    pub(crate) fn take(&mut self, session: SessionKey) {
        self.held.insert(session);
    }

    /// `session` ended: its slot is free. A session without one changes nothing.
    pub(crate) fn release(&mut self, session: SessionKey) {
        self.held.remove(&session);
        self.releasing.remove(&session);
    }

    /// `session` ended and its removal started: its slot may come free soon.
    pub(crate) fn begin_release(&mut self, session: SessionKey) {
        if self.held.contains(&session) {
            self.releasing.insert(session);
        }
    }

    /// `session`'s removal ended without freeing it: it keeps its slot. Answers whether it was
    /// releasing.
    pub(crate) fn end_release(&mut self, session: SessionKey) -> bool {
        self.releasing.remove(&session)
    }

    /// Whether a slot is free for a session that holds none.
    pub(crate) fn any_free(&self) -> bool {
        self.held.len() < self.limit
    }

    /// Whether `session` has no slot now but one may come free once a removal that runs ends.
    pub(crate) fn releasing(&self, session: SessionKey) -> bool {
        !self.free_for(session) && !self.releasing.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use clauseo_protocol::{MatterId, ThreadId};

    use super::{BrowserSlots, SessionKey, slot_limit};
    use crate::pipeline::tests::support::id;

    const GB: u64 = 1024 * 1024 * 1024;

    fn key(launch: u64) -> SessionKey {
        SessionKey {
            matter: id::<MatterId>(1),
            thread: id::<ThreadId>(2),
            launch,
        }
    }

    #[test]
    fn two_on_8_gb_or_less_and_four_above() {
        assert_eq!(slot_limit(4 * GB), 2);
        assert_eq!(slot_limit(8 * GB), 2);
        assert_eq!(slot_limit(8 * GB + 1), 4);
        assert_eq!(slot_limit(16 * GB), 4);
        assert_eq!(slot_limit(0), 2);
    }

    #[test]
    fn the_third_session_on_8_gb_has_no_slot_until_one_ends() {
        let mut slots = BrowserSlots::new(Some(8 * GB));
        for launch in [1, 2] {
            assert!(slots.free_for(key(launch)));
            slots.take(key(launch));
        }
        assert!(!slots.free_for(key(3)));
        // A session holding one keeps it, and taking it again counts once.
        assert!(slots.free_for(key(1)));
        slots.take(key(1));
        assert!(!slots.free_for(key(3)));
        slots.release(key(2));
        assert!(slots.free_for(key(3)));
        // Releasing a session that holds none frees nothing.
        slots.take(key(3));
        slots.release(key(9));
        assert!(!slots.free_for(key(4)));
    }

    #[test]
    fn the_fifth_on_16_gb_and_unknown_memory_counts_as_small() {
        let mut slots = BrowserSlots::new(Some(16 * GB));
        for launch in 1..=4 {
            assert!(slots.free_for(key(launch)), "{launch}");
            slots.take(key(launch));
        }
        assert!(!slots.free_for(key(5)));
        let mut unknown = BrowserSlots::new(None);
        unknown.take(key(1));
        unknown.take(key(2));
        assert!(!unknown.free_for(key(3)));
    }
}
