//! Which addresses the proxy may connect to: public ones only.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// True when `ip` is a public unicast address. Refused (stage E's requirement 4, plus the ranges
/// below): IPv4 `0.0.0.0/8`, `10/8`, `100.64/10`, `127/8`, `169.254/16`, `172.16/12`,
/// `192.168/16`, `198.18/15`, `224/4`, `240/4` (with `255.255.255.255`); IPv6 `::`, `::1`,
/// `fe80::/10`, `fec0::/10`, `fc00::/7`, `ff00::/8`, Teredo `2001::/32` and local NAT64
/// `64:ff9b:1::/48` (both hide an address the proxy cannot check), and the IPv4 inside
/// `::ffff:0:0/96`, `::ffff:0:0:0/96`, `::/96`, `64:ff9b::/96` and `2002::/16` by the IPv4 rules.
/// Documentation ranges count as public.
pub(crate) fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_public(v4),
        IpAddr::V6(v6) => v6_public(v6),
    }
}

fn v4_public(ip: Ipv4Addr) -> bool {
    let [first, second, _, _] = ip.octets();
    let refused = first == 0
        || first == 10
        || (first == 100 && second & 0xc0 == 64)
        || first == 127
        || (first == 169 && second == 254)
        || (first == 172 && second & 0xf0 == 16)
        || (first == 192 && second == 168)
        || (first == 198 && second & 0xfe == 18)
        || first >= 224;
    !refused
}

fn v6_public(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    let [s0, s1, s2, s3, s4, s5, s6, s7] = s;
    let embedded = |hi: u16, lo: u16| {
        let [o1, o2] = hi.to_be_bytes();
        let [o3, o4] = lo.to_be_bytes();
        Ipv4Addr::new(o1, o2, o3, o4)
    };
    let zero_to = |n: usize| s.iter().take(n).all(|&x| x == 0);
    if zero_to(8) || (zero_to(7) && s7 == 1) {
        return false;
    }
    if s0 & 0xffc0 == 0xfe80 || s0 & 0xffc0 == 0xfec0 || s0 & 0xfe00 == 0xfc00 {
        return false;
    }
    if s0 & 0xff00 == 0xff00 {
        return false;
    }
    if s0 == 0x2001 && s1 == 0 {
        return false; // Teredo
    }
    if s0 == 0x64 && s1 == 0xff9b && s2 == 1 {
        return false; // local-use NAT64
    }
    let mapped = zero_to(5) && s5 == 0xffff;
    let translated = zero_to(4) && s4 == 0xffff && s5 == 0;
    let compatible = zero_to(6);
    let nat64 = s0 == 0x64 && s1 == 0xff9b && s2 == 0 && s3 == 0 && s4 == 0 && s5 == 0;
    if mapped || translated || compatible || nat64 {
        return v4_public(embedded(s6, s7));
    }
    if s0 == 0x2002 {
        return v4_public(embedded(s1, s2));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(text: &str) -> bool {
        is_public(text.parse().unwrap())
    }

    #[test]
    fn ipv4_ranges() {
        for refused in [
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "127.255.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.255",
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!public(refused), "{refused}");
        }
        for allowed in [
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.0",
            "172.15.255.255",
            "172.32.0.0",
            "192.0.2.1",
            "198.17.255.255",
            "198.20.0.0",
            "198.51.100.7",
            "203.0.113.9",
            "223.255.255.255",
        ] {
            assert!(public(allowed), "{allowed}");
        }
    }

    #[test]
    fn ipv6_ranges() {
        for refused in [
            "::",
            "::1",
            "fe80::1",
            "febf::1",
            "fec0::1",
            "fc00::1",
            "fd12:3456::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "::ffff:0:192.168.0.1",
            "::127.0.0.1",
            "::10.0.0.1",
            "64:ff9b::7f00:1",
            "64:ff9b:1::1",
            "2002:7f00:1::",
            "2002:a9fe:a9fe::1",
            "2001:0:4136:e378::1",
        ] {
            assert!(!public(refused), "{refused}");
        }
        for allowed in [
            "2606:4700::1111",
            "2001:db8::1",
            "::ffff:192.0.2.1",
            "64:ff9b::c000:201",
            "2002:c000:201::",
        ] {
            assert!(public(allowed), "{allowed}");
        }
    }
}
