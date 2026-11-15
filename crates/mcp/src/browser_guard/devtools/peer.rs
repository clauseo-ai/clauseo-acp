//! The relay's second lock, beside the secret: the process at the other end of each connection
//! must be agent-browser (the guard's child, [`super::Devtools::allow_descendants_of`]) or one of
//! its descendants, its daemon among them. Anything else is closed before a byte is read.
//!
//! The secret alone may not stay secret: on macOS and Linux a process of the same user can read
//! another's environment (`ps eww`, `KERN_PROCARGS2`; `/proc/<pid>/environ`), and agent-browser's
//! holds `AGENT_BROWSER_CDP`. So the relay finds who owns the client's socket, by its address
//! pair (the client's port, the relay's port, both on 127.0.0.1):
//!
//! - Linux: the socket's inode in `/proc/net/tcp`, then a process among agent-browser's
//!   descendants (by `/proc/<pid>/stat`'s parent pid) holding `socket:[<inode>]` in its
//!   `/proc/<pid>/fd`.
//! - macOS: a process among agent-browser's descendants (`proc_listchildpids`) holding a TCP
//!   socket with that pair (`proc_pidinfo(PROC_PIDLISTFDS)`,
//!   `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)`).
//! - Windows: the socket's owning pid from `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_ALL)`, then
//!   its parent chain (a Toolhelp snapshot) up to agent-browser, each parent created no later
//!   than its child, so a parent pid Windows reused is never taken for agent-browser. A process
//!   cannot claim agent-browser as its parent (`PROC_THREAD_ATTRIBUTE_PARENT_PROCESS` needs
//!   `PROCESS_CREATE_PROCESS` on it, which its hardened DACL denies, `harden.rs`).
//!
//! On macOS and Linux the walk goes down from agent-browser, so a process another one reparented
//! is never found: an orphan's parent becomes init or launchd, not agent-browser. The browser
//! (the guard's child, beside agent-browser) is never let in.

use std::net::TcpStream;

/// The most processes walked from agent-browser down (macOS, Linux) or from the client up
/// (Windows).
const MAX_PROCESSES: usize = 4096;

/// Whether the process at the other end of `stream` is `root` or one of its descendants.
pub(crate) fn allowed(stream: &TcpStream, root: u32) -> bool {
    let (Ok(peer), Ok(local)) = (stream.peer_addr(), stream.local_addr()) else {
        return false;
    };
    peer.ip().is_loopback() && os::owned_within(peer.port(), local.port(), root)
}

/// One `/proc/net/tcp` line's inode, when it is the client socket from `client` to `relay`, both
/// on 127.0.0.1. Pure.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn tcp_line_inode(line: &str, client: u16, relay: u16) -> Option<u64> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let endpoint = |field: &str| -> Option<(u32, u16)> {
        let (address, port) = field.split_once(':')?;
        Some((
            u32::from_str_radix(address, 16).ok()?,
            u16::from_str_radix(port, 16).ok()?,
        ))
    };
    // `/proc/net/tcp` prints the network-order address as a host-order word.
    let loopback = u32::from_ne_bytes([127, 0, 0, 1]);
    let (local, remote) = (endpoint(fields.get(1)?)?, endpoint(fields.get(2)?)?);
    if local != (loopback, client) || remote != (loopback, relay) {
        return None;
    }
    fields.get(9)?.parse().ok()
}

/// A `/proc/<pid>/stat` line's parent pid. Pure.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn stat_parent(stat: &str) -> Option<u32> {
    // `pid (comm) state ppid ...`, where comm may hold spaces and parentheses.
    let after = stat.get(stat.rfind(')')? + 1..)?;
    after.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(target_os = "linux")]
mod os {
    use std::collections::{BTreeMap, VecDeque};

    use super::{MAX_PROCESSES, stat_parent, tcp_line_inode};

    pub(super) fn owned_within(client: u16, relay: u16, root: u32) -> bool {
        let Ok(table) = std::fs::read_to_string("/proc/net/tcp") else {
            return false;
        };
        let Some(inode) = table
            .lines()
            .skip(1)
            .find_map(|line| tcp_line_inode(line, client, relay))
        else {
            return false;
        };
        let link = format!("socket:[{inode}]");
        descendants(root).into_iter().any(|pid| holds(pid, &link))
    }

    /// `root` and every process below it.
    fn descendants(root: u32) -> Vec<u32> {
        let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten().take(MAX_PROCESSES * 8) {
                let Some(pid) = entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.parse().ok())
                else {
                    continue;
                };
                let stat = std::fs::read_to_string(entry.path().join("stat")).unwrap_or_default();
                if let Some(parent) = stat_parent(&stat) {
                    children.entry(parent).or_default().push(pid);
                }
            }
        }
        let mut found = Vec::new();
        let mut queue = VecDeque::from([root]);
        while let Some(pid) = queue.pop_front() {
            if found.len() >= MAX_PROCESSES || found.contains(&pid) {
                continue;
            }
            found.push(pid);
            queue.extend(children.get(&pid).into_iter().flatten());
        }
        found
    }

    /// Whether `pid` holds an fd linking to `link`.
    fn holds(pid: u32, link: &str) -> bool {
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            return false;
        };
        fds.flatten()
            .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|target| target.as_os_str() == link))
    }
}

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "FFI to libproc, to find which process holds the relay client's socket"
)]
mod os {
    use std::collections::VecDeque;
    use std::ffi::{c_int, c_void};

    use super::MAX_PROCESSES;

    /// `PROC_PIDFDSOCKETINFO` (`sys/proc_info.h`).
    const PROC_PIDFDSOCKETINFO: c_int = 3;
    /// `SOCKINFO_TCP`, a `socket_info`'s `soi_kind`.
    const SOCKINFO_TCP: i32 = 2;
    /// `AF_INET`.
    const AF_INET: i32 = 2;
    /// Offsets into `struct socket_fdinfo` (`sys/proc_info.h`): `pfi` is a 24-byte
    /// `proc_fileinfo`; in `psi`, `soi_family` sits at 160 and `soi_kind` at 232 (after the
    /// 136-byte `vinfo_stat`, two `uint64_t`, three `int`, eight `short`, a `uint32_t` and two
    /// 24-byte `sockbuf_info`), and `soi_proto` at 240, whose `tcp_sockinfo` starts with the
    /// `in_sockinfo` fields `insi_fport` and `insi_lport` (ports in network order in an `int`).
    const FAMILY_AT: usize = 24 + 160;
    const KIND_AT: usize = 24 + 232;
    const FOREIGN_PORT_AT: usize = 24 + 240;
    const LOCAL_PORT_AT: usize = 24 + 244;
    /// Room for a whole `socket_fdinfo` (about 800 bytes in every macOS so far).
    const SOCKET_INFO_BYTES: usize = 2048;

    pub(super) fn owned_within(client: u16, relay: u16, root: u32) -> bool {
        descendants(root)
            .into_iter()
            .any(|pid| holds(pid, client, relay))
    }

    /// `root` and every process below it.
    fn descendants(root: u32) -> Vec<i32> {
        let Ok(root) = i32::try_from(root) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        let mut queue = VecDeque::from([root]);
        while let Some(pid) = queue.pop_front() {
            if found.len() >= MAX_PROCESSES || found.contains(&pid) || pid <= 0 {
                continue;
            }
            found.push(pid);
            queue.extend(children(pid));
        }
        found
    }

    fn children(pid: i32) -> Vec<i32> {
        let mut buffer = vec![0_i32; 1024];
        let bytes = c_int::try_from(buffer.len() * size_of::<i32>()).unwrap_or(0);
        // SAFETY: `buffer` is `bytes` writable bytes, live for the call.
        let count = unsafe { libc::proc_listchildpids(pid, buffer.as_mut_ptr().cast(), bytes) };
        // Whether the call counts pids or bytes, the entries past the real ones stay 0.
        let count = usize::try_from(count).unwrap_or(0).min(buffer.len());
        buffer.truncate(count);
        buffer.retain(|child| *child > 0);
        buffer
    }

    /// Whether `pid` holds a TCP socket from 127.0.0.1:`client` to the relay's `relay`.
    fn holds(pid: i32, client: u16, relay: u16) -> bool {
        let entry = size_of::<libc::proc_fdinfo>();
        let mut fds = vec![
            libc::proc_fdinfo {
                proc_fd: 0,
                proc_fdtype: 0,
            };
            4096
        ];
        let bytes = c_int::try_from(fds.len() * entry).unwrap_or(0);
        // SAFETY: `fds` is `bytes` writable bytes of `proc_fdinfo`, live for the call.
        let filled = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                fds.as_mut_ptr().cast(),
                bytes,
            )
        };
        let count = usize::try_from(filled).unwrap_or(0) / entry;
        fds.iter()
            .take(count)
            .filter(|fd| {
                u32::try_from(libc::PROX_FDTYPE_SOCKET).is_ok_and(|socket| fd.proc_fdtype == socket)
            })
            .any(|fd| socket_matches(pid, fd.proc_fd, client, relay))
    }

    fn socket_matches(pid: i32, fd: i32, client: u16, relay: u16) -> bool {
        let mut info = vec![0_u64; SOCKET_INFO_BYTES / 8];
        let bytes = c_int::try_from(SOCKET_INFO_BYTES).unwrap_or(0);
        // SAFETY: `info` is `bytes` writable bytes, aligned for the struct, live for the call.
        let filled = unsafe {
            libc::proc_pidfdinfo(
                pid,
                fd,
                PROC_PIDFDSOCKETINFO,
                info.as_mut_ptr().cast::<c_void>(),
                bytes,
            )
        };
        if usize::try_from(filled).unwrap_or(0) < LOCAL_PORT_AT + 4 {
            return false;
        }
        let raw: Vec<u8> = info.iter().flat_map(|word| word.to_ne_bytes()).collect();
        let int_at = |at: usize| {
            raw.get(at..at + 4)
                .and_then(|bytes| bytes.try_into().ok())
                .map_or(-1, i32::from_ne_bytes)
        };
        let port_at = |at: usize| u16::from_be(u16::try_from(int_at(at) & 0xFFFF).unwrap_or(0));
        int_at(KIND_AT) == SOCKINFO_TCP
            && int_at(FAMILY_AT) == AF_INET
            && port_at(LOCAL_PORT_AT) == client
            && port_at(FOREIGN_PORT_AT) == relay
    }
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "FFI to the IP Helper and Toolhelp APIs, to find which process owns the relay \
              client's socket"
)]
mod os {
    use std::collections::BTreeMap;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};

    use windows::Win32::Foundation::{FILETIME, HANDLE, NO_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows::Win32::Networking::WinSock::AF_INET;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    use super::MAX_PROCESSES;

    pub(super) fn owned_within(client: u16, relay: u16, root: u32) -> bool {
        let Some(owner) = owner(client, relay) else {
            return false;
        };
        let parents = parents();
        let mut pid = owner;
        for _ in 0..MAX_PROCESSES {
            if pid == root {
                return true;
            }
            let Some(&parent) = parents.get(&pid) else {
                return false;
            };
            // A parent created after its child is a reused pid, not the child's parent.
            match (created(parent), created(pid)) {
                (Some(born), Some(child)) if born <= child && parent != pid => pid = parent,
                _ => return false,
            }
        }
        false
    }

    /// The pid owning the IPv4 TCP connection from 127.0.0.1:`client` to 127.0.0.1:`relay`.
    fn owner(client: u16, relay: u16) -> Option<u32> {
        let family = u32::from(AF_INET.0);
        let mut size = 0_u32;
        // SAFETY: with no table the call only writes the size it needs into `size`; its error
        // code (ERROR_INSUFFICIENT_BUFFER) is expected.
        let _sizing: u32 = unsafe {
            GetExtendedTcpTable(
                None,
                &raw mut size,
                false,
                family,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        // Room for rows added since.
        let mut buffer = vec![0_u32; usize::try_from(size).ok()?.div_ceil(4) + 64];
        let mut size = u32::try_from(buffer.len() * 4).ok()?;
        // SAFETY: `buffer` is `size` writable bytes, aligned for the table, live for the call.
        let read = unsafe {
            GetExtendedTcpTable(
                Some(buffer.as_mut_ptr().cast()),
                &raw mut size,
                false,
                family,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if read != NO_ERROR.0 {
            return None;
        }
        let count = usize::try_from(*buffer.first()?).ok()?;
        let words = size_of::<MIB_TCPROW_OWNER_PID>() / 4;
        let loopback = u32::from_ne_bytes([127, 0, 0, 1]);
        let port = |word: u32| u16::from_be(u16::try_from(word & 0xFFFF).unwrap_or(0));
        (0..count).find_map(|row| {
            let at = 1 + row * words;
            let row = buffer.get(at..at + words)?;
            let (local, local_port, remote, remote_port, pid) =
                (row[1], row[2], row[3], row[4], row[5]);
            (local == loopback
                && remote == loopback
                && port(local_port) == client
                && port(remote_port) == relay)
                .then_some(pid)
        })
    }

    /// Each process's parent pid, from a snapshot.
    fn parents() -> BTreeMap<u32, u32> {
        let mut parents = BTreeMap::new();
        // SAFETY: plain values; the handle returned is owned and closed by `snapshot`.
        let Ok(raw) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return parents;
        };
        // SAFETY: `raw` is the open handle just returned, which nothing else owns.
        let snapshot = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let handle = HANDLE(std::os::windows::io::AsRawHandle::as_raw_handle(&snapshot));
        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(0),
            ..PROCESSENTRY32W::default()
        };
        // SAFETY: `handle` is the open snapshot and `entry` a live PROCESSENTRY32W whose
        // `dwSize` is its size.
        let mut more = unsafe { Process32FirstW(handle, &raw mut entry) }.is_ok();
        while more && parents.len() < MAX_PROCESSES * 8 {
            parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
            // SAFETY: as above.
            more = unsafe { Process32NextW(handle, &raw mut entry) }.is_ok();
        }
        parents
    }

    /// When process `pid` was created, in 100 ns units; none if it cannot be opened.
    fn created(pid: u32) -> Option<u64> {
        // SAFETY: plain values; the handle returned is owned and closed by `process`.
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        // SAFETY: `raw` is the open handle just returned, which nothing else owns.
        let process = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let handle = HANDLE(std::os::windows::io::AsRawHandle::as_raw_handle(&process));
        let mut times = [FILETIME::default(); 4];
        let [creation, exit, kernel, user] = &mut times;
        // SAFETY: `handle` is open with PROCESS_QUERY_LIMITED_INFORMATION; the four FILETIMEs
        // are live locals the call writes.
        unsafe { GetProcessTimes(handle, creation, exit, kernel, user) }.ok()?;
        Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
    }
}
