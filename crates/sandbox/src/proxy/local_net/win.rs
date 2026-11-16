//! The interface list on Windows: `GetAdaptersAddresses`, each unicast address with the
//! on-link prefix length Windows keeps for it, and each adapter's DNS suffix (the firm's own
//! names, `upstream/firm.rs`).
#![expect(
    unsafe_code,
    reason = "GetAdaptersAddresses, and reading the adapter list it writes into our buffer"
)]

use std::io;
use std::mem::offset_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA, ERROR_SUCCESS};
use windows::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses,
    IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::Networking::WinSock::{
    ADDRESS_FAMILY, AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6, SOCKET_ADDRESS,
};

use super::OnLink;

/// The first buffer's size; Windows says when it needs more.
const FIRST_SIZE: u32 = 16 * 1024;

/// Every IPv4 and IPv6 unicast address of every adapter, with its on-link prefix length.
pub(super) fn read() -> io::Result<Vec<OnLink>> {
    adapters(walk)
}

/// Every adapter's connection-specific DNS suffix (set by DHCP or by hand), where it has one.
pub(crate) fn dns_suffixes() -> io::Result<Vec<String>> {
    adapters(suffixes)
}

/// `GetAdaptersAddresses`'s list, read by `walker` while its buffer lives; empty when there is
/// no adapter.
fn adapters<T: Default>(walker: unsafe fn(*const IP_ADAPTER_ADDRESSES_LH) -> T) -> io::Result<T> {
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size = FIRST_SIZE;
    // The list can grow between the call that sizes it and the next; three tries.
    for _ in 0..3 {
        // In u64 words, so the buffer is aligned for `IP_ADAPTER_ADDRESSES_LH`.
        let words = usize::try_from(size).unwrap_or(usize::MAX).div_ceil(8);
        let mut buffer = vec![0u64; words];
        let first = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: `first` points at `size` writable bytes, aligned for the struct; Windows
        // writes at most `size` bytes, or sets `size` to what it needs.
        let status = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC.0),
                flags,
                None,
                Some(first),
                &raw mut size,
            )
        };
        if status == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if status == ERROR_NO_DATA.0 {
            return Ok(T::default());
        }
        if status != ERROR_SUCCESS.0 {
            return Err(io::Error::other(format!(
                "GetAdaptersAddresses failed: {status}"
            )));
        }
        // SAFETY: on success the list starts at `first`, inside `buffer`, alive here, which is
        // what `walker` asks for.
        return Ok(unsafe { walker(first.cast_const()) });
    }
    Err(io::Error::other("the adapter list kept growing"))
}

/// The unicast addresses of the adapter list starting at `adapter`.
///
/// # Safety
///
/// `adapter` is null or the head of a list `GetAdaptersAddresses` wrote, still alive.
unsafe fn walk(mut adapter: *const IP_ADAPTER_ADDRESSES_LH) -> Vec<OnLink> {
    let mut found = Vec::new();
    while !adapter.is_null() {
        // SAFETY: a node of the list, as the caller vouches.
        let node = unsafe { &*adapter };
        let mut unicast = node.FirstUnicastAddress.cast_const();
        while !unicast.is_null() {
            // SAFETY: a unicast node of that adapter, in the same list.
            let address = unsafe { &*unicast };
            // SAFETY: its sockaddr is null or `iSockaddrLength` bytes in the same list.
            if let Some(ip) = unsafe { ip_of(&address.Address) } {
                found.push(OnLink::new(ip, address.OnLinkPrefixLength));
            }
            unicast = address.Next.cast_const();
        }
        adapter = node.Next.cast_const();
    }
    found
}

/// The DNS suffixes of the adapter list starting at `adapter`; an empty one is skipped.
///
/// # Safety
///
/// `adapter` is null or the head of a list `GetAdaptersAddresses` wrote, still alive.
unsafe fn suffixes(mut adapter: *const IP_ADAPTER_ADDRESSES_LH) -> Vec<String> {
    let mut found = Vec::new();
    while !adapter.is_null() {
        // SAFETY: a node of the list, as the caller vouches.
        let node = unsafe { &*adapter };
        if !node.DnsSuffix.is_null() {
            // SAFETY: a non-null suffix is a NUL-terminated UTF-16 string in the same list.
            if let Ok(suffix) = unsafe { node.DnsSuffix.to_string() }
                && !suffix.trim().is_empty()
            {
                found.push(suffix);
            }
        }
        adapter = node.Next.cast_const();
    }
    found
}

/// The IPv4 or IPv6 address in `address`, or `None` for another family.
///
/// # Safety
///
/// `address.lpSockaddr` is null or points at `address.iSockaddrLength` readable bytes.
unsafe fn ip_of(address: &SOCKET_ADDRESS) -> Option<IpAddr> {
    let sa = address.lpSockaddr.cast_const();
    let len = usize::try_from(address.iSockaddrLength).unwrap_or(0);
    if sa.is_null() || len < size_of::<ADDRESS_FAMILY>() {
        return None;
    }
    // SAFETY: at least the family's bytes are readable, and a SOCKADDR starts with it.
    let family = unsafe { (*sa).sa_family };
    let bytes = sa.cast::<u8>();
    if family == AF_INET {
        let at = offset_of!(SOCKADDR_IN, sin_addr);
        // SAFETY: `len` bytes at `bytes` are readable.
        Some(IpAddr::V4(Ipv4Addr::from(unsafe {
            field::<4>(bytes, len, at)
        })))
    } else if family == AF_INET6 {
        let at = offset_of!(SOCKADDR_IN6, sin6_addr);
        // SAFETY: as above.
        Some(IpAddr::V6(Ipv6Addr::from(unsafe {
            field::<16>(bytes, len, at)
        })))
    } else {
        None
    }
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
