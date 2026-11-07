//! Whether tier B's ports (`ports::TIER_B_PORTS`) are usable on this machine (S15 PR7), with no
//! Windows call. Set-up's static permits open the range to every pool user, so nothing but
//! Clauseo's own listeners may sit there: Windows' TCP dynamic range must not overlap it (the OS
//! would hand its ports to any program's port-0 bind), no port of it may be excluded (Hyper-V,
//! WSL and Docker reserve ranges Windows then refuses to bind), and at each launch every TCP
//! listener in it must be this app's. `os_ports.rs` reads the machine.

use std::ops::RangeInclusive;

use super::check::NetworkGap;
use crate::ports::TIER_B_PORTS;

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
    /// The ports of the range Windows refuses to bind on `127.0.0.1` or `::1` (excluded).
    pub reserved: Vec<u16>,
    /// The TCP listeners in the range, any address, IPv4 and IPv6; none at set-up.
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

/// Whether `listener` is this app's: this process (the proxy's and the relay's listeners), or a
/// process running this process's own program file (another of the app's processes).
pub fn is_ours(listener: &Listener, own_pid: u32, own_image: Option<&str>) -> bool {
    listener.pid == own_pid
        || matches!(
            (listener.image.as_deref(), own_image),
            (Some(theirs), Some(ours)) if theirs.eq_ignore_ascii_case(ours)
        )
}

/// Checks `seen` for tier B: no dynamic range overlapping the range, no port of it reserved,
/// and every listener in it one that `ours` accepts.
pub fn ports_usable(seen: &PortsSeen, ours: impl Fn(&Listener) -> bool) -> Result<(), NetworkGap> {
    if let Some(&(first, last)) = seen
        .dynamic
        .iter()
        .find(|span| overlaps(&TIER_B_PORTS, **span))
    {
        return Err(NetworkGap::DynamicPortsOverlap { first, last });
    }
    if let Some(&port) = seen
        .reserved
        .iter()
        .find(|port| TIER_B_PORTS.contains(port))
    {
        return Err(NetworkGap::PortsReserved(port));
    }
    let foreign = seen
        .listeners
        .iter()
        .find(|listener| TIER_B_PORTS.contains(&listener.port) && !ours(listener));
    if let Some(listener) = foreign {
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
