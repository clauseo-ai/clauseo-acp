//! The machine's own networks, which the proxy never connects to: each interface address and
//! its on-link prefix, read once when the proxy starts ([`LocalNets::read`]). Their addresses
//! can be public (a global IPv6 /64, a public IPv4 on the interface), so the public-address
//! check alone would let a page that learned them reach the router and the other devices behind
//! the router's firewall through the browser listener. The IPv4 address inside an IPv4-mapped,
//! NAT64 (`64:ff9b::/96`) or 6to4 (`2002::/16`) address is checked too. A network the machine
//! joins during the session is seen by the next launch's proxy only.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use self::unix as os;
#[cfg(windows)]
mod win;
#[cfg(windows)]
use self::win as os;

/// One interface address and the length of its on-link prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OnLink {
    ip: IpAddr,
    prefix: u8,
}

impl OnLink {
    /// `ip` on a `prefix`-bit network. A zero prefix (a point-to-point link names no network)
    /// or one longer than the address stands for the address alone.
    pub(crate) fn new(ip: IpAddr, prefix: u8) -> OnLink {
        let bits = if ip.is_ipv4() { 32 } else { 128 };
        let prefix = if prefix == 0 || prefix > bits {
            bits
        } else {
            prefix
        };
        OnLink { ip, prefix }
    }

    fn contains(self, ip: IpAddr) -> bool {
        match (self.ip, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                same_prefix(v4_bits(net), v4_bits(ip), self.prefix)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                same_prefix(u128::from(net), u128::from(ip), self.prefix)
            }
            _ => false,
        }
    }
}

/// An IPv4 address in the top 32 bits, so one mask rule serves both families.
fn v4_bits(ip: Ipv4Addr) -> u128 {
    u128::from(u32::from(ip)) << 96
}

/// Whether `a` and `b` share their first `prefix` bits (1 to 128).
fn same_prefix(a: u128, b: u128, prefix: u8) -> bool {
    let mask = u128::MAX.checked_shl(128 - u32::from(prefix)).unwrap_or(0);
    (a ^ b) & mask == 0
}

/// The IPv4 address an IPv6 address carries to a translator or a relay: IPv4-mapped
/// (`::ffff:0:0/96`), NAT64 (`64:ff9b::/96`) or 6to4 (`2002::/16`).
fn carried_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let ([0, 0, 0, 0, 0, 0xffff, hi, lo]
    | [0x64, 0xff9b, 0, 0, 0, 0, hi, lo]
    | [0x2002, hi, lo, ..]) = ip.segments()
    else {
        return None;
    };
    let bits = (u32::from(hi) << 16) | u32::from(lo);
    Some(Ipv4Addr::from(bits))
}

/// The machine's own networks.
#[derive(Debug, Clone, Default)]
pub(crate) struct LocalNets(Vec<OnLink>);

impl LocalNets {
    /// These networks (the test seam).
    #[cfg(test)]
    pub(crate) fn new(nets: Vec<OnLink>) -> LocalNets {
        LocalNets(nets)
    }

    /// The OS's interface list: every IPv4 and IPv6 address with its on-link prefix.
    pub(crate) fn read() -> io::Result<LocalNets> {
        os::read().map(LocalNets)
    }

    /// How many networks it holds (logged at start; never the addresses).
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether `ip`, or the IPv4 address it carries, is on one of these networks.
    pub(crate) fn contains(&self, ip: IpAddr) -> bool {
        let carried = match ip {
            IpAddr::V6(v6) => carried_v4(v6).map(IpAddr::V4),
            IpAddr::V4(_) => None,
        };
        self.0
            .iter()
            .any(|net| net.contains(ip) || carried.is_some_and(|v4| net.contains(v4)))
    }
}

#[cfg(not(any(unix, windows)))]
mod os {
    /// No interface list on this OS.
    pub(super) fn read() -> std::io::Result<Vec<super::OnLink>> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nets(entries: &[(&str, u8)]) -> LocalNets {
        LocalNets::new(
            entries
                .iter()
                .map(|(ip, prefix)| OnLink::new(ip.parse().unwrap(), *prefix))
                .collect(),
        )
    }

    fn on(nets: &LocalNets, ip: &str) -> bool {
        nets.contains(ip.parse().unwrap())
    }

    #[test]
    fn an_interface_s_address_and_its_on_link_prefix_are_local() {
        let local = nets(&[("2001:db8:1:2::10", 64), ("203.0.113.7", 24)]);
        for ip in [
            "2001:db8:1:2::10",
            "2001:db8:1:2::1",
            "2001:db8:1:2:ffff:ffff:ffff:ffff",
            "203.0.113.7",
            "203.0.113.1",
            "203.0.113.255",
            "::ffff:203.0.113.1",
            "64:ff9b::203.0.113.1",
            "2002:cb00:7101::1",
        ] {
            assert!(on(&local, ip), "{ip}");
        }
        for ip in [
            "2001:db8:1:3::1",
            "2001:db8:2:2::10",
            "203.0.114.1",
            "198.51.100.7",
            "::ffff:198.51.100.7",
        ] {
            assert!(!on(&local, ip), "{ip}");
        }
    }

    #[test]
    fn a_zero_or_overlong_prefix_is_the_address_alone() {
        let local = nets(&[("198.51.100.7", 0), ("2001:db8::7", 200)]);
        assert!(on(&local, "198.51.100.7"));
        assert!(!on(&local, "198.51.100.8"));
        assert!(on(&local, "2001:db8::7"));
        assert!(!on(&local, "2001:db8::8"));
    }

    #[test]
    fn a_full_length_prefix_and_a_one_bit_prefix_hold() {
        let local = nets(&[("192.0.2.9", 32), ("2001:db8::", 1)]);
        assert!(on(&local, "192.0.2.9"));
        assert!(!on(&local, "192.0.2.10"));
        // 2001:db8::/1 is every address with the top bit clear.
        assert!(on(&local, "7fff::1"));
        assert!(!on(&local, "8000::1"));
    }

    #[test]
    fn the_os_list_reads_and_holds_loopback() {
        let local = LocalNets::read().unwrap();
        assert_ne!(local.len(), 0);
        assert!(on(&local, "127.0.0.1"), "{local:?}");
    }
}
