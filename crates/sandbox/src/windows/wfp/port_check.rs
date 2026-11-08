//! Whether tier B's ports are usable on this machine (S15 PR7, 0049 "Loopback ports per slot"),
//! with no Windows call. `os_ports.rs` reads the machine.
//!
//! - **Machine-wide**, at set-up and at every launch: Windows' TCP dynamic range must not overlap
//!   blocks H and P (`ports::TIER_B_PORTS`), or the OS would hand their ports to any program's
//!   port-0 bind.
//! - **Per slot**, at a launch: only the launch's slot is looked at, and only that slot can be
//!   refused. A block of it (`H_s`, `P_s` or `D_s`) whose 8 ports are all unusable refuses
//!   the launch ([`NetworkGap::SlotPortsUnusable`]): a port Windows reserves (an excluded range,
//!   refused at the bind) is unusable, and in `H_s` so is one another program listens on. Fewer
//!   than 8 is fine: the H claim skips a busy port, and a P or D claim the same (a reserved one
//!   fails its bind and turns the browser off for the run). A foreign listener elsewhere in H
//!   (Inedo's agent on 46336, say) is no concern of the launch: no static filter lets a pool
//!   user connect to it, and PR6's permits name only ports the session holds.
//! - A foreign listener on a host port the launch's policy names (one its own listener should
//!   hold) refuses ([`NetworkGap::ForeignListener`]).

use std::ops::RangeInclusive;

use super::check::NetworkGap;
use crate::ports::{SLOT_PORTS, Slot, TIER_B_PORTS};

/// A TCP listener as Windows lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    /// Its local port.
    pub port: u16,
    /// Its process.
    pub pid: u32,
    /// Its process's program file, when it could be read.
    pub image: Option<String>,
}

/// What the machine says about tier B's ports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortsSeen {
    /// Windows' TCP dynamic port ranges, first and last port, one per TCP setting.
    pub dynamic: Vec<(u16, u16)>,
    /// The launch's slot's ports that Windows refuses to bind on `127.0.0.1` or `::1`
    /// (excluded); none at set-up.
    pub reserved: Vec<u16>,
    /// The TCP listeners, any address, IPv4 and IPv6; none at set-up.
    pub listeners: Vec<Listener>,
}

/// A dynamic range given as its start and its number of ports, as first and last port.
pub fn dynamic_span(start: u16, count: u16) -> Option<(u16, u16)> {
    let last = u32::from(start) + u32::from(count.checked_sub(1)?);
    Some((start, u16::try_from(last).unwrap_or(u16::MAX)))
}

/// Whether `span` shares a port with `range`.
fn overlaps(range: &RangeInclusive<u16>, (first, last): (u16, u16)) -> bool {
    first <= *range.end() && last >= *range.start()
}

/// Whether `listener` is this app's: this process's, since the proxy and the relay run in the
/// host. A program-file match isn't enough: another Windows user's Clauseo, or this program
/// started suspended and injected into, would pass it.
pub fn is_ours(listener: &Listener, own_pid: u32) -> bool {
    listener.pid == own_pid
}

/// What a launch checks beyond the machine: its slot, and the ports its policy names (its own
/// listeners').
#[derive(Debug, Clone, Copy)]
pub struct LaunchPorts<'a> {
    /// The launch's slot.
    pub slot: Slot,
    /// The ports the launch's policy names.
    pub ports: &'a [u16],
}

/// Checks `seen` for tier B: no dynamic range overlapping the range; with `launch`, no block of
/// its slot wholly unusable and no listener on its policy's ports that `ours` refuses.
pub fn ports_usable(
    seen: &PortsSeen,
    launch: Option<LaunchPorts<'_>>,
    ours: impl Fn(&Listener) -> bool,
) -> Result<(), NetworkGap> {
    if let Some(&(first, last)) = seen
        .dynamic
        .iter()
        .find(|span| overlaps(&TIER_B_PORTS, **span))
    {
        return Err(NetworkGap::DynamicPortsOverlap { first, last });
    }
    let Some(LaunchPorts { slot, ports }) = launch else {
        return Ok(());
    };
    let foreign = |port: u16| {
        seen.listeners
            .iter()
            .find(|listener| listener.port == port && !ours(listener))
    };
    let blocks = [
        ("host", slot.host(), true),
        ("browser", slot.pool(), false),
        ("browser daemon", slot.daemon(), false),
    ];
    for (block, range, listeners_count) in blocks {
        let unusable = range
            .clone()
            .filter(|port| {
                seen.reserved.contains(port) || (listeners_count && foreign(*port).is_some())
            })
            .count();
        if unusable == usize::from(SLOT_PORTS) {
            return Err(NetworkGap::SlotPortsUnusable {
                slot,
                block,
                first: *range.start(),
                last: *range.end(),
            });
        }
    }
    let own = ports.iter().filter(|port| slot.host().contains(port));
    if let Some(listener) = own.into_iter().find_map(|port| foreign(*port)) {
        return Err(NetworkGap::ForeignListener {
            port: listener.port,
            program: listener
                .image
                .clone()
                .unwrap_or_else(|| format!("process {}", listener.pid)),
        });
    }
    Ok(())
}
