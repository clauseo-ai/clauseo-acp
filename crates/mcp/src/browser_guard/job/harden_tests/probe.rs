//! The probes `harden_tests.rs` makes of a hardened process, and what a failed check prints: the
//! target's security descriptor and the probing token, so one CI run tells a DACL that was not
//! applied from a token that bypasses it.
//!
//! - [`Access::of`]: which of the rights the checks name this process may open a pid for.
//! - [`remove_debug`]: `SeDebugPrivilege` taken out of this process's token for good.
//! - [`restricted`]: this test binary's [`super::restricted_probe_child`] run under a restricted
//!   token, as Codex's sandbox makes one: every privilege but `SeChangeNotify` deleted
//!   (`DISABLE_MAX_PRIVILEGE`) and BUILTIN\Administrators deny-only.
//! - [`sddl_of`] and [`token_report`]: the diagnosis.
#![expect(
    unsafe_code,
    reason = "FFI to the token, security descriptor and process APIs, to probe a hardened process"
)]

use std::ffi::OsString;
use std::io::{self, Read};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::{HANDLE, HLOCAL, LUID, LocalFree, WAIT_OBJECT_0};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW, GetSecurityInfo,
    SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, CreateRestrictedToken, CreateWellKnownSid, DACL_SECURITY_INFORMATION,
    DISABLE_MAX_PRIVILEGE, EqualSid, GROUP_SECURITY_INFORMATION, GetTokenInformation,
    LABEL_SECURITY_INFORMATION, LUID_AND_ATTRIBUTES, LookupPrivilegeNameW, LookupPrivilegeValueW,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DEBUG_NAME, SE_PRIVILEGE_ENABLED,
    SE_PRIVILEGE_REMOVED, SID_AND_ATTRIBUTES, TOKEN_ACCESS_MASK, TOKEN_ADJUST_PRIVILEGES,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS,
    TOKEN_MANDATORY_LABEL, TOKEN_OWNER, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER, TokenElevation,
    TokenGroups, TokenIntegrityLevel, TokenOwner, TokenPrivileges, TokenUser,
    WinBuiltinAdministratorsSid,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, GetCurrentProcess,
    OpenProcess, OpenProcessToken, PROCESS_ACCESS_RIGHTS, PROCESS_DUP_HANDLE, PROCESS_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_VM_READ, STARTF_USESTDHANDLES,
    STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

use super::super::super::command_line::{command_line, env_block};
use super::super::spawn::inheritable;

/// The variable that tells [`super::restricted_probe_child`] the pid to probe.
pub(super) const PROBE_PID: &str = "CLAUSEO_HARDEN_PROBE_PID";
/// The start of the line the restricted probe prints its result on.
const RESULT: &str = "HARDEN-PROBE";
/// `WRITE_DAC`, as a process access right.
const WRITE_DAC: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0004_0000);
/// `SE_GROUP_ENABLED` and `SE_GROUP_USE_FOR_DENY_ONLY`.
const GROUP_ENABLED: u32 = 0x4;
const GROUP_DENY_ONLY: u32 = 0x10;
/// How long the restricted probe may take before it is ended and the check fails.
const PROBE_WAIT_MS: u32 = 120_000;

/// The rights a probe asks for, each by the name its result line gives it: the three the
/// hardened DACL denies, then `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`, the probes the
/// host and the guard make, which it grants.
const RIGHTS: [(&str, PROCESS_ACCESS_RIGHTS); 4] = [
    ("vm_read", PROCESS_VM_READ),
    ("dup_handle", PROCESS_DUP_HANDLE),
    ("write_dac", WRITE_DAC),
    (
        "query",
        PROCESS_ACCESS_RIGHTS(PROCESS_QUERY_LIMITED_INFORMATION.0 | PROCESS_SYNCHRONIZE.0),
    ),
];

/// Which of [`RIGHTS`] a probe was granted on the target, in their order.
#[derive(Debug)]
pub(super) struct Access([bool; 4]);

impl Access {
    /// What this process, with its token as it is now, may open `pid` for.
    pub(super) fn of(pid: u32) -> Access {
        Access(RIGHTS.map(|(_, right)| opens(pid, right)))
    }

    pub(super) fn vm_read(&self) -> bool {
        self.0[0]
    }

    pub(super) fn dup_handle(&self) -> bool {
        self.0[1]
    }

    pub(super) fn write_dac(&self) -> bool {
        self.0[2]
    }

    pub(super) fn query(&self) -> bool {
        self.0[3]
    }

    /// The result line the restricted probe prints.
    pub(super) fn line(&self) -> String {
        let fields: Vec<String> = RIGHTS
            .iter()
            .zip(self.0)
            .map(|((name, _), granted)| {
                let word = if granted { "granted" } else { "denied" };
                format!("{name}={word}")
            })
            .collect();
        format!("{RESULT} {}", fields.join(" "))
    }

    /// The result in the restricted probe's `output`, if it printed one.
    pub(super) fn parse(output: &str) -> Option<Access> {
        let line = output
            .lines()
            .find(|line| line.trim_start().starts_with(RESULT))?;
        let mut granted = [false; 4];
        for ((name, _), slot) in RIGHTS.iter().zip(&mut granted) {
            let value = line
                .split_whitespace()
                .find_map(|field| field.strip_prefix(name)?.strip_prefix('='))?;
            *slot = value == "granted";
        }
        Some(Access(granted))
    }
}

/// Whether this process may open `pid` for `access`.
fn opens(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> bool {
    // SAFETY: plain values; a handle returned is closed at once by its drop below.
    match unsafe { OpenProcess(access, false, pid) } {
        Ok(handle) => {
            // SAFETY: `handle` is the open handle just returned, owned and closed once here.
            drop(unsafe { OwnedHandle::from_raw_handle(handle.0) });
            true
        }
        Err(_) => false,
    }
}

/// This process's token, opened for `access`.
fn own_token(access: TOKEN_ACCESS_MASK) -> io::Result<OwnedHandle> {
    let mut raw = HANDLE::default();
    // SAFETY: the pseudo handle needs no closing; `raw` is a live HANDLE the call writes.
    unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut raw) }
        .map_err(io::Error::from)?;
    // SAFETY: `raw` is the open token handle just returned, owned and closed once here.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw.0) })
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// `class` of `token`, in a buffer aligned for any of the TOKEN_* structs.
fn info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<u64>> {
    let mut size = 0_u32;
    // SAFETY: with no buffer the call only writes the size it needs into `size`; its
    // ERROR_INSUFFICIENT_BUFFER is expected.
    drop(unsafe { GetTokenInformation(token, class, None, 0, &raw mut size) });
    let words = usize::try_from(size).map_err(io::Error::other)?.div_ceil(8);
    let mut buffer = vec![0_u64; words.max(1)];
    let length = u32::try_from(buffer.len() * 8).map_err(io::Error::other)?;
    // SAFETY: `buffer` is `length` writable bytes, 8-aligned, live for the call.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            length,
            &raw mut size,
        )
    }
    .map_err(io::Error::from)?;
    Ok(buffer)
}

/// `sid` as a string (`S-1-5-...`), or what went wrong.
fn sid_string(sid: PSID) -> String {
    let mut text = PWSTR::null();
    // SAFETY: `sid` is a valid SID the caller keeps live; `text` is a live pointer the call sets
    // to a LocalAlloc'd string, freed below.
    if let Err(error) = unsafe { ConvertSidToStringSidW(sid, &raw mut text) } {
        return format!("(unreadable SID: {error})");
    }
    // SAFETY: `text` is the NUL-terminated string the call returned.
    let string = unsafe { text.to_string() }.unwrap_or_else(|error| format!("({error})"));
    // SAFETY: `text` was allocated by LocalAlloc and is freed here, once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    string
}

/// BUILTIN\Administrators' SID, in a buffer of `SECURITY_MAX_SID_SIZE` bytes or more.
fn administrators() -> io::Result<Box<[u64; 9]>> {
    let mut buffer = Box::new([0_u64; 9]);
    let mut size = u32::try_from(size_of::<[u64; 9]>()).map_err(io::Error::other)?;
    // SAFETY: `buffer` is `size` writable bytes, live for the call.
    unsafe {
        CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            None,
            Some(PSID(buffer.as_mut_ptr().cast())),
            &raw mut size,
        )
    }
    .map_err(io::Error::from)?;
    Ok(buffer)
}

/// Whether `token` is elevated (`TokenElevation`).
fn is_elevated(token: HANDLE) -> io::Result<bool> {
    let buffer = info(token, TokenElevation)?;
    // SAFETY: the call filled `buffer` with a TOKEN_ELEVATION.
    Ok(unsafe { (*buffer.as_ptr().cast::<TOKEN_ELEVATION>()).TokenIsElevated } != 0)
}

/// Whether this process's token is elevated; no on any error.
pub(super) fn elevated() -> bool {
    own_token(TOKEN_QUERY).is_ok_and(|token| is_elevated(raw(&token)).unwrap_or(false))
}

/// `token`'s privileges: each one's name and whether it is enabled.
fn privileges(token: HANDLE) -> io::Result<Vec<(String, bool)>> {
    let buffer = info(token, TokenPrivileges)?;
    let list = buffer.as_ptr().cast::<TOKEN_PRIVILEGES>();
    // SAFETY: the call filled `buffer` with a TOKEN_PRIVILEGES of `PrivilegeCount` entries.
    let count = unsafe { (*list).PrivilegeCount };
    let mut out = Vec::new();
    for index in 0..usize::try_from(count).map_err(io::Error::other)? {
        // SAFETY: `index` is below the count; the entries follow in `buffer`.
        let entry = unsafe {
            *(&raw const (*list).Privileges)
                .cast::<LUID_AND_ATTRIBUTES>()
                .add(index)
        };
        let mut name = [0_u16; 128];
        let mut length = u32::try_from(name.len()).map_err(io::Error::other)?;
        // SAFETY: `name` is `length` writable UTF-16 units; the LUID is a live local.
        unsafe {
            LookupPrivilegeNameW(
                PCWSTR::null(),
                &raw const entry.Luid,
                Some(PWSTR(name.as_mut_ptr())),
                &raw mut length,
            )
        }
        .map_err(io::Error::from)?;
        let name = name
            .get(..usize::try_from(length).map_err(io::Error::other)?)
            .unwrap_or(&[]);
        let enabled = entry.Attributes.0 & SE_PRIVILEGE_ENABLED.0 != 0;
        out.push((String::from_utf16_lossy(name), enabled));
    }
    Ok(out)
}

/// What `token` holds of BUILTIN\Administrators: `enabled`, `deny-only`, `disabled` or `absent`.
fn administrators_state(token: HANDLE) -> io::Result<&'static str> {
    let mut admins = administrators()?;
    let buffer = info(token, TokenGroups)?;
    let groups = buffer.as_ptr().cast::<TOKEN_GROUPS>();
    // SAFETY: the call filled `buffer` with a TOKEN_GROUPS of `GroupCount` entries.
    let count = unsafe { (*groups).GroupCount };
    for index in 0..usize::try_from(count).map_err(io::Error::other)? {
        // SAFETY: `index` is below the count; the entries follow in `buffer`, their SIDs
        // pointing inside it.
        let group = unsafe {
            *(&raw const (*groups).Groups)
                .cast::<SID_AND_ATTRIBUTES>()
                .add(index)
        };
        // SAFETY: both SIDs are valid and live for the call.
        if unsafe { EqualSid(group.Sid, PSID(admins.as_mut_ptr().cast())) }.is_ok() {
            return Ok(if group.Attributes & GROUP_DENY_ONLY != 0 {
                "deny-only"
            } else if group.Attributes & GROUP_ENABLED != 0 {
                "enabled"
            } else {
                "disabled"
            });
        }
    }
    Ok("absent")
}

/// The integrity level of `token`, as its label's SID and name.
fn integrity(token: HANDLE) -> io::Result<String> {
    let buffer = info(token, TokenIntegrityLevel)?;
    // SAFETY: the call filled `buffer` with a TOKEN_MANDATORY_LABEL whose SID is inside it.
    let sid = sid_string(unsafe { (*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid });
    let name = match sid.as_str() {
        "S-1-16-4096" => "Low",
        "S-1-16-8192" => "Medium",
        "S-1-16-12288" => "High",
        "S-1-16-16384" => "System",
        _ => "other",
    };
    Ok(format!("{name} ({sid})"))
}

fn describe(token: HANDLE) -> io::Result<String> {
    let user = info(token, TokenUser)?;
    // SAFETY: the call filled `user` with a TOKEN_USER whose SID is inside it.
    let user = sid_string(unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid });
    let owner = info(token, TokenOwner)?;
    // SAFETY: the call filled `owner` with a TOKEN_OWNER whose SID is inside it.
    let owner = sid_string(unsafe { (*owner.as_ptr().cast::<TOKEN_OWNER>()).Owner });
    let privileges = privileges(token)?;
    let names = |enabled: bool| -> Vec<&str> {
        privileges
            .iter()
            .filter(|entry| entry.1 == enabled)
            .map(|entry| entry.0.as_str())
            .collect()
    };
    Ok(format!(
        "user {user}, default owner {owner}, integrity {}, elevated {}, administrators {}, \
         privileges enabled {:?}, held but disabled {:?}",
        integrity(token)?,
        is_elevated(token)?,
        administrators_state(token)?,
        names(true),
        names(false)
    ))
}

/// This process's token, as a failed check prints it.
pub(super) fn token_report() -> String {
    own_token(TOKEN_QUERY)
        .and_then(|token| describe(raw(&token)))
        .unwrap_or_else(|error| format!("(token unreadable: {error})"))
}

/// The restricted probe's own token, for its parent to check: `administrators=<state>
/// sedebug=<enabled|off>`.
pub(super) fn token_state() -> String {
    let state = own_token(TOKEN_QUERY).and_then(|token| {
        let token = raw(&token);
        Ok((administrators_state(token)?, debug_enabled_in(token)?))
    });
    match state {
        Ok((admins, debug)) => {
            let debug = if debug { "enabled" } else { "off" };
            format!("administrators={admins} sedebug={debug}")
        }
        Err(error) => format!("(token unreadable: {error})"),
    }
}

fn debug_enabled_in(token: HANDLE) -> io::Result<bool> {
    Ok(privileges(token)?
        .iter()
        .any(|(name, enabled)| *enabled && name == "SeDebugPrivilege"))
}

/// Whether `SeDebugPrivilege` is enabled in this process's token.
pub(super) fn debug_enabled() -> io::Result<bool> {
    debug_enabled_in(raw(&own_token(TOKEN_QUERY)?))
}

/// Takes `SeDebugPrivilege` out of this process's token for good (`SE_PRIVILEGE_REMOVED`): a token
/// that does not hold it is left as it is.
pub(super) fn remove_debug() -> io::Result<()> {
    let token = own_token(TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY)?;
    let mut luid = LUID::default();
    // SAFETY: the name is a static NUL-terminated literal; `luid` is a live local it writes.
    unsafe { LookupPrivilegeValueW(PCWSTR::null(), SE_DEBUG_NAME, &raw mut luid) }
        .map_err(io::Error::from)?;
    let state = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_REMOVED,
        }],
    };
    // SAFETY: `token` is open with TOKEN_ADJUST_PRIVILEGES; `state` is a live TOKEN_PRIVILEGES
    // of one entry; no previous state asked for.
    unsafe { AdjustTokenPrivileges(raw(&token), false, Some(&raw const state), 0, None, None) }
        .map_err(io::Error::from)
}

/// The security descriptor of `process` (a handle with `READ_CONTROL`) as SDDL: owner, group,
/// DACL and integrity label; or what went wrong.
pub(super) fn sddl_of(process: HANDLE) -> String {
    let parts = OWNER_SECURITY_INFORMATION
        | GROUP_SECURITY_INFORMATION
        | DACL_SECURITY_INFORMATION
        | LABEL_SECURITY_INFORMATION;
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `process` is open for the call; `descriptor` is a live local the call sets to a
    // LocalAlloc'd descriptor, freed below.
    let read = unsafe {
        GetSecurityInfo(
            process,
            SE_KERNEL_OBJECT,
            parts,
            None,
            None,
            None,
            None,
            Some(&raw mut descriptor),
        )
    };
    if let Err(error) = read.ok() {
        return format!("(security descriptor unreadable: {error})");
    }
    let mut text = PWSTR::null();
    // SAFETY: `descriptor` is the valid descriptor just read; `text` is a live local the call
    // sets to a LocalAlloc'd string, freed below.
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            parts,
            &raw mut text,
            None,
        )
    };
    let string = match converted {
        // SAFETY: `text` is the NUL-terminated string the call returned.
        Ok(()) => unsafe { text.to_string() }.unwrap_or_else(|error| format!("({error})")),
        Err(error) => format!("(security descriptor not convertible: {error})"),
    };
    // SAFETY: both were LocalAlloc'd by the calls above (or are null) and are freed here, once.
    unsafe {
        LocalFree(Some(HLOCAL(text.0.cast())));
        LocalFree(Some(HLOCAL(descriptor.0)));
    }
    string
}

/// Runs the test `entry` of this test binary under a restricted token made from this process's
/// own (privileges but `SeChangeNotify` deleted, BUILTIN\Administrators deny-only), with
/// [`PROBE_PID`] set to `pid`, and returns all it printed, stdout and stderr.
pub(super) fn restricted(entry: &str, pid: u32) -> io::Result<String> {
    let token = own_token(TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_QUERY)?;
    let mut admins = administrators()?;
    let disable = [SID_AND_ATTRIBUTES {
        Sid: PSID(admins.as_mut_ptr().cast()),
        Attributes: 0,
    }];
    let mut made = HANDLE::default();
    // SAFETY: `token` is open with TOKEN_DUPLICATE; `disable` holds one valid SID, live for the
    // call; `made` is a live local the call writes.
    unsafe {
        CreateRestrictedToken(
            raw(&token),
            DISABLE_MAX_PRIVILEGE,
            Some(&disable),
            None,
            None,
            &raw mut made,
        )
    }
    .map_err(io::Error::from)?;
    // SAFETY: `made` is the open token just returned, owned and closed once here.
    let restricted = unsafe { OwnedHandle::from_raw_handle(made.0) };

    let (mut reader, writer) = io::pipe()?;
    let output = inheritable(HANDLE(writer.as_raw_handle()))?;
    drop(writer);
    let program: Vec<u16> = std::env::current_exe()?.as_os_str().encode_wide().collect();
    let args: Vec<Vec<u16>> = [entry, "--exact", "--nocapture", "--test-threads=1"]
        .iter()
        .map(|arg| arg.encode_utf16().collect())
        .collect();
    let mut line = command_line(&program, &args)?;
    let application: Vec<u16> = program.into_iter().chain([0]).collect();
    let vars = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value)))
        .chain([(PROBE_PID.to_owned(), OsString::from(pid.to_string()))])
        .map(|(name, value)| (name, value.encode_wide().collect()))
        .collect();
    let env = env_block(vars)?;
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).map_err(io::Error::other)?,
        dwFlags: STARTF_USESTDHANDLES,
        hStdOutput: raw(&output),
        hStdError: raw(&output),
        ..STARTUPINFOW::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: `restricted` is a primary token made from this process's own, which needs no
    // SeAssignPrimaryTokenPrivilege; `application` and `env` are NUL-terminated UTF-16 buffers
    // (`env` a whole block, as CREATE_UNICODE_ENVIRONMENT says) and `line` a NUL-terminated
    // buffer the call may write into, all live until it returns; `startup`'s `cb` is its size
    // and its output handles are the open inheritable `output`; `info` is a live local it writes.
    unsafe {
        CreateProcessAsUserW(
            Some(raw(&restricted)),
            PCWSTR(application.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            true,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            Some(env.as_ptr().cast()),
            PCWSTR::null(),
            &raw const startup,
            &raw mut info,
        )
    }
    .map_err(io::Error::from)?;
    // The probe's only copy of the pipe's write end is its own now, so the read ends when it
    // does, or when it is ended below.
    drop(output);
    let read = std::thread::spawn(move || {
        let mut printed = Vec::new();
        reader
            .read_to_end(&mut printed)
            .map(|_| String::from_utf8_lossy(&printed).into_owned())
    });
    // SAFETY: both are open handles the call just returned, which nothing else owns.
    let (process, thread) = unsafe {
        (
            OwnedHandle::from_raw_handle(info.hProcess.0),
            OwnedHandle::from_raw_handle(info.hThread.0),
        )
    };
    drop(thread);
    // SAFETY: `process` is open with all access (its creator's handle).
    let waited = unsafe { WaitForSingleObject(raw(&process), PROBE_WAIT_MS) };
    let mut note = String::new();
    if waited != WAIT_OBJECT_0 {
        // SAFETY: as above.
        drop(unsafe { TerminateProcess(raw(&process), 1) });
        note = format!("\n(the probe did not end within {PROBE_WAIT_MS} ms and was ended)");
    }
    let printed = read
        .join()
        .map_err(|_panic| io::Error::other("the probe's reader panicked"))??;
    Ok(printed + &note)
}
