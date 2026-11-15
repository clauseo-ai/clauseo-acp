//! The interface list on macOS and Linux: `getifaddrs`, walked once and freed.
#![expect(
    unsafe_code,
    reason = "getifaddrs and freeifaddrs, and reading the sockaddrs of the list they own"
)]

use std::io;
use std::mem::offset_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::OnLink;

/// Every IPv4 and IPv6 interface address with its netmask's prefix length (zero, the address
/// alone, when it has no netmask).
pub(super) fn read() -> io::Result<Vec<OnLink>> {
    let mut first: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: on success getifaddrs points `first` at a list it allocated, freed below.
    if unsafe { libc::getifaddrs(&raw mut first) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut found = Vec::new();
    let mut at = first.cast_const();
    while !at.is_null() {
        // SAFETY: `at` is a node of that list, which is freed only after the walk.
        let node = unsafe { &*at };
        // SAFETY: each sockaddr pointer of a node is null or points into the list.
        if let Some(ip) = unsafe { ip_of(node.ifa_addr, None) } {
            // SAFETY: as above; a netmask is read as its address's family.
            let mask = unsafe { ip_of(node.ifa_netmask, Some(ip)) };
            found.push(OnLink::new(ip, mask.map_or(0, prefix_len)));
        }
        at = node.ifa_next.cast_const();
    }
    // SAFETY: `first` came from getifaddrs and is freed once, after its last use.
    unsafe { libc::freeifaddrs(first) };
    Ok(found)
}

/// The leading one bits of a netmask.
fn prefix_len(mask: IpAddr) -> u8 {
    let ones = match mask {
        IpAddr::V4(v4) => u32::from(v4).leading_ones(),
        IpAddr::V6(v6) => u128::from(v6).leading_ones(),
    };
    u8::try_from(ones).unwrap_or(0)
}

/// The IPv4 or IPv6 address in `sa`, read as its own family, or as `like`'s for a netmask
/// (macOS may leave a netmask's family unset). `None` for a null pointer or another family.
///
/// # Safety
///
/// `sa` is null or points at a sockaddr of its family's size (on macOS, of its `sa_len`).
unsafe fn ip_of(sa: *const libc::sockaddr, like: Option<IpAddr>) -> Option<IpAddr> {
    if sa.is_null() {
        return None;
    }
    let family = match like {
        Some(IpAddr::V4(_)) => libc::AF_INET,
        Some(IpAddr::V6(_)) => libc::AF_INET6,
        // SAFETY: a non-null sockaddr holds at least its family.
        None => i32::from(unsafe { (*sa).sa_family }),
    };
    let full = match family {
        libc::AF_INET => size_of::<libc::sockaddr_in>(),
        libc::AF_INET6 => size_of::<libc::sockaddr_in6>(),
        _ => return None,
    };
    // SAFETY: as the caller's.
    let len = unsafe { readable(sa, full) };
    let bytes = sa.cast::<u8>();
    Some(if family == libc::AF_INET {
        let at = offset_of!(libc::sockaddr_in, sin_addr);
        // SAFETY: `len` bytes at `bytes` are readable.
        IpAddr::V4(Ipv4Addr::from(unsafe { field::<4>(bytes, len, at) }))
    } else {
        let at = offset_of!(libc::sockaddr_in6, sin6_addr);
        // SAFETY: as above.
        IpAddr::V6(Ipv6Addr::from(unsafe { field::<16>(bytes, len, at) }))
    })
}

/// How many bytes of `sa` may be read, at most `full`: its own length, which may be short
/// for a netmask (the rest counts as zero).
///
/// # Safety
///
/// `sa` points at a sockaddr.
#[cfg(target_os = "macos")]
unsafe fn readable(sa: *const libc::sockaddr, full: usize) -> usize {
    // SAFETY: as the caller's; `sa_len` is its first byte.
    usize::from(unsafe { (*sa).sa_len }).min(full)
}

/// How many bytes of `sa` may be read: Linux gives each its family's whole struct.
///
/// # Safety
///
/// `sa` points at a sockaddr of `full` bytes.
#[cfg(not(target_os = "macos"))]
unsafe fn readable(_sa: *const libc::sockaddr, full: usize) -> usize {
    full
}

/// The `N` bytes at `at` of the `len` readable bytes at `sa`, those past `len` zero.
///
/// # Safety
///
/// `len` bytes at `sa` are readable.
unsafe fn field<const N: usize>(sa: *const u8, len: usize, at: usize) -> [u8; N] {
    let mut out = [0u8; N];
    let n = len.saturating_sub(at).min(N);
    if n > 0 {
        // SAFETY: `at + n <= len`, all readable; `out` has room for `n <= N` bytes.
        unsafe { std::ptr::copy_nonoverlapping(sa.add(at), out.as_mut_ptr(), n) };
    }
    out
}
