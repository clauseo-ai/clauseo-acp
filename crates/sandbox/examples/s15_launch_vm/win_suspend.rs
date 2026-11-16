//! The `cancel` row's look at suspended processes (`checks_cancel.rs`): every process with its
//! threads' states, from `NtQuerySystemInformation(SystemProcessInformation)`, and the token user
//! of one pid. Toolhelp gives no thread state, so the call is declared here against ntdll
//! directly, as `win_thread.rs` declares Toolhelp: the crate's `windows` features don't hold
//! `Wdk_System_SystemInformation`, and an example shouldn't widen the product's list.
//!
//! The buffer the call fills is read as `SYSTEM_PROCESS_INFORMATION` entries chained by
//! `NextEntryOffset`, each followed by its `SYSTEM_THREAD_INFORMATION` entries; every offset, count
//! and the image name's pointer is checked against the buffer before it is read.
#![expect(
    unsafe_code,
    reason = "FFI to NtQuerySystemInformation and the token calls"
)]

use std::ffi::c_void;
use std::mem::offset_of;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::TOKEN_QUERY;
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::win::{Owned, code_of, token_user};

/// `SystemProcessInformation`.
const SYSTEM_PROCESS_INFORMATION: u32 = 5;
/// `STATUS_INFO_LENGTH_MISMATCH`: the buffer was too small.
const INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32.cast_signed();
/// `KTHREAD_STATE::Waiting`.
const WAITING: u32 = 5;
/// `KWAIT_REASON::Suspended`.
const SUSPENDED: u32 = 5;
/// The largest buffer the listing asks for, in bytes.
const MOST: usize = 64 << 20;
/// How many times the listing grows its buffer before it gives up.
const TRIES: usize = 8;

/// `UNICODE_STRING`.
#[repr(C)]
#[derive(Clone, Copy)]
struct UnicodeString {
    length: u16,
    maximum: u16,
    buffer: *const u16,
}

/// `SYSTEM_PROCESS_INFORMATION`, the fields the harness reads and the rest as their sizes.
#[repr(C)]
#[derive(Clone, Copy)]
struct ProcessInfo {
    next: u32,
    threads: u32,
    working_set_private: i64,
    hard_faults: u32,
    threads_high_watermark: u32,
    cycle_time: u64,
    create_time: i64,
    user_time: i64,
    kernel_time: i64,
    image: UnicodeString,
    base_priority: i32,
    pid: usize,
    parent: usize,
    handles: u32,
    session: u32,
    key: usize,
    /// `PeakVirtualSize` and `VirtualSize`.
    sizes: [usize; 2],
    page_faults: u32,
    /// `PeakWorkingSetSize` to `PrivatePageCount`.
    more_sizes: [usize; 9],
    /// The read, write and other operation counts and transfer counts.
    io: [i64; 6],
}

/// `SYSTEM_THREAD_INFORMATION`.
#[repr(C)]
#[derive(Clone, Copy)]
struct ThreadInfo {
    kernel_time: i64,
    user_time: i64,
    create_time: i64,
    wait_time: u32,
    start: usize,
    client_process: usize,
    client_thread: usize,
    priority: i32,
    base_priority: i32,
    context_switches: u32,
    state: u32,
    wait_reason: u32,
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<ProcessInfo>() == 0x100);
    assert!(offset_of!(ProcessInfo, threads) == 0x4);
    assert!(offset_of!(ProcessInfo, image) == 0x38);
    assert!(offset_of!(ProcessInfo, pid) == 0x50);
    assert!(size_of::<ThreadInfo>() == 0x50);
    assert!(offset_of!(ThreadInfo, state) == 0x44);
    assert!(offset_of!(ThreadInfo, wait_reason) == 0x48);
};

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, needed: *mut u32) -> i32;
}

/// One process and its threads' states.
#[derive(Debug, Clone)]
pub(crate) struct Threads {
    /// Its pid.
    pub(crate) pid: u32,
    /// Its image's file name (empty for the idle process).
    pub(crate) image: String,
    /// Whether it has threads and every one is waiting, suspended (`Waiting`, `Suspended`).
    pub(crate) suspended: bool,
}

/// Every process with its threads' states; the `NTSTATUS` when the call fails.
pub(crate) fn thread_states() -> Result<Vec<Threads>, i32> {
    let mut room = 1usize << 20;
    for _ in 0..TRIES {
        let mut buffer = vec![0u64; room.div_ceil(size_of::<u64>())];
        let len = u32::try_from(size_of_val(buffer.as_slice())).unwrap_or(u32::MAX);
        let mut needed = 0u32;
        // SAFETY: `buffer` is live, writable, `len` bytes and aligned for the entries; `needed`
        // is a live out-pointer.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PROCESS_INFORMATION,
                buffer.as_mut_ptr().cast::<c_void>(),
                len,
                &raw mut needed,
            )
        };
        if status == INFO_LENGTH_MISMATCH {
            let asked = usize::try_from(needed).unwrap_or(MOST);
            room = asked.saturating_add(64 << 10).max(room.saturating_mul(2));
            if room > MOST {
                return Err(status);
            }
            continue;
        }
        if status < 0 {
            return Err(status);
        }
        return Ok(walk(&buffer));
    }
    Err(INFO_LENGTH_MISMATCH)
}

/// The entries in `buffer`, as the call filled it. An entry, or a thread list, that would run
/// past the buffer ends the walk; an image name whose pointer leaves the buffer reads as `?`.
fn walk(buffer: &[u64]) -> Vec<Threads> {
    let base = buffer.as_ptr().cast::<u8>();
    let len = size_of_val(buffer);
    let bytes: Vec<u8> = buffer.iter().flat_map(|word| word.to_ne_bytes()).collect();
    let mut found = Vec::new();
    let mut at = 0usize;
    while let Some(head_end) = at
        .checked_add(size_of::<ProcessInfo>())
        .filter(|end| *end <= len)
    {
        // SAFETY: `at..head_end` is inside `buffer`; the read is unaligned-safe and copies.
        let entry = unsafe { base.add(at).cast::<ProcessInfo>().read_unaligned() };
        let count = usize::try_from(entry.threads).unwrap_or(usize::MAX);
        let fits = count
            .checked_mul(size_of::<ThreadInfo>())
            .and_then(|room| head_end.checked_add(room))
            .is_some_and(|end| end <= len);
        if !fits {
            break;
        }
        let suspended = count > 0
            && (0..count).all(|i| {
                let offset = head_end + i * size_of::<ThreadInfo>();
                // SAFETY: `offset` plus one entry ends inside `buffer` (`fits`);
                // the read is unaligned-safe and copies.
                let thread = unsafe { base.add(offset).cast::<ThreadInfo>().read_unaligned() };
                thread.state == WAITING && thread.wait_reason == SUSPENDED
            });
        found.push(Threads {
            pid: u32::try_from(entry.pid).unwrap_or(u32::MAX),
            image: image_name(&bytes, base.addr(), entry.image),
            suspended,
        });
        if entry.next == 0 {
            break;
        }
        let next = usize::try_from(entry.next).unwrap_or(usize::MAX);
        match at.checked_add(next) {
            Some(after) if after > at => at = after,
            _ => break,
        }
    }
    found
}

/// The text of `name`, when its buffer lies inside `bytes`, the call's buffer copied (which was
/// at `base`; the call writes the names there); empty for no name, `?` for one outside.
fn image_name(bytes: &[u8], base: usize, name: UnicodeString) -> String {
    if name.buffer.is_null() || name.length == 0 {
        return String::new();
    }
    let start = name.buffer.addr().wrapping_sub(base);
    let Some(text) = start
        .checked_add(usize::from(name.length))
        .filter(|_| start < bytes.len())
        .and_then(|end| bytes.get(start..end))
    else {
        return "?".to_owned();
    };
    let units: Vec<u16> = text
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_ne_bytes(*pair))
        .collect();
    String::from_utf16_lossy(&units)
}

/// The token user of the process `pid`, as text (`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`
/// and `OpenProcessToken(TOKEN_QUERY)`); the Win32 code of the call that failed otherwise.
pub(crate) fn user_of(pid: u32) -> Result<String, u32> {
    // SAFETY: plain call; a handle that comes back is closed by `Owned`.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map(Owned)
        .map_err(|e| code_of(&e))?;
    let mut token = HANDLE::default();
    // SAFETY: the process handle is live; `token` is a live out-pointer, closed by `Owned`.
    unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &raw mut token) }.map_err(|e| code_of(&e))?;
    let token = Owned(token);
    token_user(token.0).map_err(|e| e.raw_os_error().map_or(0, i32::cast_unsigned))
}
