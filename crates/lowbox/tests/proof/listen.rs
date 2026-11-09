//! The no-listener check: chrome speaks CDP over pipes, so none of its processes may listen on a
//! TCP port. The helper reads every listening TCP socket's owner (`GetExtendedTcpTable`,
//! `TCP_TABLE_OWNER_PID_LISTENER`, IPv4 and IPv6) and counts them for each process in chrome's
//! Job Object (`JobObjectBasicProcessIdList`), or for the inspected pids when the job cannot be
//! read. One `listen` line per pid; the owner asserts every count is zero.
#![expect(
    unsafe_code,
    reason = "FFI to GetExtendedTcpTable and QueryInformationJobObject"
)]

use std::ffi::c_void;
use std::os::windows::io::OwnedHandle;

use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::System::JobObjects::{
    JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList, QueryInformationJobObject,
};

use super::helper::say;
use super::win::{Res, failed, raw};

/// `AF_INET`.
const AF_INET: u32 = 2;
/// `AF_INET6`.
const AF_INET6: u32 = 23;
/// `ERROR_INSUFFICIENT_BUFFER`: the table grew between the size query and the read.
const INSUFFICIENT_BUFFER: u32 = 122;
/// The most pids the job list is read for.
const JOB_PIDS: usize = 1024;

/// The pids of the processes in `job` now.
fn job_pids(job: &OwnedHandle) -> Res<Vec<u32>> {
    let header = size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
    let bytes = header + JOB_PIDS * size_of::<usize>();
    let mut buffer = vec![0_u64; bytes.div_ceil(8)];
    let list = buffer
        .as_mut_ptr()
        .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
    // SAFETY: an open job with query access and a writable, aligned buffer of `bytes` bytes.
    unsafe {
        QueryInformationJobObject(
            Some(raw(job)),
            JobObjectBasicProcessIdList,
            list.cast::<c_void>(),
            u32::try_from(bytes).unwrap_or(u32::MAX),
            None,
        )
    }
    .map_err(|e| failed("QueryInformationJobObject(JobObjectBasicProcessIdList)", &e))?;
    // SAFETY: the call filled the header and `NumberOfProcessIdsInList` ids (at most JOB_PIDS,
    // the room given) from `ProcessIdList` on, all inside `buffer`.
    let pids = unsafe {
        let count = usize::try_from((*list).NumberOfProcessIdsInList).unwrap_or(0);
        let first = (&raw const (*list).ProcessIdList).cast::<usize>();
        (0..count.min(JOB_PIDS))
            .map(|i| u32::try_from(first.add(i).read()).unwrap_or(u32::MAX))
            .collect()
    };
    Ok(pids)
}

/// The listening table of `family` (`AF_INET` or `AF_INET6`) as raw bytes.
fn table(family: u32) -> Res<Vec<u64>> {
    let mut size = 0_u32;
    for _ in 0..4 {
        let mut buffer = vec![0_u64; usize::try_from(size).unwrap_or(0).div_ceil(8).max(1)];
        size = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
        // SAFETY: `buffer` is writable and aligned for `size` bytes and outlives the call.
        let rc = unsafe {
            GetExtendedTcpTable(
                Some(buffer.as_mut_ptr().cast::<c_void>()),
                &raw mut size,
                false,
                family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if rc == 0 {
            return Ok(buffer);
        }
        if rc != INSUFFICIENT_BUFFER {
            return Err(format!("GetExtendedTcpTable({family}) failed with {rc}"));
        }
    }
    Err(format!("GetExtendedTcpTable({family}) kept growing"))
}

/// The owner pid of every listening TCP socket, IPv4 and IPv6.
fn listener_pids() -> Res<Vec<u32>> {
    let mut owners = Vec::new();
    for (family, row) in [
        (AF_INET, size_of::<MIB_TCPROW_OWNER_PID>()),
        (AF_INET6, size_of::<MIB_TCP6ROW_OWNER_PID>()),
    ] {
        let buffer = table(family)?;
        let base = buffer.as_ptr().cast::<u8>();
        let bytes = buffer.len() * 8;
        // The table starts with a u32 count (little-endian, so the first word's low half), then
        // that many rows from offset 4.
        let count = usize::try_from(buffer.first().map_or(0, |w| w & 0xFFFF_FFFF)).unwrap_or(0);
        for i in 0..count {
            let at = 4 + i * row;
            if at + row > bytes {
                break;
            }
            // SAFETY: the row lies inside `buffer` (checked above); rows are read unaligned.
            let pid = unsafe {
                if family == AF_INET {
                    base.add(at)
                        .cast::<MIB_TCPROW_OWNER_PID>()
                        .read_unaligned()
                        .dwOwningPid
                } else {
                    base.add(at)
                        .cast::<MIB_TCP6ROW_OWNER_PID>()
                        .read_unaligned()
                        .dwOwningPid
                }
            };
            owners.push(pid);
        }
    }
    Ok(owners)
}

/// Prints `listen stage=<stage> source=<job|inspected> pid=<pid> listeners=<n>` for each pid of
/// `job` (else of `inspected`), or one `listen stage=<stage> error=…` line.
pub(crate) fn report(stage: &str, job: &OwnedHandle, inspected: &[u32]) {
    let (source, pids) = match job_pids(job) {
        Ok(pids) => ("job", pids),
        Err(e) => {
            say(format_args!(
                "listen stage={stage} job-list={}",
                e.replace(' ', "_")
            ));
            ("inspected", inspected.to_vec())
        }
    };
    let owners = match listener_pids() {
        Ok(owners) => owners,
        Err(e) => {
            say(format_args!(
                "listen stage={stage} error={}",
                e.replace(' ', "_")
            ));
            return;
        }
    };
    say(format_args!(
        "listen stage={stage} source={source} pids={}",
        pids.len()
    ));
    for pid in pids {
        let count = owners.iter().filter(|&&owner| owner == pid).count();
        say(format_args!(
            "listen stage={stage} source={source} pid={pid} listeners={count}"
        ));
    }
}
