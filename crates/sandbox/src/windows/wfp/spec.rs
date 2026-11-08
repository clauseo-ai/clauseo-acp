// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/wfp/filter_specs.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1); see PROVENANCE.md

//! What tier B's network objects are, with no Windows call, so they build and are tested on
//! every OS (S15 PR7): set-up's persistent block filters and per-slot filters, the owner's
//! read-only grants and the firewall rules ([`NetworkSetup`]), and how each filter maps to WFP's
//! match types and flags ([`Condition::matching`], [`FilterSpec::flags`]).
//!
//! WFP's filter arbitration decides the shape. Within our sublayer the matching filter with the
//! highest weight decides; across sublayers a block wins over a permit that didn't clear its
//! action right. So:
//! - set-up's block filters (weight [`BLOCK_WEIGHT`]) catch every token that holds a pool user's
//!   SID or the pool group's, enabled or deny-only (`sddl.rs`), and block every connect,
//!   loopback included, ICMP at resource assignment, inbound that isn't loopback, and a listen on
//!   any slot's port (blocks H, P and D, `ports::slot`), IPv4 and IPv6;
//! - each pool user's slot gets its own filters (`slot_filters.rs`, 0049 "Loopback ports per
//!   slot"): connects to `127.0.0.1` over TCP on its 24 ports and listens on its pool and daemon
//!   ports permitted (weight [`PERMIT_WEIGHT`]), and a listen on its pool ports blocked for
//!   every token but its own. Nothing opens `::1` or any other loopback address;
//! - the firewall rules, in Windows Firewall's own sublayer, block what isn't loopback, loopback
//!   UDP, and loopback TCP outside the three blocks, a second cap on the permits.

use std::net::Ipv4Addr;

pub use super::rules::{
    Direction, FIREWALL_UNDER_POLICY, FirewallOutcome, FirewallTrouble, LOOPBACK_ADDRESSES,
    NON_LOOPBACK_ADDRESSES, RuleProtocol, RuleSpec, outside_range, rule_names, rule_users,
};
pub use super::sddl::{allowed_users, denied_users, not_pool_sddl, user_sddl};
pub use super::slot_filters::{
    OLD_PERMIT_PORTS, SLOT_BASE, SLOT_FILTERS, SLOT_STRIDE, checked_ports, is_old_permit_key,
    old_permit_filter, permit_ports, slot_filters, slot_key, slots_of,
};
use crate::ports::{DAEMON_BLOCK, HOST_BLOCK, POOL_BLOCK, Slot};
use crate::windows::pool::POOL_CAP;
use crate::windows::sids::Sid;

/// The weight (`FWP_UINT8`) of set-up's block filters in our sublayer.
pub const BLOCK_WEIGHT: u8 = 1;
/// The weight of the static permits: above every block filter, so a permit decides for the
/// traffic it matches.
pub const PERMIT_WEIGHT: u8 = 10;
/// Our sublayer's weight: near the top, as PR0 (c) measured it.
pub const SUBLAYER_WEIGHT: u16 = 0xFFFE;

/// `FWPM_ACTRL_ENUM`: enumerate the container's objects.
pub const ACTRL_ENUM: u32 = 0x20;
/// `FWPM_ACTRL_READ`: read the object.
pub const ACTRL_READ: u32 = 0x80;

/// `IPPROTO_ICMP`.
pub const PROTOCOL_ICMP: u8 = 1;
/// `IPPROTO_TCP`.
pub const PROTOCOL_TCP: u8 = 6;
/// `IPPROTO_ICMPV6`.
pub const PROTOCOL_ICMPV6: u8 = 58;

/// The first of an earlier set-up's shared loopback permits' offsets from
/// [`Namespace::filters`]: permit `i` (the `i`th pool user, sorted) was
/// `filter_key(PERMIT_BASE + i)`. No set-up writes them now; set-up and removal delete them, and a
/// launch that finds one asks for set-up again.
pub const PERMIT_BASE: u16 = 0x100;

/// Where tier B's WFP objects and firewall rules live: the real ones
/// ([`Namespace::production`]), or a test's, so a test never touches a real set-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    /// The provider's key.
    pub provider: u128,
    /// The sublayer's key.
    pub sublayer: u128,
    /// The first block filter's key: block filter `n` is `filters + n`.
    pub filters: u128,
    /// The firewall rules' names start with this.
    pub rules: String,
    /// The name the provider, sublayer, filters and rules show in Windows' tools.
    pub display: String,
}

impl Namespace {
    /// The names a real set-up and every launch use. Never change a key: set-up and
    /// `--remove` find the objects by them.
    pub fn production() -> Namespace {
        Namespace {
            provider: 0xa517_0842_1321_4b98_aa1e_e79c_132f_1c10,
            sublayer: 0x9103_7008_b7c9_4530_b42e_2877_0023_9f22,
            filters: 0x674a_69a4_1c8b_4a2b_9891_6793_c5f9_0000,
            rules: "clauseo_sandbox".to_owned(),
            display: "Clauseo sandbox".to_owned(),
        }
    }

    /// The names the elevated tests use: their own keys and rule names.
    pub fn test() -> Namespace {
        Namespace::numbered_test(0)
    }

    /// Test namespace `n`: its own keys and rule names, so elevated tests that run at once in
    /// separate processes never remove each other's objects. `numbered_test(0)` is
    /// [`Namespace::test`].
    pub fn numbered_test(n: u8) -> Namespace {
        let mark = u128::from(n) << 64;
        let (rules, display) = if n == 0 {
            (
                "clauseo_sandbox_test".to_owned(),
                "Clauseo sandbox (test)".to_owned(),
            )
        } else {
            (
                format!("clauseo_sandbox_test{n}"),
                format!("Clauseo sandbox (test {n})"),
            )
        };
        Namespace {
            provider: 0x05c5_72bc_28f8_4478_b3f6_959b_aa54_104f ^ mark,
            sublayer: 0xf231_bc43_ecf7_4a7f_8143_6bd3_b285_a0d7 ^ mark,
            filters: 0xeeac_d408_d0b5_42b4_be07_b9d7_9d54_0000 ^ mark,
            rules,
            display,
        }
    }

    /// Block filter `n`'s key.
    pub fn filter_key(&self, n: u16) -> u128 {
        self.filters.wrapping_add(u128::from(n))
    }

    /// Every filter key set-up has ever written here: the block keys (with room for later
    /// ones), an earlier set-up's shared permit key per pool user the record can hold, and
    /// every slot's filter keys. Removal and set-up delete them, and every other filter in our
    /// sublayer too.
    pub fn removal_keys(&self) -> Vec<u128> {
        let blocks = (0..REMOVAL_KEYS).map(|n| self.filter_key(n));
        let permits = (0..u16::from(POOL_CAP)).map(|i| self.filter_key(PERMIT_BASE + i));
        let slots = Slot::all().flat_map(|slot| (0..SLOT_FILTERS).map(move |n| (slot, n)));
        let slots = slots.map(|(slot, n)| slot_key(self, slot, n));
        blocks.chain(permits).chain(slots).collect()
    }
}

/// How many block filter keys removal deletes: today's [`BLOCKS`] and room for later ones.
const REMOVAL_KEYS: u16 = 32;

/// A WFP layer a filter sits at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// `FWPM_LAYER_ALE_AUTH_CONNECT_V4`: outbound connects, and the first packet of each UDP
    /// or ICMP flow.
    ConnectV4,
    /// `FWPM_LAYER_ALE_AUTH_CONNECT_V6`.
    ConnectV6,
    /// `FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4`: inbound connections and flows.
    RecvAcceptV4,
    /// `FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V6`.
    RecvAcceptV6,
    /// `FWPM_LAYER_ALE_RESOURCE_ASSIGNMENT_V4`: a raw socket's bind (ICMP).
    AssignV4,
    /// `FWPM_LAYER_ALE_RESOURCE_ASSIGNMENT_V6`.
    AssignV6,
    /// `FWPM_LAYER_ALE_AUTH_LISTEN_V4`: a TCP socket's `listen`. Never resource assignment,
    /// which every outbound connect's source port also passes.
    ListenV4,
    /// `FWPM_LAYER_ALE_AUTH_LISTEN_V6`, dual-stack sockets included.
    ListenV6,
}

/// What a filter does with what it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `FWP_ACTION_BLOCK`, with its action right cleared.
    Block,
    /// `FWP_ACTION_PERMIT`, soft: a block in another sublayer still wins.
    Permit,
}

/// How a condition compares (`FWP_MATCH_TYPE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Match {
    /// `FWP_MATCH_EQUAL`; for a descriptor, true when an access check grants `CC`.
    Equal,
    /// `FWP_MATCH_NOT_EQUAL`; for a descriptor, true when an access check doesn't grant it.
    NotEqual,
    /// `FWP_MATCH_FLAGS_NONE_SET`.
    FlagsNoneSet,
    /// `FWP_MATCH_RANGE`.
    Range,
}

/// One condition of a filter; a filter matches when all of its conditions do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// `FWPM_CONDITION_ALE_USER_ID`, `EQUAL`, over [`user_sddl`]: the token is one of these
    /// users, its SID enabled. Under design (B) a pool user's SID is both its token's user and a
    /// restricting SID, so the agent's restricted token passes both checks on it.
    Users(Vec<Sid>),
    /// `FWPM_CONDITION_ALE_USER_ID`, `NOT_EQUAL`, over [`not_pool_sddl`]: the token holds the
    /// group's or a pool user's SID, enabled or deny-only.
    NotPool {
        /// The pool group, `ClauseoSandboxUsers`.
        group: Sid,
        /// Every pool user.
        pool: Vec<Sid>,
    },
    /// `FWPM_CONDITION_FLAGS` has no `FWP_CONDITION_FLAG_IS_LOOPBACK`.
    NotLoopback,
    /// `FWPM_CONDITION_IP_PROTOCOL` is this.
    Protocol(u8),
    /// `FWPM_CONDITION_IP_REMOTE_ADDRESS` is this IPv4 address.
    RemoteV4(Ipv4Addr),
    /// `FWPM_CONDITION_IP_REMOTE_PORT` is in this range, both ends included.
    RemotePorts(u16, u16),
    /// `FWPM_CONDITION_IP_LOCAL_PORT` is in this range, both ends included: at a listen layer,
    /// the port listened on, whatever the address (`0.0.0.0`, `::`, `::1`, any `127.x`).
    LocalPorts(u16, u16),
    /// `FWPM_CONDITION_ALE_USER_ID`, `NOT_EQUAL`, over [`user_sddl`]: the token is none of these
    /// users, its SID enabled. Every other token matches: other pool users, the lawyer's,
    /// services'.
    NotUsers(Vec<Sid>),
}

impl Condition {
    /// The match type WFP is given for this condition.
    pub fn matching(&self) -> Match {
        match self {
            Condition::Users(_) | Condition::Protocol(_) | Condition::RemoteV4(_) => Match::Equal,
            Condition::NotPool { .. } | Condition::NotUsers(_) => Match::NotEqual,
            Condition::NotLoopback => Match::FlagsNoneSet,
            Condition::RemotePorts(..) | Condition::LocalPorts(..) => Match::Range,
        }
    }
}

/// The flags a filter is added with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags {
    /// `FWPM_FILTER_FLAG_PERSISTENT`.
    pub persistent: bool,
    /// `FWPM_FILTER_FLAG_CLEAR_ACTION_RIGHT`.
    pub clear_action_right: bool,
}

/// One filter to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterSpec {
    /// Its fixed key.
    pub key: u128,
    /// Its name in Windows' tools.
    pub name: String,
    /// Its layer.
    pub layer: Layer,
    /// Its action.
    pub action: Action,
    /// Its weight in our sublayer.
    pub weight: u8,
    /// Its conditions.
    pub conditions: Vec<Condition>,
}

impl FilterSpec {
    /// Its flags: every filter of ours is persistent; a block clears its action right, so no
    /// other sublayer's soft permit beats it; a permit stays soft, so another sublayer's block
    /// (the firewall's range cap) still wins.
    pub fn flags(&self) -> Flags {
        Flags {
            persistent: true,
            clear_action_right: self.action == Action::Block,
        }
    }
}

/// A WFP object whose security descriptor set-up changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Object {
    /// Our sublayer.
    Sublayer,
    /// The engine's filter container (the filters' security, key `NULL`).
    FilterContainer,
    /// Our provider.
    Provider,
    /// One of set-up's filters, by key.
    Filter(u128),
}

/// One allow entry set-up adds for the owner on an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    /// The object.
    pub object: Object,
    /// The rights (`FWPM_ACTRL_*`): read-only, never add, link or delete.
    pub mask: u32,
}

/// Why set-up's network inputs were refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpecError {
    /// No pool user: a filter naming nobody would match nobody.
    #[error("there is no sandbox user to filter")]
    NoPoolUser,
    /// A SID that isn't a local or domain account's (`S-1-5-21-<a>-<b>-<c>-<rid>`): a
    /// well-known group such as `Users` or `Everyone` would filter the lawyer's own programs.
    #[error("{0} isn't a sandbox user's or group's SID")]
    NotAnAccount(Sid),
    /// The owner is named as a pool user or the group: the lawyer's own programs would lose the
    /// network.
    #[error("the owner, {0}, is named as a sandbox user or group")]
    OwnerInPool(Sid),
    /// A pool user whose account name isn't `ClauseoSbNN`, so it has no slot.
    #[error("the sandbox user {0} isn't named ClauseoSb01 to ClauseoSb32")]
    NoSlot(String),
    /// Two pool users with one slot, or one user with two.
    #[error("the sandbox users' loopback slots clash at {0}")]
    SlotClash(Slot),
}

/// Set-up's network step: who gets the read grants, the pool group, and the pool users.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSetup {
    owner: Sid,
    group: Sid,
    pool: Vec<Sid>,
    slots: Vec<(Slot, Sid)>,
}

/// Set-up's block filters, in key order: each one's name, layer and conditions besides the
/// pool's. Appended only, so each keeps its key.
const BLOCKS: [(&str, Layer, Block); 12] = [
    ("block_connect_v4", Layer::ConnectV4, Block::All),
    ("block_connect_v6", Layer::ConnectV6, Block::All),
    ("block_inbound_v4", Layer::RecvAcceptV4, Block::NotLoopback),
    ("block_inbound_v6", Layer::RecvAcceptV6, Block::NotLoopback),
    (
        "block_icmp_v4",
        Layer::AssignV4,
        Block::Protocol(PROTOCOL_ICMP),
    ),
    (
        "block_icmp_v6",
        Layer::AssignV6,
        Block::Protocol(PROTOCOL_ICMPV6),
    ),
    (
        "block_listen_host_v4",
        Layer::ListenV4,
        Block::Listen(HOST_BLOCK),
    ),
    (
        "block_listen_host_v6",
        Layer::ListenV6,
        Block::Listen(HOST_BLOCK),
    ),
    (
        "block_listen_pool_v4",
        Layer::ListenV4,
        Block::Listen(POOL_BLOCK),
    ),
    (
        "block_listen_pool_v6",
        Layer::ListenV6,
        Block::Listen(POOL_BLOCK),
    ),
    (
        "block_listen_daemon_v4",
        Layer::ListenV4,
        Block::Listen(DAEMON_BLOCK),
    ),
    (
        "block_listen_daemon_v6",
        Layer::ListenV6,
        Block::Listen(DAEMON_BLOCK),
    ),
];

/// A block filter's conditions besides the pool's.
#[derive(Debug, Clone)]
enum Block {
    All,
    NotLoopback,
    Protocol(u8),
    Listen(std::ops::RangeInclusive<u16>),
}

impl NetworkSetup {
    /// The step for `owner`'s set-up, with pool group `group`, covering every pool user in
    /// `pool` with its slot ([`slots_of`] reads them from the account names), sorted and
    /// de-duplicated so a re-run over the same pool builds the same filters. A slot named for
    /// two users, or a user with two slots, is refused.
    pub fn new(owner: &Sid, group: &Sid, pool: &[(Slot, Sid)]) -> Result<NetworkSetup, SpecError> {
        let mut slots = pool.to_vec();
        slots.sort();
        slots.dedup();
        for pair in slots.windows(2) {
            if let [(a, _), (b, _)] = pair
                && a == b
            {
                return Err(SpecError::SlotClash(*a));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        if let Some((slot, _)) = slots.iter().find(|(_, sid)| !seen.insert(sid.clone())) {
            return Err(SpecError::SlotClash(*slot));
        }
        let mut sorted: Vec<Sid> = seen.into_iter().collect();
        sorted.sort();
        if sorted.is_empty() {
            return Err(SpecError::NoPoolUser);
        }
        for sid in sorted.iter().chain(std::iter::once(group)) {
            if !is_account(sid) {
                return Err(SpecError::NotAnAccount(sid.clone()));
            }
            if sid == owner {
                return Err(SpecError::OwnerInPool(sid.clone()));
            }
        }
        Ok(NetworkSetup {
            owner: owner.clone(),
            group: group.clone(),
            pool: sorted,
            slots,
        })
    }

    /// The owner: the desktop user whose launches read the filters.
    pub fn owner(&self) -> &Sid {
        &self.owner
    }

    /// The pool users the filters and rules cover, sorted.
    pub fn pool(&self) -> &[Sid] {
        &self.pool
    }

    /// Each pool user's slot, by slot.
    pub fn slots(&self) -> &[(Slot, Sid)] {
        &self.slots
    }

    /// Set-up's filters, persistent, in key order: the blocks ([`block_filters`]), then each
    /// pool user's slot's ([`slot_filters`]), by slot.
    pub fn filters(&self, ns: &Namespace) -> Vec<FilterSpec> {
        let mut filters = block_filters(ns, &self.group, &self.pool);
        for (slot, user) in &self.slots {
            filters.extend(slot_filters(ns, *slot, user));
        }
        filters
    }

    /// The owner's grants, read-only (orchestrator ruling, 2026-11-07): `READ|ENUM` on the
    /// filter container and `READ` on our provider, sublayer and each of our filters, so a
    /// launch can check them. No add, link or delete anywhere.
    pub fn grants(&self, ns: &Namespace) -> Vec<Grant> {
        let mut grants = vec![
            Grant {
                object: Object::FilterContainer,
                mask: ACTRL_READ | ACTRL_ENUM,
            },
            Grant {
                object: Object::Sublayer,
                mask: ACTRL_READ,
            },
            Grant {
                object: Object::Provider,
                mask: ACTRL_READ,
            },
        ];
        grants.extend(self.filters(ns).into_iter().map(|filter| Grant {
            object: Object::Filter(filter.key),
            mask: ACTRL_READ,
        }));
        grants
    }

    /// The firewall rules for the pool ([`rules::rules`](super::rules::rules)).
    pub fn rules(&self, ns: &Namespace) -> Vec<RuleSpec> {
        super::rules::rules(ns, &self.pool)
    }
}

/// The objects whose owner grant removal takes back; the sublayer, the provider and the
/// filters go with their deletion.
pub fn revocations() -> [Object; 1] {
    [Object::FilterContainer]
}

/// Set-up's block filters in `ns` for `group` and `pool`, persistent, in key order.
pub fn block_filters(ns: &Namespace, group: &Sid, pool: &[Sid]) -> Vec<FilterSpec> {
    (0u16..)
        .zip(BLOCKS)
        .map(|(n, (name, layer, block))| {
            let mut conditions = vec![Condition::NotPool {
                group: group.clone(),
                pool: pool.to_vec(),
            }];
            match block {
                Block::All => {}
                Block::NotLoopback => conditions.push(Condition::NotLoopback),
                Block::Protocol(protocol) => conditions.push(Condition::Protocol(protocol)),
                Block::Listen(ports) => {
                    conditions.push(Condition::LocalPorts(*ports.start(), *ports.end()));
                }
            }
            FilterSpec {
                key: ns.filter_key(n),
                name: format!("{} {name}", ns.display),
                layer,
                action: Action::Block,
                weight: BLOCK_WEIGHT,
                conditions,
            }
        })
        .collect()
}

/// How many block filters set-up writes.
pub const BLOCK_COUNT: usize = BLOCKS.len();

/// Whether `sid` is an account's or a local group's: `S-1-5-21-<a>-<b>-<c>-<rid>`, never a
/// well-known group.
pub fn is_account(sid: &Sid) -> bool {
    sid.domain().is_some() && sid.as_str().split('-').count() == 8
}

/// What set-up's network step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkReport {
    /// How many filters it wrote: the blocks and each pool user's slot's.
    pub filters: usize,
    /// How many filters in our sublayer it deleted that weren't ours.
    pub foreign_removed: usize,
    /// The pool users they cover, sorted.
    pub covered: Vec<Sid>,
    /// The firewall rules.
    pub firewall: FirewallOutcome,
}
