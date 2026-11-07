//! The harness's positive control for the UI-limit effect, of the agent's kind: a Low restricted
//! token made from the harness's own (the lawyer's) token the way `src/windows/token/os.rs`
//! makes the agent's from the runner's ([`control_token`]), and a process started with it on the
//! control station's desktop ([`run_as`]), outside any job of ours. The example can't call the
//! crate's `pub(in crate::windows)` token code, so the calls are mirrored here. A restricted copy
//! of the caller's own primary token is one `CreateProcessAsUserW` may assign with no privilege.
#![expect(unsafe_code, reason = "FFI to the token, pipe and process calls")]

use std::ffi::c_void;
use std::io::Read as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Foundation::{
    HANDLE, HANDLE_FLAG_INHERIT, HLOCAL, LocalFree, SetHandleInformation, WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    ACL, CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, GetLengthSid, GetSecurityDescriptorDacl,
    PSECURITY_DESCRIPTOR, PSID, SID_AND_ATTRIBUTES, SetTokenInformation, TOKEN_ASSIGN_PRIMARY,
    TOKEN_DEFAULT_DACL, TOKEN_DUPLICATE, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenDefaultDacl,
    TokenIntegrityLevel,
};
use windows::Win32::System::JobObjects::IsProcessInJob;
use windows::Win32::System::SystemServices::{SE_GROUP_INTEGRITY, SE_GROUP_LOGON_ID};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessAsUserW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcessToken,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    STARTUPINFOW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::core::{BOOL, HSTRING, PCWSTR, PWSTR};

use super::launch::{note_ended, note_started};
use super::win::{self, Owned, code_of};

/// The Low mandatory level.
pub(crate) const LOW: &str = "S-1-16-4096";
/// The restricting SIDs after the harness's own user: `RESTRICTED`, `Users`, `Everyone`.
const RESTRICTING: [&str; 3] = ["S-1-5-12", "S-1-5-32-545", "S-1-1-0"];
/// `SE_GROUP_LOGON_ID`, as the `u32` the attributes are.
const LOGON_ID: u32 = SE_GROUP_LOGON_ID.cast_unsigned();

/// `call` failed with the Win32 code in `error`, as the INFO line names it.
fn gave(call: &str, error: &windows::core::Error) -> String {
    format!("{call} gave {}", code_of(error))
}

/// A SID made from its text, freed on drop.
struct LocalSid(PSID);

impl LocalSid {
    fn new(sid: &str) -> Result<LocalSid, String> {
        let mut out = PSID::default();
        // SAFETY: the text is a live HSTRING; `out` is a live out-pointer, freed on drop.
        unsafe { ConvertStringSidToSidW(&HSTRING::from(sid), &raw mut out) }
            .map_err(|e| gave(&format!("ConvertStringSidToSidW({sid})"), &e))?;
        Ok(LocalSid(out))
    }

    fn entry(&self) -> SID_AND_ATTRIBUTES {
        SID_AND_ATTRIBUTES {
            Sid: self.0,
            Attributes: 0,
        }
    }
}

impl Drop for LocalSid {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSidToSidW with LocalAlloc, freed once, here.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// The control token, the agent's shape made from the harness's own token:
/// `CreateRestrictedToken(own, DISABLE_MAX_PRIVILEGE, deny-only {the logon SID}, restricting
/// {own user, S-1-5-12, S-1-5-32-545, S-1-1-0})`, `TokenIntegrityLevel` [`LOW`], and
/// `TokenDefaultDacl` `D:(A;;GA;;;<own user>)(A;;GA;;;SY)`. The token and the harness's user's
/// SID; the call that failed and its code otherwise.
pub(crate) fn control_token() -> Result<(Owned, String), String> {
    let facts = win::own_token().map_err(|e| {
        let code = e.raw_os_error().unwrap_or(-1);
        format!("GetTokenInformation (the harness's token) gave {code}")
    })?;
    let logon: Vec<&str> = facts
        .groups
        .iter()
        .filter(|(_, attributes)| attributes & LOGON_ID == LOGON_ID)
        .map(|(sid, _)| sid.as_str())
        .collect();
    let [logon] = logon.as_slice() else {
        return Err(format!(
            "GetTokenInformation(TokenGroups) gave {} logon SIDs, not one",
            logon.len()
        ));
    };
    let mut base = HANDLE::default();
    let access = TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY;
    // SAFETY: the pseudo-handle needs no closing; `base` is a live out-pointer, owned below.
    unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut base) }
        .map_err(|e| gave("OpenProcessToken", &e))?;
    let base = Owned(base);
    let deny = [LocalSid::new(logon)?];
    let mut restricting = vec![LocalSid::new(&facts.user)?];
    for sid in RESTRICTING {
        restricting.push(LocalSid::new(sid)?);
    }
    let deny: Vec<SID_AND_ATTRIBUTES> = deny.iter().map(LocalSid::entry).collect();
    let restricting_entries: Vec<SID_AND_ATTRIBUTES> =
        restricting.iter().map(LocalSid::entry).collect();
    let mut token = HANDLE::default();
    // SAFETY: `base` is live with TOKEN_DUPLICATE; the arrays point at SIDs owned by `deny`'s
    // and `restricting`'s `LocalSid`s, which outlive the call; `token` is owned below.
    unsafe {
        CreateRestrictedToken(
            base.0,
            DISABLE_MAX_PRIVILEGE,
            Some(&deny),
            None,
            Some(&restricting_entries),
            &raw mut token,
        )
    }
    .map_err(|e| gave("CreateRestrictedToken", &e))?;
    let token = Owned(token);
    set_integrity(&token)?;
    set_default_dacl(&token, &format!("D:(A;;GA;;;{})(A;;GA;;;SY)", facts.user))?;
    Ok((token, facts.user))
}

/// Sets `token`'s integrity level to [`LOW`].
fn set_integrity(token: &Owned) -> Result<(), String> {
    let sid = LocalSid::new(LOW)?;
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid.0,
            Attributes: SE_GROUP_INTEGRITY.cast_unsigned(),
        },
    };
    // SAFETY: the SID is valid while `sid` lives.
    let sid_len = unsafe { GetLengthSid(sid.0) };
    let len = u32::try_from(size_of::<TOKEN_MANDATORY_LABEL>())
        .unwrap_or(u32::MAX)
        .saturating_add(sid_len);
    // SAFETY: `label` and the SID it points at outlive the call; `len` covers both.
    unsafe {
        SetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            (&raw const label).cast::<c_void>(),
            len,
        )
    }
    .map_err(|e| gave("SetTokenInformation(TokenIntegrityLevel)", &e))
}

/// Sets `token`'s default DACL to the one `sddl` describes.
fn set_default_dacl(token: &Owned, sddl: &str) -> Result<(), String> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the text is a live HSTRING; `descriptor` is a live out-pointer, freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(sddl),
            SDDL_REVISION_1,
            &raw mut descriptor,
            None,
        )
    }
    .map_err(|e| gave("ConvertStringSecurityDescriptorToSecurityDescriptorW", &e))?;
    let mut present = BOOL(0);
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut defaulted = BOOL(0);
    // SAFETY: the descriptor is live until freed below; the out-pointers are live.
    let read = unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &raw mut present,
            &raw mut dacl,
            &raw mut defaulted,
        )
    }
    .map_err(|e| gave("GetSecurityDescriptorDacl", &e));
    let set = read.and_then(|()| {
        let info = TOKEN_DEFAULT_DACL { DefaultDacl: dacl };
        // SAFETY: `info` points at the DACL inside the descriptor, freed only after the call.
        unsafe {
            SetTokenInformation(
                token.0,
                TokenDefaultDacl,
                (&raw const info).cast::<c_void>(),
                u32::try_from(size_of::<TOKEN_DEFAULT_DACL>()).unwrap_or(u32::MAX),
            )
        }
        .map_err(|e| gave("SetTokenInformation(TokenDefaultDacl)", &e))
    });
    // SAFETY: allocated by the conversion with LocalAlloc, freed once, after its last use.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    set
}

/// Whether the harness is in a job, and that job's UI limits, as one note line: the control
/// process starts outside any job of ours, but inherits whatever job the harness runs in (on
/// CI, the runner service's).
pub(crate) fn own_job() -> String {
    let mut inside = BOOL(0);
    // SAFETY: the pseudo-handle needs no closing; `inside` is a live out-pointer.
    let asked = unsafe { IsProcessInJob(GetCurrentProcess(), None, &raw mut inside) };
    let limits = win::job_ui_limits().map_or_else(
        |e| format!("unread ({})", e.raw_os_error().unwrap_or(-1)),
        |l| format!("{l:#x}"),
    );
    match asked {
        Ok(()) => format!(
            "the harness is in a job: {}; its innermost job's UI limits: {limits}",
            inside.as_bool()
        ),
        Err(e) => format!("IsProcessInJob gave {}; UI limits: {limits}", code_of(&e)),
    }
}

/// An attribute list holding one handle list, deleted on drop.
struct Attributes {
    buffer: Vec<u64>,
}

impl Attributes {
    fn list(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        LPPROC_THREAD_ATTRIBUTE_LIST(self.buffer.as_mut_ptr().cast())
    }

    /// A list whose only attribute is `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` of `handles`, which
    /// must outlive it.
    fn inherit_only(handles: &[HANDLE; 1]) -> Result<Attributes, String> {
        let mut size = 0usize;
        // SAFETY: no list and a size out-pointer ask only for the size; the call then fails
        // with ERROR_INSUFFICIENT_BUFFER, which is expected.
        drop(unsafe { InitializeProcThreadAttributeList(None, 1, None, &raw mut size) });
        let mut buffer = vec![0u64; size.div_ceil(8).max(1)];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(buffer.as_mut_ptr().cast());
        // SAFETY: the buffer is writable for at least `size` bytes, as asked.
        unsafe { InitializeProcThreadAttributeList(Some(list), 1, None, &raw mut size) }
            .map_err(|e| gave("InitializeProcThreadAttributeList", &e))?;
        // Only an initialised list goes in the guard, which deletes it on drop.
        let mut made = Attributes { buffer };
        // SAFETY: the list is initialised; `handles` outlives it (the caller's promise).
        unsafe {
            UpdateProcThreadAttribute(
                made.list(),
                0,
                usize::try_from(PROC_THREAD_ATTRIBUTE_HANDLE_LIST).unwrap_or(0),
                Some(handles.as_ptr().cast()),
                size_of_val(handles),
                None,
                None,
            )
        }
        .map_err(|e| gave("UpdateProcThreadAttribute(HANDLE_LIST)", &e))?;
        Ok(made)
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: the guard holds only a list `inherit_only` initialised; deleted once, here.
        unsafe { DeleteProcThreadAttributeList(self.list()) };
    }
}

/// What the control process did: its exit code (`None` when it was ended at the limit) and its
/// stdout and stderr lines.
#[derive(Debug)]
pub(crate) struct Ran {
    /// Its pid.
    pub(crate) pid: u32,
    /// Its exit code; `None` when it overran `limit` and was ended.
    pub(crate) code: Option<u32>,
    /// What it printed, stdout and stderr together.
    pub(crate) lines: Vec<String>,
}

/// Starts `program args` with `CreateProcessAsUserW(token, lpDesktop = desktop)` in `cwd`, its
/// stdout and stderr one pipe (the only handle it inherits), outside any job of ours; waits up
/// to `limit`, then ends it. The call that failed and its code otherwise.
pub(crate) fn run_as(
    token: &Owned,
    desktop: &str,
    program: &Path,
    args: &[&str],
    cwd: &Path,
    limit: Duration,
) -> Result<Ran, String> {
    let (mut reader, writer) = std::io::pipe().map_err(|e| format!("CreatePipe: {e}"))?;
    let write = HANDLE(writer.as_raw_handle());
    // SAFETY: `write` is the live write end `writer` owns.
    unsafe { SetHandleInformation(write, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT) }
        .map_err(|e| gave("SetHandleInformation(HANDLE_FLAG_INHERIT)", &e))?;
    let handles = [write];
    let mut attributes = Attributes::inherit_only(&handles)?;
    let quoted = args.iter().map(|arg| format!(" \"{arg}\""));
    let line = format!("\"{}\"{}", program.display(), quoted.collect::<String>());
    let mut line: Vec<u16> = line.encode_utf16().chain(std::iter::once(0)).collect();
    let mut desk: Vec<u16> = desktop.encode_utf16().chain(std::iter::once(0)).collect();
    let info = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: u32::try_from(size_of::<STARTUPINFOEXW>()).unwrap_or(0),
            lpDesktop: PWSTR(desk.as_mut_ptr()),
            dwFlags: STARTF_USESTDHANDLES,
            hStdOutput: write,
            hStdError: write,
            ..STARTUPINFOW::default()
        },
        lpAttributeList: attributes.list(),
    };
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: the token is a live primary token with TOKEN_ASSIGN_PRIMARY; the command line and
    // desktop are NUL-terminated, writable and outlive the call; `info` is a STARTUPINFOEXW
    // (EXTENDED_STARTUPINFO_PRESENT) whose list and handles outlive the call; `process` is a live
    // out-pointer whose handles are owned below.
    let started = unsafe {
        CreateProcessAsUserW(
            Some(token.0),
            PCWSTR::null(),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            true,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
            None,
            &HSTRING::from(cwd.as_os_str()),
            (&raw const info).cast::<STARTUPINFOW>(),
            &raw mut process,
        )
    };
    drop(attributes);
    drop(writer);
    started.map_err(|e| gave("CreateProcessAsUserW", &e))?;
    drop(Owned(process.hThread));
    let child = Owned(process.hProcess);
    let pid = process.dwProcessId;
    note_started(pid);
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        drop(reader.read_to_string(&mut text));
        drop(send.send(text));
    });
    let millis = u32::try_from(limit.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the process handle is live with every right (we made it).
    let code = if unsafe { WaitForSingleObject(child.0, millis) } == WAIT_OBJECT_0 {
        let mut code = 0u32;
        // SAFETY: as above; `code` is a live out-pointer.
        unsafe { GetExitCodeProcess(child.0, &raw mut code) }
            .map_err(|e| gave("GetExitCodeProcess", &e))?;
        Some(code)
    } else {
        // SAFETY: as above.
        drop(unsafe { TerminateProcess(child.0, 1) });
        None
    };
    note_ended(pid);
    let text = receive
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    Ok(Ran {
        pid,
        code,
        lines: text.lines().map(|l| l.trim_end().to_owned()).collect(),
    })
}
