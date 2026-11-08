//! A pool user's loopback port blocks on Windows (0049, "Loopback ports per slot"): slot `s` is
//! pool user `ClauseoSbNN` with `s = NN - 1`, and owns 8 ports in each of three blocks, which
//! set-up's WFP filters give to that user alone.
//!
//! - **H**, the host's listeners (the proxy's agent and browser listeners, the R24 relay):
//!   `46100 + 8s ..= +7`. Only the host binds them; no pool user may listen there.
//! - **P**, the pool user's listeners (Chrome's debugging port, agent-browser's stream server):
//!   `46356 + 8s ..= +7`. Only the slot's own user may listen there.
//! - **D**, agent-browser's Windows daemon: `65024 + 8s ..= +7`, inside Windows' dynamic range
//!   because the daemon's port is a hash of its session name in 49152-65534. No other pool user
//!   may listen there; the lawyer's own programs still may, as Windows hands them any free
//!   dynamic port.
//!
//! The slot's agent may connect to `127.0.0.1` on its own 24 ports and on no other loopback
//! port. The number is the account's, never its place in the sorted pool, so an extend never
//! moves a user's ports.

use std::ops::RangeInclusive;

use crate::windows::pool::{POOL_CAP, pool_name, pool_number};

/// How many ports a slot holds in each block.
pub const SLOT_PORTS: u16 = 8;
/// Block H: every slot's host listeners.
pub const HOST_BLOCK: RangeInclusive<u16> = 46_100..=46_355;
/// Block P: every slot's pool listeners.
pub const POOL_BLOCK: RangeInclusive<u16> = 46_356..=46_611;
/// Block D: every slot's agent-browser daemon port, inside Windows' default dynamic range.
pub const DAEMON_BLOCK: RangeInclusive<u16> = 65_024..=65_279;

/// One pool user's slot: `ClauseoSbNN` is slot `NN - 1`, 0 to `POOL_CAP - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot(u8);

impl Slot {
    /// Slot `index`; `None` at [`POOL_CAP`] or above.
    pub fn new(index: u8) -> Option<Slot> {
        (index < POOL_CAP).then_some(Slot(index))
    }

    /// The slot of the pool user named `name` (`ClauseoSb01` is slot 0); `None` for any other
    /// name.
    pub fn from_account(name: &str) -> Option<Slot> {
        pool_number(name).and_then(|n| Slot::new(n.checked_sub(1)?))
    }

    /// Every slot, in order.
    pub fn all() -> impl Iterator<Item = Slot> {
        (0..POOL_CAP).map(Slot)
    }

    /// Its index, 0 to `POOL_CAP - 1`.
    pub fn index(self) -> u8 {
        self.0
    }

    /// Its pool user's account name, as IT sees it (`ClauseoSb01` for slot 0).
    pub fn account(self) -> String {
        pool_name(self.0 + 1).unwrap_or_default()
    }

    /// Its 8 ports in the block starting at `base`.
    fn ports_from(self, base: u16) -> RangeInclusive<u16> {
        let first = base + SLOT_PORTS * u16::from(self.0);
        first..=first + (SLOT_PORTS - 1)
    }

    /// Its host ports, `H_s`.
    pub fn host(self) -> RangeInclusive<u16> {
        self.ports_from(*HOST_BLOCK.start())
    }

    /// Its pool ports, `P_s`.
    pub fn pool(self) -> RangeInclusive<u16> {
        self.ports_from(*POOL_BLOCK.start())
    }

    /// Its daemon ports, `D_s`: the range agent-browser's session name must hash into.
    pub fn daemon(self) -> RangeInclusive<u16> {
        self.ports_from(*DAEMON_BLOCK.start())
    }

    /// Whether `port` is one of the slot's 24, the only loopback ports its agent may reach.
    pub fn reaches(self, port: u16) -> bool {
        [self.host(), self.pool(), self.daemon()]
            .iter()
            .any(|ports| ports.contains(&port))
    }
}

impl std::fmt::Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "slot {}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slot_is_the_account_number_less_one() {
        assert_eq!(Slot::from_account("ClauseoSb01"), Slot::new(0));
        assert_eq!(Slot::from_account("ClauseoSb17"), Slot::new(16));
        assert_eq!(Slot::from_account("ClauseoSb32"), Slot::new(31));
        assert_eq!(
            Slot::new(29).map(Slot::account).as_deref(),
            Some("ClauseoSb30")
        );
        for slot in Slot::all() {
            assert_eq!(Slot::from_account(&slot.account()), Some(slot));
        }
        for name in [
            "ClauseoSb00",
            "ClauseoSb33",
            "ClauseoSb1",
            "clauseosb01",
            "Other01",
        ] {
            assert_eq!(Slot::from_account(name), None, "{name}");
        }
        assert_eq!(Slot::new(POOL_CAP), None);
    }

    #[test]
    fn each_slot_holds_eight_ports_in_each_block() {
        let first = Slot::new(0).unwrap();
        assert_eq!(first.host(), 46_100..=46_107);
        assert_eq!(first.pool(), 46_356..=46_363);
        assert_eq!(first.daemon(), 65_024..=65_031);
        let fifth = Slot::new(4).unwrap();
        assert_eq!(fifth.host(), 46_132..=46_139);
        assert_eq!(fifth.pool(), 46_388..=46_395);
        assert_eq!(fifth.daemon(), 65_056..=65_063);
        let last = Slot::new(POOL_CAP - 1).unwrap();
        assert_eq!(last.host().end(), HOST_BLOCK.end());
        assert_eq!(last.pool().end(), POOL_BLOCK.end());
        assert_eq!(last.daemon().end(), DAEMON_BLOCK.end());
    }

    #[test]
    fn the_blocks_are_whole_apart_and_clear_of_the_dynamic_range_but_d() {
        assert_eq!(POOL_CAP, 32);
        let total = |block: &RangeInclusive<u16>| block.len();
        for block in [&HOST_BLOCK, &POOL_BLOCK, &DAEMON_BLOCK] {
            assert_eq!(
                total(block),
                usize::from(POOL_CAP) * usize::from(SLOT_PORTS)
            );
        }
        assert_eq!(HOST_BLOCK.end() + 1, *POOL_BLOCK.start());
        assert!(
            *POOL_BLOCK.end() < 49_152,
            "P stays below the dynamic range"
        );
        assert!(*DAEMON_BLOCK.start() >= 49_152 && *DAEMON_BLOCK.end() <= 65_534);
        let mut seen = std::collections::BTreeSet::new();
        for slot in Slot::all() {
            for port in slot.host().chain(slot.pool()).chain(slot.daemon()) {
                assert!(seen.insert(port), "{port} is in two slots");
                assert!(slot.reaches(port));
            }
        }
        assert_eq!(seen.len(), 768);
        let (a, b) = (Slot::new(0).unwrap(), Slot::new(1).unwrap());
        assert!(!a.reaches(*b.pool().start()) && !a.reaches(*b.daemon().end()));
    }
}
