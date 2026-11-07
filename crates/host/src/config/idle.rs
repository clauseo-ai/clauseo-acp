//! The idle stop's settings (R20b, decision record 0052): how long an agent with nothing pending
//! stays live, and how many agents may be live at once on a small machine
//! (`actor/turn/idle.rs`).

use std::time::Duration;

use crate::browser::limits::SMALL_MACHINE_BYTES;

/// How long an agent with nothing pending stays live: 300 s, decided on 2026-11-06 on G3's
/// proposal (`budgets.toml` `agent_idle_stop`).
/// [`HostConfig::with_idle_limit`](crate::HostConfig::with_idle_limit) sets another; tests
/// shorten it, and `Duration::MAX` never stops one.
pub const IDLE_LIMIT: Duration = Duration::from_secs(300);

/// How many agents may be live at once on a machine of 8 GB or less: 2, the design rule
/// `budgets.toml` `live_agents_8gb` holds until G3 item 3 measures 2 and 3 live agents.
pub const LIVE_AGENTS: usize = 2;

/// How long an agent must have been idle before the live-agents cap may stop it for another
/// thread's launch: 60 s, so lawyers moving between three threads do not stop and resume an
/// agent at every switch (decision record 0052).
pub const CAP_MIN_IDLE: Duration = Duration::from_secs(60);

/// The idle stop's settings, as the host config holds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IdleStop {
    /// How long an agent with nothing pending stays live. `Duration::MAX` never stops one.
    pub(crate) limit: Duration,
    /// How many agents may be live at once on a small machine.
    pub(crate) live_agents: usize,
    /// How long an agent must have been idle to be the cap's victim.
    pub(crate) cap_min_idle: Duration,
    /// The machine's total memory in bytes, when the desktop gave it
    /// (`HostConfig::with_machine_memory`).
    pub(crate) memory: Option<u64>,
}

impl Default for IdleStop {
    fn default() -> Self {
        Self {
            limit: IDLE_LIMIT,
            live_agents: LIVE_AGENTS,
            cap_min_idle: CAP_MIN_IDLE,
            memory: None,
        }
    }
}

impl IdleStop {
    /// Whether the live-agents cap applies: on a machine of 8 GB or less, or of unknown memory,
    /// which the browser slots also count as small (`browser/limits.rs`). Above 8 GB there is no
    /// cap in v1 beyond the idle stop.
    pub(crate) fn capped(&self) -> bool {
        self.memory.is_none_or(|bytes| bytes <= SMALL_MACHINE_BYTES)
    }

    /// When an agent idle since `since` (monotonic) reaches the limit; `None` for a limit of
    /// `Duration::MAX`, which never stops one.
    pub(crate) fn deadline(&self, since: Duration) -> Option<Duration> {
        if self.limit == Duration::MAX {
            return None;
        }
        since.checked_add(self.limit)
    }
}

#[cfg(test)]
mod tests {
    use super::{CAP_MIN_IDLE, IDLE_LIMIT, IdleStop, LIVE_AGENTS};

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn the_defaults_are_300_s_2_agents_and_60_s_before_a_cap_stop() {
        let idle = IdleStop::default();
        assert_eq!(idle.limit.as_secs(), 300);
        assert_eq!((idle.limit, idle.live_agents), (IDLE_LIMIT, LIVE_AGENTS));
        assert_eq!(LIVE_AGENTS, 2);
        assert_eq!(idle.cap_min_idle, CAP_MIN_IDLE);
        assert_eq!(CAP_MIN_IDLE.as_secs(), 60);
    }

    #[test]
    fn a_limit_of_duration_max_never_falls() {
        let off = IdleStop {
            limit: std::time::Duration::MAX,
            ..IdleStop::default()
        };
        assert_eq!(off.deadline(std::time::Duration::ZERO), None);
        let on = IdleStop::default().deadline(std::time::Duration::from_secs(1));
        assert_eq!(on, Some(std::time::Duration::from_secs(301)));
    }

    #[test]
    fn the_cap_applies_at_8_gb_or_less_and_with_unknown_memory() {
        let at = |memory| IdleStop {
            memory,
            ..IdleStop::default()
        };
        assert!(at(None).capped());
        assert!(at(Some(8 * GB)).capped());
        assert!(at(Some(4 * GB)).capped());
        assert!(!at(Some(8 * GB + 1)).capped());
        assert!(!at(Some(16 * GB)).capped());
    }
}
