//! Each pool user's slot's filters (0049, "Loopback ports per slot"), with no Windows call: who
//! may listen on slot `s`'s ports. Set-up's listen blocks (`spec.rs`, weight 1) keep every pool
//! user off blocks H, P and D; these, per slot:
//!
//! - 0, `listen_pool` (F3): Listen V4 permit, weight 10, `Users[Us]`, local `P_s`;
//! - 1, `listen_daemon` (F3): Listen V4 permit, weight 10, `Users[Us]`, local `D_s`;
//! - 2, `only_listen_pool_v4`: Listen V4 block, weight 1, `NotUsers[Us]`, local `P_s`;
//! - 3, `only_listen_pool_v6`: Listen V6 block, weight 1, `NotUsers[Us]`, local `P_s`;
//! - 4, `listen_pool_v6` (F3): Listen V6 permit, weight 10, `Users[Us]`, local `P_s`;
//! - 5, `listen_daemon_v6` (F3): Listen V6 permit, weight 10, `Users[Us]`, local `D_s`.
//!
//! So only `Us` listens on `P_s`, over IPv4 or IPv6 (a daemon on `[::]` included). On `D_s`,
//! inside Windows' dynamic range, `Us` and any program that isn't a pool user's may listen, as
//! Windows hands such programs a dynamic port for any port-0 bind. No pool user listens on
//! `H_s`; the host binds it before the agent starts and holds it.
//!
//! Nothing here permits a connect: set-up's `block_connect_v4` and `block_connect_v6` refuse
//! every loopback connect of every pool user, its own slot's ports included. The exact-port
//! connect permits come from PR6's SYSTEM server, per session (`PermitPorts`, `RevokePorts`;
//! 0049 "Connect permits, from PR6"). An earlier build of this branch wrote three static
//! connect permits per slot at key offsets 0 to 2 ([`RETIRED_OFFSETS`]); no set-up writes them
//! now, a launch that finds one refuses it as a filter set-up didn't write, and set-up and
//! removal delete them.
//!
//! Filter `n` of slot `s` is `filter_key(SLOT_BASE + SLOT_STRIDE * s + SLOT_FIRST + n)`, so a
//! slot's filters keep their keys whatever other users the pool holds.

use std::net::Ipv4Addr;
use std::ops::RangeInclusive;

use super::check::NetworkGap;
use super::spec::{
    Action, BLOCK_WEIGHT, Condition, FilterSpec, Layer, Namespace, PERMIT_BASE, PERMIT_WEIGHT,
    PROTOCOL_TCP, SpecError,
};
use crate::ports::Slot;
use crate::windows::pool::{POOL_CAP, PoolUser};
use crate::windows::sids::Sid;
use crate::{Loopback, Policy};

/// Slot 0's first filter's offset from [`Namespace::filters`].
pub const SLOT_BASE: u16 = 0x200;
/// How many keys each slot has, with room for later filters.
pub const SLOT_STRIDE: u16 = 16;
/// How many filters each slot has.
pub const SLOT_FILTERS: u16 = 6;
/// Slot filter 0's offset within its slot's keys: offsets below it are retired.
pub const SLOT_FIRST: u16 = 3;
/// The key offsets an earlier build of this branch gave each slot's static connect permits,
/// never written now and never reused.
pub const RETIRED_OFFSETS: std::ops::Range<u16> = 0..SLOT_FIRST;
/// The ports an earlier set-up's shared permit opened to every pool user.
pub const OLD_PERMIT_PORTS: RangeInclusive<u16> = 46_100..=46_199;

/// What each of a slot's filters is, in key order.
#[derive(Debug, Clone, Copy)]
enum Shape {
    ListenPermit(Block, Layer),
    OnlyListen(Layer),
}

/// One of a slot's blocks of ports a pool user listens on.
#[derive(Debug, Clone, Copy)]
enum Block {
    Pool,
    Daemon,
}

const SHAPES: [(&str, Shape); SLOT_FILTERS as usize] = [
    (
        "listen_pool",
        Shape::ListenPermit(Block::Pool, Layer::ListenV4),
    ),
    (
        "listen_daemon",
        Shape::ListenPermit(Block::Daemon, Layer::ListenV4),
    ),
    ("only_listen_pool_v4", Shape::OnlyListen(Layer::ListenV4)),
    ("only_listen_pool_v6", Shape::OnlyListen(Layer::ListenV6)),
    (
        "listen_pool_v6",
        Shape::ListenPermit(Block::Pool, Layer::ListenV6),
    ),
    (
        "listen_daemon_v6",
        Shape::ListenPermit(Block::Daemon, Layer::ListenV6),
    ),
];

impl Block {
    fn of(self, slot: Slot) -> (u16, u16) {
        let ports = match self {
            Block::Pool => slot.pool(),
            Block::Daemon => slot.daemon(),
        };
        (*ports.start(), *ports.end())
    }
}

/// The key at `offset` within `slot`'s keys in `ns`, retired offsets included.
pub fn slot_offset_key(ns: &Namespace, slot: Slot, offset: u16) -> u128 {
    ns.filter_key(SLOT_BASE + SLOT_STRIDE * u16::from(slot.index()) + offset)
}

/// Filter `n` of `slot`'s key in `ns`.
pub fn slot_key(ns: &Namespace, slot: Slot, n: u16) -> u128 {
    slot_offset_key(ns, slot, SLOT_FIRST + n)
}

/// The slot and filter number whose key `key` is in `ns`, when it is one; a retired key is
/// none.
pub(super) fn slot_of_key(ns: &Namespace, key: u128) -> Option<(Slot, u16)> {
    let offset = u16::try_from(key.checked_sub(ns.filter_key(SLOT_BASE))?).ok()?;
    let (index, offset) = (offset / SLOT_STRIDE, offset % SLOT_STRIDE);
    let slot = Slot::new(u8::try_from(index).ok()?)?;
    let n = offset.checked_sub(SLOT_FIRST)?;
    (n < SLOT_FILTERS).then_some((slot, n))
}

/// `slot`'s filters for its pool user `user`, in key order, persistent.
pub fn slot_filters(ns: &Namespace, slot: Slot, user: &Sid) -> Vec<FilterSpec> {
    let us = || vec![user.clone()];
    (0u16..)
        .zip(SHAPES)
        .map(|(n, (name, shape))| {
            let (layer, action, weight, conditions) = match shape {
                Shape::ListenPermit(block, layer) => {
                    let (low, high) = block.of(slot);
                    let conditions = vec![Condition::Users(us()), Condition::LocalPorts(low, high)];
                    (layer, Action::Permit, PERMIT_WEIGHT, conditions)
                }
                Shape::OnlyListen(layer) => {
                    let (low, high) = Block::Pool.of(slot);
                    let conditions =
                        vec![Condition::NotUsers(us()), Condition::LocalPorts(low, high)];
                    (layer, Action::Block, BLOCK_WEIGHT, conditions)
                }
            };
            FilterSpec {
                key: slot_key(ns, slot, n),
                name: format!("{} {name} slot {}", ns.display, slot.index()),
                layer,
                action,
                weight,
                conditions,
            }
        })
        .collect()
}

/// Each pool user's slot, from its account name (`ClauseoSbNN` is slot `NN - 1`), never from
/// its place in the pool. A name that isn't a pool user's is refused.
pub fn slots_of(users: &[PoolUser]) -> Result<Vec<(Slot, Sid)>, SpecError> {
    users
        .iter()
        .map(|user| {
            Slot::from_account(&user.name)
                .map(|slot| (slot, user.sid.clone()))
                .ok_or_else(|| SpecError::NoSlot(user.name.clone()))
        })
        .collect()
}

/// An earlier set-up's shared permit for `user`, the `i`th pool user (sorted): `127.0.0.1`
/// over TCP on [`OLD_PERMIT_PORTS`] at `ALE_AUTH_CONNECT_V4`. No set-up writes it now; a launch
/// that finds one refuses ([`NetworkGap::OldSetup`]) until set-up replaces it.
pub fn old_permit_filter(ns: &Namespace, i: u16, user: &Sid) -> FilterSpec {
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
            Condition::RemotePorts(*OLD_PERMIT_PORTS.start(), *OLD_PERMIT_PORTS.end()),
        ],
    }
}

/// Whether `key` is one of an earlier set-up's shared permits' keys in `ns`.
pub fn is_old_permit_key(ns: &Namespace, key: u128) -> bool {
    (0..u16::from(POOL_CAP)).any(|i| ns.filter_key(PERMIT_BASE + i) == key)
}

/// The loopback ports a launch names: `policy`'s proxy ports and its [`Loopback::Ports`].
/// [`Loopback::Any`] is refused (D10).
pub fn permit_ports(policy: &Policy) -> Result<Vec<u16>, NetworkGap> {
    let mut ports = policy.proxy_ports().to_vec();
    match policy.loopback() {
        Loopback::None => {}
        Loopback::Ports(more) => ports.extend(more),
        Loopback::Any => return Err(NetworkGap::LoopbackAny),
    }
    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

/// `ports` sorted and de-duplicated; a port that isn't one of `slot`'s 24 is refused, so a
/// launch never names a port PR6's per-session connect permits wouldn't open. Until those land,
/// set-up's blocks refuse the agent's connects to all of them.
pub fn checked_ports(slot: Slot, ports: &[u16]) -> Result<Vec<u16>, NetworkGap> {
    let mut ports = ports.to_vec();
    ports.sort_unstable();
    ports.dedup();
    if let Some(port) = ports.iter().find(|port| !slot.reaches(**port)) {
        return Err(NetworkGap::PortOutsideRange(*port));
    }
    Ok(ports)
}
