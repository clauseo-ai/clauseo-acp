//! Windows' listeners and their owners: `GetExtendedTcpTable` with
//! `TCP_TABLE_OWNER_PID_LISTENER`, for IPv4 and IPv6. The tables are read into a `u32` buffer
//! (the rows' alignment) and each row is copied out with `read_unaligned`.
#![expect(unsafe_code, reason = "FFI to the IP Helper's GetExtendedTcpTable")]

use std::collections::BTreeSet;

use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};

/// `AF_INET` and `AF_INET6`, the address families the table is asked for.
const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
/// `NO_ERROR` and `ERROR_INSUFFICIENT_BUFFER`.
const NO_ERROR: u32 = 0;
const INSUFFICIENT_BUFFER: u32 = 122;
/// How many times the table is asked for again when it grew between the size and the read.
const TRIES: usize = 4;

/// The pids holding a listener on `port`, IPv4 and IPv6; `None` when a table cannot be read.
pub(super) fn listener_pids(port: u16) -> Option<BTreeSet<u32>> {
    let mut pids = BTreeSet::new();
    let v4 = table(AF_INET)?;
    for row in rows::<MIB_TCPROW_OWNER_PID>(&v4) {
        if local_port(row.dwLocalPort) == port {
            pids.insert(row.dwOwningPid);
        }
    }
    let v6 = table(AF_INET6)?;
    for row in rows::<MIB_TCP6ROW_OWNER_PID>(&v6) {
        if local_port(row.dwLocalPort) == port {
            pids.insert(row.dwOwningPid);
        }
    }
    Some(pids)
}

/// A row's port: the low 16 bits, in network byte order.
fn local_port(raw: u32) -> u16 {
    let bytes = raw.to_le_bytes();
    u16::from_be_bytes([bytes[0], bytes[1]])
}

/// The whole listener table of `family`, as the call wrote it.
fn table(family: u32) -> Option<Vec<u32>> {
    let mut size: u32 = 0;
    // SAFETY: a null table with a valid size pointer asks only for the size needed.
    let first = unsafe {
        GetExtendedTcpTable(
            None,
            &raw mut size,
            false,
            family,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if first != INSUFFICIENT_BUFFER && first != NO_ERROR {
        return None;
    }
    for _ in 0..TRIES {
        let words = usize::try_from(size).ok()?.div_ceil(4).max(1);
        let mut buffer = vec![0_u32; words];
        let mut given = u32::try_from(words.checked_mul(4)?).ok()?;
        // SAFETY: `buffer` holds `given` bytes, aligned for the table's rows, and outlives the
        // call; the size pointer is valid.
        let result = unsafe {
            GetExtendedTcpTable(
                Some(buffer.as_mut_ptr().cast()),
                &raw mut given,
                false,
                family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        match result {
            NO_ERROR => return Some(buffer),
            INSUFFICIENT_BUFFER => size = given,
            _ => return None,
        }
    }
    None
}

/// The rows of a table: its first `u32` is the count, the rows follow it.
fn rows<R: Copy>(buffer: &[u32]) -> Vec<R> {
    let Some((&count, rest)) = buffer.split_first() else {
        return Vec::new();
    };
    let row_size = size_of::<R>();
    let available = rest.len().saturating_mul(4) / row_size.max(1);
    let count = usize::try_from(count).unwrap_or(0).min(available);
    let base = rest.as_ptr().cast::<u8>();
    (0..count)
        .map(|index| {
            // SAFETY: `index < available`, so the row's bytes lie inside `rest`; the read is
            // unaligned-safe and `R` is a plain C struct of integers and byte arrays.
            unsafe { base.add(index * row_size).cast::<R>().read_unaligned() }
        })
        .collect()
}
