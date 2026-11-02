//! The host a `url` names, read the way a WHATWG URL parser (Chrome's, and the `url` crate behind
//! reqwest) reads it, and refused when it is loopback, unspecified, private or link-local. This
//! is extra defence: the real rule is the egress proxy's, which checks the address after DNS
//! resolution (`docs/decisions/0029-browser.md`, "Requirements on stage C's proxy and jail").
//! Anything this reader cannot settle is refused, never passed.
//!
//! Only values that already passed the scheme check reach it: `http://...`, `https://...`, or a
//! value without `:` (agent-browser puts `https://` in front, `commands.rs:76-90`). Steps:
//! 1. Any ASCII control character or space in the trimmed value: unclear (WHATWG drops tabs and
//!    newlines anywhere, so `127.0.0\n.1` would mean `127.0.0.1`).
//! 2. The authority runs to the first `/`, `\`, `?` or `#`; userinfo ends at its last `@`.
//! 3. A port must be ASCII digits. An IPv6 literal sits in brackets and must parse as one.
//! 4. The host must be ASCII letters, digits, `.`, `-` or `_`: a percent escape or a non-ASCII
//!    character could decode to digits and dots (`%31` or `１` for `1`), so both are refused.
//! 5. Trailing dots go. `localhost` and `*.localhost` are local.
//! 6. A host whose last label is a number (decimal, `0`-led octal or `0x` hex) is an IPv4
//!    address in WHATWG's reading, with one to four parts (`2130706433`, `0x7f.1`); one that
//!    does not fit is unclear.

use std::net::{Ipv4Addr, Ipv6Addr};

/// The refusal's name for a local or private address.
pub(crate) const LOCAL: &str = "local or private network addresses";
/// The refusal's name for a URL whose host the guard cannot settle.
pub(crate) const UNCLEAR: &str = "URLs whose host Clauseo cannot read";
/// The refusal's name for a host outside ASCII.
pub(crate) const NON_ASCII: &str = "non-ASCII host names (use the xn-- form)";

/// `Err` with the refusal's name. `url` is the raw value; `about:blank` passes.
pub(crate) fn check(url: &str) -> Result<(), &'static str> {
    let text = url.trim();
    if text.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err(UNCLEAR);
    }
    if text.eq_ignore_ascii_case("about:blank") {
        return Ok(());
    }
    // ASCII only: a non-ASCII host is refused below, so no Unicode case mapping (the Kelvin
    // sign lower-cases to `k`) can turn one into a name that passes.
    let lowered = text.to_ascii_lowercase();
    let rest = lowered
        .strip_prefix("http://")
        .or_else(|| lowered.strip_prefix("https://"))
        .unwrap_or(&lowered);
    let end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
    let authority = rest.get(..end).ok_or(UNCLEAR)?;
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    check_host(split_port(host_port)?)
}

/// The host without its port; `[...]` kept for an IPv6 literal.
fn split_port(host_port: &str) -> Result<&str, &'static str> {
    let (host, port) = if let Some(inner) = host_port.strip_prefix('[') {
        let (address, after) = inner.split_once(']').ok_or(UNCLEAR)?;
        let host = host_port.get(..address.len() + 2).ok_or(UNCLEAR)?;
        match after {
            "" => (host, None),
            _ => (host, Some(after.strip_prefix(':').ok_or(UNCLEAR)?)),
        }
    } else {
        match host_port.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };
    if port.is_some_and(|port| !port.bytes().all(|b| b.is_ascii_digit())) {
        return Err(UNCLEAR);
    }
    Ok(host)
}

fn check_host(host: &str) -> Result<(), &'static str> {
    if let Some(inner) = host.strip_prefix('[') {
        let address: Ipv6Addr = inner
            .strip_suffix(']')
            .and_then(|text| text.parse().ok())
            .ok_or(UNCLEAR)?;
        return if is_local_v6(address) {
            Err(LOCAL)
        } else {
            Ok(())
        };
    }
    if !host.is_ascii() {
        return Err(NON_ASCII);
    }
    if !host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return Err(UNCLEAR);
    }
    let name = host.trim_end_matches('.');
    if name.is_empty() {
        return Err(UNCLEAR);
    }
    if name == "localhost" || name.ends_with(".localhost") {
        return Err(LOCAL);
    }
    let last = name.rsplit('.').next().unwrap_or_default();
    if !is_number_label(last) {
        return Ok(());
    }
    match parse_ipv4(name) {
        Some(address) if is_local_v4(address) => Err(LOCAL),
        Some(_) => Ok(()),
        None => Err(UNCLEAR),
    }
}

/// WHATWG's "ends in a number": all decimal digits, or `0x` and hex digits (`0x` alone too).
fn is_number_label(label: &str) -> bool {
    if let Some(hex) = label.strip_prefix("0x") {
        return hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    !label.is_empty() && label.bytes().all(|b| b.is_ascii_digit())
}

/// WHATWG's IPv4 parser: one to four parts, each decimal, `0`-led octal or `0x` hex; all but the
/// last at most 255, the last filling the bytes left. `None` for anything else.
pub(crate) fn parse_ipv4(name: &str) -> Option<Ipv4Addr> {
    let parts: Vec<&str> = name.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let numbers = parts
        .iter()
        .map(|part| parse_part(part))
        .collect::<Option<Vec<u64>>>()?;
    let (last, front) = numbers.split_last()?;
    if front.iter().any(|n| *n > 255) {
        return None;
    }
    let left_bytes = u32::try_from(5 - numbers.len()).ok()?;
    if *last >= 256_u64.pow(left_bytes) {
        return None;
    }
    let mut value: u64 = 0;
    for (i, n) in front.iter().enumerate() {
        value |= n << (8 * (3 - i));
    }
    u32::try_from(value | last).ok().map(Ipv4Addr::from)
}

fn parse_part(part: &str) -> Option<u64> {
    let (digits, radix) = if let Some(hex) = part.strip_prefix("0x") {
        (hex, 16)
    } else if part.len() > 1 && part.starts_with('0') {
        (part.get(1..)?, 8)
    } else {
        (part, 10)
    };
    if digits.is_empty() {
        // `0x` alone is zero; an empty part is not a number.
        return (radix == 16).then_some(0);
    }
    if digits.len() > 32 || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    u64::from_str_radix(digits, radix).ok()
}

/// `0.0.0.0/8`, `127/8`, `10/8`, `172.16/12`, `192.168/16`, `169.254/16`.
pub(crate) fn is_local_v4(address: Ipv4Addr) -> bool {
    let [a, b, _, _] = address.octets();
    matches!(a, 0 | 10 | 127)
        || (a == 172 && (b & 0xf0) == 16)
        || (a == 192 && b == 168)
        || (a == 169 && b == 254)
}

/// `::1`, `::`, `fc00::/7`, `fe80::/10`, and IPv4-mapped (`::ffff:a.b.c.d`) or IPv4-compatible
/// (`::a.b.c.d`) forms of a local IPv4 address.
pub(crate) fn is_local_v6(address: Ipv6Addr) -> bool {
    let first = address.segments()[0];
    if address.is_loopback()
        || address.is_unspecified()
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
    {
        return true;
    }
    if let Some(v4) = address.to_ipv4_mapped() {
        return is_local_v4(v4);
    }
    let octets = address.octets();
    if octets[..12].iter().all(|b| *b == 0) {
        return is_local_v4(Ipv4Addr::new(
            octets[12], octets[13], octets[14], octets[15],
        ));
    }
    false
}

#[cfg(test)]
mod tests;
