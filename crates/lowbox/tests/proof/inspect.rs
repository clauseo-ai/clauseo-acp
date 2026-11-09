//! The helper's look at chrome and its descendants, as the guard (`B_s`) would look: which it
//! can open (`PROCESS_QUERY_LIMITED_INFORMATION`, then the token with `TOKEN_QUERY`), each one's
//! role from its command line, its token's integrity, `AppContainer` package and restriction,
//! and its machine type. It snapshots every 500 ms for up to 20 s until a renderer, the GPU
//! process and the network service are all there and open. One `fact` line per process of the
//! last snapshot; one that ended before or during the look is `open=gone`. The package SID is
//! never printed, only whether a token's package is this one.
#![expect(
    unsafe_code,
    reason = "FFI to the Toolhelp snapshot, OpenProcess, the token queries and \
              NtQueryInformationProcess"
)]

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::os::windows::io::OwnedHandle;

use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows::Win32::Foundation::{HANDLE, STILL_ACTIVE, UNICODE_STRING};
use windows::Win32::Security::{
    GetLengthSid, GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation,
    IsTokenRestricted, TOKEN_APPCONTAINER_INFORMATION, TOKEN_INFORMATION_CLASS,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenAppContainerSid, TokenIntegrityLevel,
    TokenIsAppContainer,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE;
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessInformation, IsWow64Process2, OpenProcess, OpenProcessToken,
    PROCESS_MACHINE_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, ProcessMachineTypeInfo,
};

use super::helper::say;
use super::win::{Res, code, failed, owned, raw, sleep_ms};

/// How many snapshots the helper takes at most (20 s at [`SNAPSHOT_PAUSE_MS`]).
const SNAPSHOTS: u32 = 40;
/// The pause between them.
const SNAPSHOT_PAUSE_MS: u32 = 500;
/// The roles the row needs, each with a live process the helper opened.
const NEEDED: [&str; 3] = ["renderer", "gpu", "network"];
/// `ERROR_INVALID_PARAMETER`: `OpenProcess` on an id whose process has ended.
const GONE: u32 = 87;

/// One process of the snapshot.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    /// Its id.
    pub(crate) pid: u32,
    /// Its parent's id.
    pub(crate) parent: u32,
    /// Its image's file name.
    pub(crate) exe: String,
}

/// A process the helper looked at, with the handle it kept for the exit code.
pub(crate) struct Seen {
    /// Its id.
    pub(crate) pid: u32,
    /// Its role (`browser`, `renderer`, `gpu`, `network`, `other-<type>`).
    pub(crate) role: String,
    /// The `PROCESS_QUERY_LIMITED_INFORMATION` handle, when the open worked.
    pub(crate) handle: Option<OwnedHandle>,
}

/// Every process now (Toolhelp snapshot).
pub(crate) fn snapshot() -> Res<Vec<Entry>> {
    // SAFETY: a flag and no process id; the snapshot handle is owned below.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|e| failed("CreateToolhelp32Snapshot", &e))?;
    let snap = owned(snap);
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(u32::MAX),
        ..PROCESSENTRY32W::default()
    };
    let mut all = Vec::new();
    // SAFETY: an open snapshot and an entry whose dwSize is set.
    let mut more = unsafe { Process32FirstW(raw(&snap), &raw mut entry) }.is_ok();
    while more {
        let len = entry.szExeFile.iter().position(|&u| u == 0).unwrap_or(260);
        all.push(Entry {
            pid: entry.th32ProcessID,
            parent: entry.th32ParentProcessID,
            exe: String::from_utf16_lossy(&entry.szExeFile[..len]).replace(' ', "_"),
        });
        // SAFETY: as above.
        more = unsafe { Process32NextW(raw(&snap), &raw mut entry) }.is_ok();
    }
    Ok(all)
}

/// `root` and its descendants in `all`, parents before children.
pub(crate) fn tree(all: &[Entry], root: u32) -> Vec<Entry> {
    let mut out: Vec<Entry> = all.iter().filter(|e| e.pid == root).cloned().collect();
    let mut known: BTreeSet<u32> = out.iter().map(|e| e.pid).collect();
    let mut next = 0;
    while next < out.len() {
        let parent = out[next].pid;
        for child in all.iter().filter(|e| e.parent == parent && e.pid != parent) {
            if known.insert(child.pid) {
                out.push(child.clone());
            }
        }
        next += 1;
    }
    out
}

/// The role a command line shows.
fn role_of(is_root: bool, line: Option<&str>) -> String {
    let Some(line) = line else {
        return "unknown".to_owned();
    };
    if line.contains("--type=renderer") {
        "renderer".to_owned()
    } else if line.contains("--type=gpu-process") {
        "gpu".to_owned()
    } else if line.contains("--utility-sub-type=network.mojom.NetworkService") {
        "network".to_owned()
    } else if let Some(at) = line.find("--type=") {
        let kind: String = line[at + 7..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        format!("other-{kind}")
    } else if is_root {
        "browser".to_owned()
    } else {
        "child".to_owned()
    }
}

/// One process of chrome's tree, opened and classified.
struct Probe {
    /// Its snapshot entry.
    entry: Entry,
    /// The `PROCESS_QUERY_LIMITED_INFORMATION` handle, or the open's error code.
    opened: Result<OwnedHandle, u32>,
    /// Its command line (`None` when the open failed), or the NTSTATUS.
    line: Option<Result<String, i32>>,
    /// Its role.
    role: String,
}

impl Probe {
    /// Whether it was opened and has not exited.
    fn live(&self) -> bool {
        self.opened.as_ref().is_ok_and(|h| exit_code(h).is_none())
    }
}

/// `root` and its descendants now, each opened and given its role.
fn probe(root: u32) -> Res<Vec<Probe>> {
    let all = snapshot()?;
    Ok(tree(&all, root)
        .into_iter()
        .map(|entry| {
            // SAFETY: a process id; the handle, when there is one, is owned below.
            let opened =
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, entry.pid) }
                    .map(owned)
                    .map_err(|e| code(&e));
            let line = opened.as_ref().ok().map(command_line);
            let role = role_of(
                entry.pid == root,
                line.as_ref()
                    .and_then(|l| l.as_ref().ok())
                    .map(String::as_str),
            );
            Probe {
                entry,
                opened,
                line,
                role,
            }
        })
        .collect())
}

/// Looks at `root` (chrome) and its descendants, snapshotting until every [`NEEDED`] role has a
/// live process (at most [`SNAPSHOTS`] times), then prints one fact line for each process of
/// the last snapshot and keeps the handles. `package` is the binary package SID.
pub(crate) fn inspect(root: u32, package: &[u8]) -> Res<Vec<Seen>> {
    let mut taken = 0;
    let probes = loop {
        let probes = probe(root)?;
        taken += 1;
        let complete = NEEDED
            .iter()
            .all(|role| probes.iter().any(|p| p.role == *role && p.live()));
        if complete || taken >= SNAPSHOTS {
            say(format_args!(
                "snapshots taken={taken} complete={}",
                u8::from(complete)
            ));
            break probes;
        }
        sleep_ms(SNAPSHOT_PAUSE_MS);
    };
    Ok(probes.into_iter().map(|p| report(p, package)).collect())
}

/// Prints the fact line of `probe` and keeps its handle.
fn report(probe: Probe, package: &[u8]) -> Seen {
    let Probe {
        entry,
        opened,
        line,
        role,
    } = probe;
    let cmdline = match &line {
        None => "-".to_owned(),
        Some(Ok(_)) => "ok".to_owned(),
        Some(Err(status)) => format!("nt{:#010x}", status.cast_unsigned()),
    };
    let (token, machine) = match &opened {
        Ok(process) => (token_facts(process, package), machine(raw(process))),
        Err(_) => (
            "token=- integrity=- appcontainer=- this_package=- restricted=-".to_owned(),
            "machine=- wow=- native=-".to_owned(),
        ),
    };
    // Read last, so a process that ended during the look shows as gone.
    let open = match &opened {
        Ok(process) if exit_code(process).is_some() => "gone".to_owned(),
        Ok(_) => "ok".to_owned(),
        Err(code) if *code == GONE => "gone".to_owned(),
        Err(code) => format!("err{code}"),
    };
    say(format_args!(
        "fact role={role} pid={} parent={} exe={} open={open} cmdline={cmdline} {token} {machine}",
        entry.pid, entry.parent, entry.exe
    ));
    Seen {
        pid: entry.pid,
        role,
        handle: opened.ok(),
    }
}

/// The token's facts as `token=… integrity=… appcontainer=… this_package=… restricted=…`.
fn token_facts(process: &OwnedHandle, package: &[u8]) -> String {
    let mut token = HANDLE::default();
    // SAFETY: an open process handle; `token` gets a new handle, owned below.
    let opened = unsafe { OpenProcessToken(raw(process), TOKEN_QUERY, &raw mut token) };
    if let Err(e) = opened {
        return format!(
            "token=err{} integrity=- appcontainer=- this_package=- restricted=-",
            code(&e)
        );
    }
    let token = owned(token);
    let integrity = integrity(&token).map_or_else(|e| e, |rid| format!("{rid:#06x}"));
    let appcontainer = query(&token, TokenIsAppContainer).map_or_else(
        |e| e,
        |buffer| {
            // SAFETY: TokenIsAppContainer fills a u32 at the buffer's start.
            let flag = unsafe { buffer.as_ptr().cast::<u32>().read() };
            u8::from(flag != 0).to_string()
        },
    );
    let this_package = is_package(&token, package).map_or_else(|e| e, |b| u8::from(b).to_string());
    // SAFETY: an open token handle with TOKEN_QUERY.
    let restricted = u8::from(unsafe { IsTokenRestricted(raw(&token)) }.is_ok());
    format!(
        "token=ok integrity={integrity} appcontainer={appcontainer} this_package={this_package} \
         restricted={restricted}"
    )
}

/// `GetTokenInformation(class)` into an 8-byte aligned buffer; the error as `err<code>`.
fn query(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<u64>, String> {
    let mut needed = 0_u32;
    // SAFETY: the size query: no buffer, `needed` gets the bytes. It fails by design.
    let _sized: windows::core::Result<()> =
        unsafe { GetTokenInformation(raw(token), class, None, 0, &raw mut needed) };
    let mut buffer = vec![0_u64; usize::try_from(needed).unwrap_or(0).div_ceil(8).max(1)];
    let len = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
    // SAFETY: `buffer` is writable and aligned for `len` bytes and outlives the call.
    unsafe {
        GetTokenInformation(
            raw(token),
            class,
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            len,
            &raw mut needed,
        )
    }
    .map_err(|e| format!("err{}", code(&e)))?;
    Ok(buffer)
}

/// The token's integrity RID (0x0000 Untrusted, 0x1000 Low, 0x2000 Medium).
fn integrity(token: &OwnedHandle) -> Result<u32, String> {
    let buffer = query(token, TokenIntegrityLevel)?;
    // SAFETY: the call filled a TOKEN_MANDATORY_LABEL whose SID points into `buffer`, alive
    // here; the SID's sub-authority count is read before the last sub-authority.
    unsafe {
        let sid = buffer
            .as_ptr()
            .cast::<TOKEN_MANDATORY_LABEL>()
            .read()
            .Label
            .Sid;
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return Err("nosub".to_owned());
        }
        Ok(*GetSidSubAuthority(sid, u32::from(count) - 1))
    }
}

/// Whether the token's `AppContainer` SID is `package`.
fn is_package(token: &OwnedHandle, package: &[u8]) -> Result<bool, String> {
    let buffer = query(token, TokenAppContainerSid)?;
    // SAFETY: the call filled a TOKEN_APPCONTAINER_INFORMATION whose SID (null when the token
    // has none) points into `buffer`, alive here.
    unsafe {
        let sid = buffer
            .as_ptr()
            .cast::<TOKEN_APPCONTAINER_INFORMATION>()
            .read()
            .TokenAppContainer;
        if sid.0.is_null() {
            return Ok(false);
        }
        let len = usize::try_from(GetLengthSid(sid)).unwrap_or(0);
        Ok(std::slice::from_raw_parts(sid.0.cast::<u8>().cast_const(), len) == package)
    }
}

/// The process's command line (`NtQueryInformationProcess(ProcessCommandLineInformation)`), or
/// the NTSTATUS.
fn command_line(process: &OwnedHandle) -> Result<String, i32> {
    let mut needed = 0_u32;
    // SAFETY: the size query: no buffer, `needed` gets the bytes.
    let status = unsafe {
        NtQueryInformationProcess(
            raw(process),
            ProcessCommandLineInformation,
            std::ptr::null_mut(),
            0,
            &raw mut needed,
        )
    };
    if needed == 0 {
        return Err(status.0);
    }
    let mut buffer = vec![0_u64; usize::try_from(needed).unwrap_or(0).div_ceil(8)];
    let len = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
    // SAFETY: `buffer` is writable and aligned for `len` bytes and outlives the call.
    let status = unsafe {
        NtQueryInformationProcess(
            raw(process),
            ProcessCommandLineInformation,
            buffer.as_mut_ptr().cast::<c_void>(),
            len,
            &raw mut needed,
        )
    };
    if status.0 < 0 {
        return Err(status.0);
    }
    // SAFETY: the call filled a UNICODE_STRING whose buffer (Length bytes) is inside `buffer`.
    unsafe {
        let text = buffer.as_ptr().cast::<UNICODE_STRING>().read();
        if text.Buffer.is_null() || text.Length == 0 {
            return Ok(String::new());
        }
        let units = std::slice::from_raw_parts(text.Buffer.0, usize::from(text.Length / 2));
        Ok(String::from_utf16_lossy(units))
    }
}

/// The process's machine (`ProcessMachineTypeInfo`) and `IsWow64Process2`'s two answers, as
/// `machine=… wow=… native=…` (0x8664 x64, 0xaa64 ARM64, 0x0000 none).
pub(crate) fn machine(process: HANDLE) -> String {
    let mut info = PROCESS_MACHINE_INFORMATION::default();
    // SAFETY: an open process handle with query access and a local of the class's size.
    let process_machine = unsafe {
        GetProcessInformation(
            process,
            ProcessMachineTypeInfo,
            (&raw mut info).cast::<c_void>(),
            u32::try_from(size_of::<PROCESS_MACHINE_INFORMATION>()).unwrap_or(u32::MAX),
        )
    }
    .map_or_else(
        |e| format!("err{}", code(&e)),
        |()| format!("{:#06x}", info.ProcessMachine.0),
    );
    let (mut wow, mut native) = (IMAGE_FILE_MACHINE::default(), IMAGE_FILE_MACHINE::default());
    // SAFETY: an open process handle and two locals.
    let wow64 = unsafe { IsWow64Process2(process, &raw mut wow, Some(&raw mut native)) }
        .map_or_else(
            |e| format!("wow=err{} native=-", code(&e)),
            |()| format!("wow={:#06x} native={:#06x}", wow.0, native.0),
        );
    format!("machine={process_machine} {wow64}")
}

/// The process's exit code, or `None` while it runs or when it cannot be read.
pub(crate) fn exit_code(process: &OwnedHandle) -> Option<u32> {
    let mut code = 0_u32;
    // SAFETY: an open process handle (PROCESS_QUERY_LIMITED_INFORMATION) and a local.
    let read = unsafe { GetExitCodeProcess(raw(process), &raw mut code) };
    (read.is_ok() && code != STILL_ACTIVE.0.cast_unsigned()).then_some(code)
}
