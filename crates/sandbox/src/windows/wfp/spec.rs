// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/wfp/filter_specs.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1); see PROVENANCE.md

//! What tier B's network objects are, with no Windows call, so they build and are tested on
//! every OS (S15 PR7): set-up's persistent block filters, its delegation grants and its firewall
//! rules ([`NetworkSetup`]), and a launch's dynamic permit ([`permit_filters`]).
//!
//! WFP's filter arbitration decides the shape. Within our sublayer the matching filter with the
//! highest weight decides; across sublayers a block wins over a permit that didn't clear its
//! action right. So:
//! - set-up's block filters (weight [`BLOCK_WEIGHT`]) name every pool user and block every
//!   connect, loopback and the 61080-61179 range included, ICMP at resource assignment, and
//!   any inbound connection that isn't loopback;
//! - a launch's permit (weight [`PERMIT_WEIGHT`]) opens `127.0.0.1` over TCP to its own ports
//!   for its own pool user, and nothing else;
//! - the firewall rules, in Windows Firewall's own sublayer, block what isn't loopback and
//!   loopback TCP outside the range. They are where the range is excepted: our soft permit
//!   can't beat a firewall block, so no permit ever opens a port outside the range.
//!
//! The range is not excepted in the block filters themselves: a block that left 61080-61179
//! open would let matter B's agent reach matter A's proxy and relay ports, since the permit
//! only adds and never narrows.

use std::net::Ipv4Addr;

use super::check::NetworkGap;
pub use super::rules::{
    Direction, FIREWALL_UNDER_POLICY, FirewallOutcome, FirewallTrouble, LOOPBACK_ADDRESSES,
    NON_LOOPBACK_ADDRESSES, RuleProtocol, RuleSpec, outside_range, rule_names, rule_users,
};
use crate::ports::WINDOWS_PORTS;
use crate::windows::sids::Sid;
use crate::{Loopback, Policy};

/// The weight (`FWP_UINT8`) of set-up's block filters in our sublayer.
pub const BLOCK_WEIGHT: u8 = 1;
/// The weight of a launch's permit filters: above every block filter, so a permit decides for
/// the traffic it matches.
pub const PERMIT_WEIGHT: u8 = 10;
/// Our sublayer's weight: near the top, as PR0 (c) measured it.
pub const SUBLAYER_WEIGHT: u16 = 0xFFFE;

/// `FWPM_ACTRL_ADD`: add an object to a container.
pub const ACTRL_ADD: u32 = 0x1;
/// `FWPM_ACTRL_ADD_LINK`: add a filter that links to this sublayer or layer.
pub const ACTRL_ADD_LINK: u32 = 0x2;
/// `FWPM_ACTRL_READ`: read the object.
pub const ACTRL_READ: u32 = 0x80;

/// `IPPROTO_ICMP`.
pub const PROTOCOL_ICMP: u8 = 1;
/// `IPPROTO_TCP`.
pub const PROTOCOL_TCP: u8 = 6;
/// `IPPROTO_ICMPV6`.
pub const PROTOCOL_ICMPV6: u8 = 58;

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

    /// Every block filter key set-up has ever written here: what removal deletes. Keys past
    /// the filters of today are kept free for filters added later, and removal deletes them
    /// too.
    pub fn removal_keys(&self) -> Vec<u128> {
        (0..REMOVAL_KEYS).map(|n| self.filter_key(n)).collect()
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
}

/// What a filter does with what it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `FWP_ACTION_BLOCK`, with its action right cleared (`FWPM_FILTER_FLAG_CLEAR_ACTION_RIGHT`).
    Block,
    /// `FWP_ACTION_PERMIT`, soft: a block in another sublayer still wins.
    Permit,
}

/// One condition of a filter; a filter matches when all of its conditions do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// `FWPM_CONDITION_ALE_USER_ID` against a security descriptor that grants
    /// `FWP_ACTRL_MATCH_FILTER` to each of these SIDs ([`user_sddl`]). Under design (B) a pool
    /// user's SID is both its token's user and a restricting SID, so an access check of the
    /// agent's restricted token passes on that one SID (PR0 (c); the first VM row confirms).
    Users(Vec<Sid>),
    /// `FWPM_CONDITION_FLAGS` has no `FWP_CONDITION_FLAG_IS_LOOPBACK`.
    NotLoopback,
    /// `FWPM_CONDITION_IP_PROTOCOL` is this.
    Protocol(u8),
    /// `FWPM_CONDITION_IP_REMOTE_ADDRESS` is this IPv4 address.
    RemoteV4(Ipv4Addr),
    /// `FWPM_CONDITION_IP_REMOTE_PORT` is this.
    RemotePort(u16),
}

/// One filter to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterSpec {
    /// Its key: a block filter's fixed one, persistent; `None` for a launch's permit, which
    /// WFP keys itself and drops when the launch's session closes.
    pub key: Option<u128>,
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

/// A WFP object whose security descriptor set-up changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Object {
    /// Our sublayer.
    Sublayer,
    /// The engine's filter container (the filters' security, key `NULL`).
    FilterContainer,
    /// A layer.
    Layer(Layer),
    /// Our provider.
    Provider,
    /// One of set-up's block filters, by key.
    Filter(u128),
}

/// One allow entry set-up adds for the owner on an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    /// The object.
    pub object: Object,
    /// The rights (`FWPM_ACTRL_*`).
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
    #[error("{0} isn't a sandbox user's SID")]
    NotAnAccount(Sid),
    /// The owner is named as a pool user: the lawyer's own programs would lose the network.
    #[error("the owner, {0}, is named as a sandbox user")]
    OwnerInPool(Sid),
}

/// Set-up's network step: who owns the delegation and which pool users the filters cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSetup {
    owner: Sid,
    pool: Vec<Sid>,
}

/// Set-up's block filters, in key order: each one's name, layer and conditions besides the
/// users. Appended only, so each keeps its key.
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

/// A block filter's conditions besides the users.
#[derive(Debug, Clone, Copy)]
enum Block {
    All,
    NotLoopback,
    Protocol(u8),
}

impl NetworkSetup {
    /// The step for `owner`'s set-up, covering every SID in `pool`, sorted and de-duplicated so
    /// a re-run over the same pool builds the same filters.
    pub fn new(owner: &Sid, pool: &[Sid]) -> Result<NetworkSetup, SpecError> {
        let mut sorted = pool.to_vec();
        sorted.sort();
        sorted.dedup();
        if sorted.is_empty() {
            return Err(SpecError::NoPoolUser);
        }
        for sid in &sorted {
            if !is_account(sid) {
                return Err(SpecError::NotAnAccount(sid.clone()));
            }
            if sid == owner {
                return Err(SpecError::OwnerInPool(sid.clone()));
            }
        }
        Ok(NetworkSetup {
            owner: owner.clone(),
            pool: sorted,
        })
    }

    /// The owner: the desktop user whose launches add and remove their own permits.
    pub fn owner(&self) -> &Sid {
        &self.owner
    }

    /// The pool users the filters and rules cover, sorted.
    pub fn pool(&self) -> &[Sid] {
        &self.pool
    }

    /// Set-up's block filters, persistent, in key order ([`block_filters`]).
    pub fn filters(&self, ns: &Namespace) -> Vec<FilterSpec> {
        block_filters(ns, &self.pool)
    }

    /// The firewall rules for the pool ([`rules::rules`](super::rules::rules)).
    pub fn rules(&self, ns: &Namespace) -> Vec<RuleSpec> {
        super::rules::rules(ns, &self.pool)
    }
}

/// The owner's grants in `ns`, the same for any pool: PR0 (c)'s three that let it add a permit
/// (the sublayer, the filter container, the permit's layer), then read on the provider and each
/// block filter, so a launch can check them before it adds its permit.
pub fn grants(ns: &Namespace) -> Vec<Grant> {
    let mut grants = vec![
        Grant {
            object: Object::Sublayer,
            mask: ACTRL_ADD_LINK | ACTRL_READ,
        },
        Grant {
            object: Object::FilterContainer,
            mask: ACTRL_ADD | ACTRL_READ,
        },
        Grant {
            object: Object::Layer(PERMIT_LAYER),
            mask: ACTRL_ADD_LINK | ACTRL_READ,
        },
        Grant {
            object: Object::Provider,
            mask: ACTRL_READ,
        },
    ];
    grants.extend(blocks(ns).into_iter().map(|(key, ..)| Grant {
        object: Object::Filter(key),
        mask: ACTRL_READ,
    }));
    grants
}

/// The objects whose owner grant removal takes back; the sublayer, the provider and the
/// filters go with their deletion.
pub fn revocations() -> [Object; 2] {
    [Object::FilterContainer, Object::Layer(PERMIT_LAYER)]
}

/// The layer a launch's permit sits at.
pub const PERMIT_LAYER: Layer = Layer::ConnectV4;

/// Set-up's block filters: key, name, layer and kind.
fn blocks(ns: &Namespace) -> Vec<(u128, String, Layer, Block)> {
    (0u16..)
        .zip(BLOCKS)
        .map(|(n, (name, layer, block))| {
            let name = format!("{} {name}", ns.display);
            (ns.filter_key(n), name, layer, block)
        })
        .collect()
}

/// Set-up's block filters in `ns` naming `pool`, persistent, in key order. A launch's check
/// builds them for its own pool user alone: the same keys, names, layers and actions.
pub fn block_filters(ns: &Namespace, pool: &[Sid]) -> Vec<FilterSpec> {
    blocks(ns)
        .into_iter()
        .map(|(key, name, layer, block)| {
            let mut conditions = vec![Condition::Users(pool.to_vec())];
            match block {
                Block::All => {}
                Block::NotLoopback => conditions.push(Condition::NotLoopback),
                Block::Protocol(protocol) => conditions.push(Condition::Protocol(protocol)),
            }
            FilterSpec {
                key: Some(key),
                name,
                layer,
                action: Action::Block,
                weight: BLOCK_WEIGHT,
                conditions,
            }
        })
        .collect()
}

/// Whether `sid` is an account's: `S-1-5-21-<a>-<b>-<c>-<rid>`, never a well-known group.
fn is_account(sid: &Sid) -> bool {
    sid.domain().is_some() && sid.as_str().split('-').count() == 8
}

/// The security descriptor an `ALE_USER_ID` condition names: owner and group `SYSTEM`, one
/// allow of `FWP_ACTRL_MATCH_FILTER` (`CC`) per SID, as PR0 (c) measured it.
pub fn user_sddl(sids: &[Sid]) -> String {
    format!("O:SYG:SYD:{}", aces(sids))
}

/// One allow of `CC` per SID, in SDDL.
pub(super) fn aces(sids: &[Sid]) -> String {
    sids.iter()
        .map(|sid| format!("(A;;CC;;;{sid})"))
        .collect::<Vec<_>>()
        .concat()
}

/// The SIDs an `ALE_USER_ID` condition's descriptor matches, read from its DACL in SDDL
/// (`ConvertSecurityDescriptorToStringSecurityDescriptorW`): each allow of exactly `CC`.
/// `None` when the DACL holds anything else (a deny, other rights, a SID in alias form such as
/// `WD`), so a launch's check never reads coverage out of a descriptor it doesn't understand.
pub fn sddl_users(sddl: &str) -> Option<Vec<Sid>> {
    let (_, dacl) = sddl.split_once("D:")?;
    let dacl = dacl.split("S:").next().unwrap_or(dacl);
    let (_, aces) = dacl.split_once('(')?;
    let mut users = Vec::new();
    for ace in format!("({aces}").split_terminator(')') {
        let ace = ace.strip_prefix('(')?;
        let fields: Vec<&str> = ace.split(';').collect();
        match fields.as_slice() {
            ["A", "", "CC", "", "", sid] => users.push(Sid::parse(sid).ok()?),
            _ => return None,
        }
    }
    Some(users)
}

/// The loopback ports a launch's permit opens: `policy`'s proxy ports and its
/// [`Loopback::Ports`] ([`checked_ports`]). [`Loopback::Any`] is refused (D10: the browser,
/// the only entry that asks for it, is off on tier B).
pub fn permit_ports(policy: &Policy) -> Result<Vec<u16>, NetworkGap> {
    let mut ports = policy.proxy_ports().to_vec();
    match policy.loopback() {
        Loopback::None => {}
        Loopback::Ports(more) => ports.extend(more),
        Loopback::Any => return Err(NetworkGap::LoopbackAny),
    }
    checked_ports(&ports)
}

/// `ports` sorted and de-duplicated; a port outside [`WINDOWS_PORTS`], which the firewall
/// would block anyway, is refused rather than permitted in vain.
pub fn checked_ports(ports: &[u16]) -> Result<Vec<u16>, NetworkGap> {
    let mut ports = ports.to_vec();
    ports.sort_unstable();
    ports.dedup();
    if let Some(port) = ports.iter().find(|port| !WINDOWS_PORTS.contains(port)) {
        return Err(NetworkGap::PortOutsideRange(*port));
    }
    Ok(ports)
}

/// A launch's permit for `user`: one filter per port, `127.0.0.1` over TCP, at
/// [`PERMIT_LAYER`] and [`PERMIT_WEIGHT`], dynamic.
pub fn permit_filters(ns: &Namespace, user: &Sid, ports: &[u16]) -> Vec<FilterSpec> {
    ports
        .iter()
        .map(|&port| FilterSpec {
            key: None,
            name: format!("{} permit 127.0.0.1:{port}", ns.display),
            layer: PERMIT_LAYER,
            action: Action::Permit,
            weight: PERMIT_WEIGHT,
            conditions: vec![
                Condition::Users(vec![user.clone()]),
                Condition::RemoteV4(Ipv4Addr::LOCALHOST),
                Condition::RemotePort(port),
                Condition::Protocol(PROTOCOL_TCP),
            ],
        })
        .collect()
}

/// What set-up's network step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkReport {
    /// How many block filters it wrote.
    pub filters: usize,
    /// The pool users they cover, sorted.
    pub covered: Vec<Sid>,
    /// The firewall rules.
    pub firewall: FirewallOutcome,
}
