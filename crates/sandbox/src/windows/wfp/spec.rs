// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/wfp/filter_specs.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1); see PROVENANCE.md

//! What tier B's network objects are, with no Windows call, so they build and are tested on
//! every OS (S15 PR7): set-up's persistent block filters and static permits, the owner's
//! read-only grants and the firewall rules ([`NetworkSetup`]), and how each filter maps to WFP's
//! match types and flags ([`Condition::matching`], [`FilterSpec::flags`]).
//!
//! WFP's filter arbitration decides the shape. Within our sublayer the matching filter with the
//! highest weight decides; across sublayers a block wins over a permit that didn't clear its
//! action right. So:
//! - set-up's block filters (weight [`BLOCK_WEIGHT`]) catch every token that holds a pool user's
//!   SID or the pool group's, enabled or deny-only (`sddl.rs`), and block every connect, loopback
//!   and 46100-46399 included, ICMP at resource assignment, and inbound that isn't loopback;
//! - one static permit per pool user (weight [`PERMIT_WEIGHT`]) opens `127.0.0.1` over TCP on
//!   46100-46399 for that user, and nothing else. Within the range the listeners' per-launch
//!   secrets are the cross-matter check (orchestrator ruling, 2026-11-07);
//! - the firewall rules, in Windows Firewall's own sublayer, block what isn't loopback and
//!   loopback TCP outside the range, a second cap on the permits.

use std::net::Ipv4Addr;

use super::check::NetworkGap;
pub use super::rules::{
    Direction, FIREWALL_UNDER_POLICY, FirewallOutcome, FirewallTrouble, LOOPBACK_ADDRESSES,
    NON_LOOPBACK_ADDRESSES, RuleProtocol, RuleSpec, outside_range, rule_names, rule_users,
};
pub use super::sddl::{allowed_users, denied_users, not_pool_sddl, user_sddl};
use crate::ports::TIER_B_PORTS;
use crate::windows::sids::Sid;
use crate::{Loopback, Policy};

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

/// The first static permit's offset from [`Namespace::filters`]: permit `i` (the `i`th pool user,
/// sorted) is `filter_key(PERMIT_BASE + i)`.
pub const PERMIT_BASE: u16 = 0x100;

/// How many static permit keys the key space holds: one per pool index, `0..PERMIT_SLOTS`. So
/// the permits sit at offsets `0x100` to `0x1FF` from [`Namespace::filters`]: clear of the block
/// keys (below [`REMOVAL_KEYS`]) and of the owner probe's stray keys (`0x7F00` and up, in
/// `grant_probe.rs`). Set-up refuses a pool with more users ([`SpecError::TooManyUsers`]), so no
/// index runs past its slot. Raising [`POOL_CAP`](crate::windows::pool::POOL_CAP) to this or
/// less needs no key change, and the keys of existing indices never change.
pub const PERMIT_SLOTS: u16 = 256;

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
    /// ones) and a permit key for every pool index ([`PERMIT_SLOTS`]). Removal and set-up delete
    /// them, and every other filter in our sublayer too; a key nothing wrote is not found, which
    /// is not an error.
    pub fn removal_keys(&self) -> Vec<u128> {
        let blocks = (0..REMOVAL_KEYS).map(|n| self.filter_key(n));
        let permits = (0..PERMIT_SLOTS).map(|i| self.filter_key(PERMIT_BASE + i));
        blocks.chain(permits).collect()
    }
}

/// How many block filter keys removal deletes: today's [`BLOCKS`] and room for later ones. Below
/// [`PERMIT_BASE`], so the two never share a key.
pub(super) const REMOVAL_KEYS: u16 = 32;

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
}

impl Condition {
    /// The match type WFP is given for this condition.
    pub fn matching(&self) -> Match {
        match self {
            Condition::Users(_) | Condition::Protocol(_) | Condition::RemoteV4(_) => Match::Equal,
            Condition::NotPool { .. } => Match::NotEqual,
            Condition::NotLoopback => Match::FlagsNoneSet,
            Condition::RemotePorts(..) => Match::Range,
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
    /// More pool users than the key space has permit slots ([`PERMIT_SLOTS`]): a user past them
    /// would have no key of its own.
    #[error("{0} sandbox users is more than the network filters have keys for")]
    TooManyUsers(usize),
}

/// Set-up's network step: who gets the read grants, the pool group, and the pool users.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSetup {
    owner: Sid,
    group: Sid,
    pool: Vec<Sid>,
}

/// Set-up's block filters, in key order: each one's name, layer and conditions besides the
/// pool's. Appended only, so each keeps its key.
const BLOCKS: [(&str, Layer, Block); 6] = [
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
];

/// A block filter's conditions besides the pool's.
#[derive(Debug, Clone, Copy)]
enum Block {
    All,
    NotLoopback,
    Protocol(u8),
}

impl NetworkSetup {
    /// The step for `owner`'s set-up, with pool group `group`, covering every SID in `pool`,
    /// sorted and de-duplicated so a re-run over the same pool builds the same filters.
    pub fn new(owner: &Sid, group: &Sid, pool: &[Sid]) -> Result<NetworkSetup, SpecError> {
        let mut sorted = pool.to_vec();
        sorted.sort();
        sorted.dedup();
        if sorted.is_empty() {
            return Err(SpecError::NoPoolUser);
        }
        if sorted.len() > usize::from(PERMIT_SLOTS) {
            return Err(SpecError::TooManyUsers(sorted.len()));
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

    /// Set-up's filters, persistent, in key order: the blocks ([`block_filters`]), then one
    /// static permit per pool user ([`permit_filter`]).
    pub fn filters(&self, ns: &Namespace) -> Vec<FilterSpec> {
        let mut filters = block_filters(ns, &self.group, &self.pool);
        filters.extend(
            (0u16..)
                .zip(&self.pool)
                .map(|(i, user)| permit_filter(ns, i, user)),
        );
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

/// The static permit for `user`, the `i`th pool user (sorted): `127.0.0.1` over TCP on
/// [`TIER_B_PORTS`], at `ALE_AUTH_CONNECT_V4` and [`PERMIT_WEIGHT`], persistent. `i` is below
/// [`PERMIT_SLOTS`] for any pool [`NetworkSetup::new`] accepts.
pub fn permit_filter(ns: &Namespace, i: u16, user: &Sid) -> FilterSpec {
    FilterSpec {
        key: ns.filter_key(PERMIT_BASE + i),
        name: format!("{} permit loopback range {user}", ns.display),
        layer: Layer::ConnectV4,
        action: Action::Permit,
        weight: PERMIT_WEIGHT,
        conditions: vec![
            Condition::Users(vec![user.clone()]),
            Condition::RemoteV4(Ipv4Addr::LOCALHOST),
            Condition::Protocol(PROTOCOL_TCP),
            Condition::RemotePorts(*TIER_B_PORTS.start(), *TIER_B_PORTS.end()),
        ],
    }
}

/// The pool index whose static permit has `key` in `ns`, when `key` is a permit slot.
pub fn permit_slot(ns: &Namespace, key: u128) -> Option<u16> {
    (0..PERMIT_SLOTS).find(|&i| ns.filter_key(PERMIT_BASE + i) == key)
}

/// Whether `sid` is an account's or a local group's: `S-1-5-21-<a>-<b>-<c>-<rid>`, never a
/// well-known group.
pub fn is_account(sid: &Sid) -> bool {
    sid.domain().is_some() && sid.as_str().split('-').count() == 8
}

/// The loopback ports a launch names: `policy`'s proxy ports and its [`Loopback::Ports`]
/// ([`checked_ports`]). The static permit opens the whole range, so these narrow nothing at the
/// WFP layer; they are still checked, so a launch never counts on a port the firewall blocks.
/// [`Loopback::Any`] is refused (D10: the browser, the only entry that asks for it, is off on
/// tier B).
pub fn permit_ports(policy: &Policy) -> Result<Vec<u16>, NetworkGap> {
    let mut ports = policy.proxy_ports().to_vec();
    match policy.loopback() {
        Loopback::None => {}
        Loopback::Ports(more) => ports.extend(more),
        Loopback::Any => return Err(NetworkGap::LoopbackAny),
    }
    checked_ports(&ports)
}

/// `ports` sorted and de-duplicated; a port outside [`TIER_B_PORTS`], which the firewall
/// and the permit leave closed, is refused.
pub fn checked_ports(ports: &[u16]) -> Result<Vec<u16>, NetworkGap> {
    let mut ports = ports.to_vec();
    ports.sort_unstable();
    ports.dedup();
    if let Some(port) = ports.iter().find(|port| !TIER_B_PORTS.contains(port)) {
        return Err(NetworkGap::PortOutsideRange(*port));
    }
    Ok(ports)
}

/// What set-up's network step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkReport {
    /// How many filters it wrote: the blocks and one permit per pool user.
    pub filters: usize,
    /// How many filters in our sublayer it deleted that weren't ours.
    pub foreign_removed: usize,
    /// The pool users they cover, sorted.
    pub covered: Vec<Sid>,
    /// The firewall rules.
    pub firewall: FirewallOutcome,
}
