//! The app's side of the set-up task (`setup::serve`): [`request_set_up`] asks
//! `\Clauseo\SandboxSetUp` for a set-up, or an extend, with no prompt, and gives the server's
//! reply. The desktop calls it for the first run's set-up and for a grow of the pool, off the UI
//! thread, and maps the reply (`SetUpReply::outcome`) to its `SetupState`;
//! [`SetUpError::NotRegistered`] means this install has no set-up task, and the elevated
//! `UserSetup` path, with its prompt, is the one left.
//!
//! In order:
//!
//! 1. starts the task (`IRegisteredTask::Run`, through the read-and-execute entry its descriptor
//!    gives this user);
//! 2. opens the pipe `\\.\pipe\ClauseoSandboxSetUp` with identification-only quality of service
//!    (`SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION`), so whoever serves it can learn who
//!    this is but never act as them, and the pipe's client rights alone (`pipe::PIPE_RIGHTS`,
//!    never a new instance). A pipe not there yet is tried again for [`CONNECT_WAIT`], the task
//!    started once more after [`RERUN_AFTER`] (a start made as the last server ended may have
//!    been its own); a busy one is waited for (`WaitNamedPipeW`);
//! 3. on that same handle, before writing anything, checks who serves it
//!    (`setup::serve::check_server`): its session (`GetNamedPipeServerSessionId`) is 0, its
//!    process (`GetNamedPipeServerProcessId`) is LOCAL SYSTEM's in the kernel's process list, and
//!    its image, as the kernel names it, is the admin copy. A pool user can make the pipe's name
//!    first; a server that fails the check gets nothing ([`SetUpError::NotTheServer`]);
//! 4. writes the request (`{"v":1,"op":"set_up"}`, or `{"v":1,"op":"add","n":n}`) and reads the one-line reply; a server
//!    that went away before replying (one that was ending as this connected) is asked again,
//!    once, after the task is started again.
#![expect(unsafe_code, reason = "FFI to the named pipe and process list calls")]

use std::path::PathBuf;

use windows::Win32::Foundation::{
    ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_NOT_CONNECTED, HANDLE,
    UNICODE_STRING,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE, OPEN_EXISTING, QueryDosDeviceW,
    ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows::Win32::System::Pipes::{
    GetNamedPipeServerProcessId, GetNamedPipeServerSessionId, WaitNamedPipeW,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::Sleep;
use windows::core::{HSTRING, PWSTR};

use super::acl::object::{Handle, os_error};
use super::admin_copy::{self, os};
use super::pipe::PIPE_RIGHTS;
use super::refresh::task::run_task;
use super::refresh::{SystemTask, TaskError};
use super::setup::serve::{
    CONNECT_WAIT, Op, PIPE_NAME, REPLY_CAP, RERUN_AFTER, ServerFacts, check_server,
};
use super::setup::stop::windows_stopper::listed_users;

pub use super::setup::serve::{Reply as SetUpReply, SetUpError};

/// The pause between two tries while the pipe isn't there, in milliseconds.
const PAUSE_MS: u32 = 250;
/// How long one wait for a busy pipe lasts, in milliseconds.
const BUSY_WAIT_MS: u32 = 1_000;
/// `SystemProcessIdInformation`: a process's image name by its id, with no handle to it.
const PROCESS_ID_INFORMATION: u32 = 88;

/// `NtQuerySystemInformation`, which the `windows` crate's features in use don't bind.
mod nt {
    windows::core::link!("ntdll.dll" "system" fn NtQuerySystemInformation(
    class: u32,
    information: *mut core::ffi::c_void,
    length: u32,
    returned: *mut u32
) -> windows::Win32::Foundation::NTSTATUS);
}

/// Asks the set-up task for a set-up (`add` none) or an extend by `add` users, as the module
/// says, as whoever calls it, and gives the server's reply. Blocks until the set-up ends: call it
/// off the UI thread.
pub fn request_set_up(add: Option<u8>) -> Result<SetUpReply, SetUpError> {
    let op = add.map_or(Op::SetUp, Op::Add);
    let request = op.to_bytes().unwrap_or_default();
    if Op::parse(&request) != Ok(op) {
        return Err(SetUpError::Request(format!("an extend by {add:?}")));
    }
    start()?;
    let mut began = now_ms();
    let mut started_again = false;
    let mut asked_again = false;
    loop {
        let elapsed = now_ms().saturating_sub(began);
        match open() {
            Ok(pipe) => match exchange(&pipe, &request) {
                Ok(reply) => return Ok(reply),
                Err(Exchange::Gone) if !asked_again => {
                    asked_again = true;
                    start()?;
                    began = now_ms();
                    started_again = false;
                }
                Err(Exchange::Gone) => {
                    return Err(SetUpError::Pipe("the server went away twice".to_owned()));
                }
                Err(Exchange::NotTheServer(why)) => return Err(SetUpError::NotTheServer(why)),
                Err(Exchange::Failed(why)) => return Err(SetUpError::Pipe(why)),
            },
            Err(code)
                if elapsed >= CONNECT_WAIT
                    && [ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY].contains(&code) =>
            {
                return Err(SetUpError::NoServer);
            }
            Err(code) if code == ERROR_FILE_NOT_FOUND => {
                if elapsed >= RERUN_AFTER && !started_again {
                    started_again = true;
                    start()?;
                }
                // SAFETY: plain value; it only waits.
                unsafe { Sleep(PAUSE_MS) };
            }
            Err(code) if code == ERROR_PIPE_BUSY => {
                // SAFETY: a live HSTRING; it only waits for an instance to be free.
                let _free_or_timed_out =
                    unsafe { WaitNamedPipeW(&HSTRING::from(PIPE_NAME), BUSY_WAIT_MS) };
            }
            Err(code) => {
                return Err(SetUpError::Pipe(format!(
                    "opening it failed (error {})",
                    code.0
                )));
            }
        }
    }
}

/// Starts the set-up task.
fn start() -> Result<(), SetUpError> {
    match run_task(SystemTask::SetUp) {
        Ok(()) => Ok(()),
        Err(TaskError::NotRegistered) => Err(SetUpError::NotRegistered),
        Err(error) => Err(SetUpError::Task(error.to_string())),
    }
}

/// The tick count, in milliseconds.
fn now_ms() -> u64 {
    // SAFETY: it only reads the tick count.
    unsafe { GetTickCount64() }
}

/// The pipe's client end, synchronous, identification only; the Win32 error when it won't open.
fn open() -> Result<Handle, windows::Win32::Foundation::WIN32_ERROR> {
    // SAFETY: the name is a live HSTRING; no security attributes; the handle returned is owned
    // by `Handle`.
    let opened = unsafe {
        CreateFileW(
            &HSTRING::from(PIPE_NAME),
            PIPE_RIGHTS,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(SECURITY_SQOS_PRESENT.0 | SECURITY_IDENTIFICATION.0),
            None,
        )
    };
    opened.map(Handle).map_err(|error| {
        windows::Win32::Foundation::WIN32_ERROR::from_error(&error).unwrap_or_default()
    })
}

/// How an exchange on an open pipe failed.
enum Exchange {
    /// The server closed before any reply.
    Gone,
    /// The server isn't the set-up task's.
    NotTheServer(String),
    /// Anything else.
    Failed(String),
}

/// Checks the server on `pipe`, then sends `request` and reads the reply.
fn exchange(pipe: &Handle, request: &[u8]) -> Result<SetUpReply, Exchange> {
    let facts = server_facts(pipe).map_err(Exchange::NotTheServer)?;
    let (exe, device) = admin_copy_device().map_err(Exchange::Failed)?;
    check_server(&facts, &exe, &device).map_err(Exchange::NotTheServer)?;
    let mut written = 0u32;
    // SAFETY: a live client end opened for writing; `request` and `written` outlive the call.
    unsafe { WriteFile(pipe.0, Some(request), Some(&raw mut written), None) }
        .map_err(|error| gone_or(&error))?;
    let mut reply = Vec::new();
    let mut chunk = [0u8; 4096];
    while !reply.contains(&b'\n') {
        if reply.len() > REPLY_CAP {
            return Err(Exchange::Failed(format!(
                "a reply of more than {REPLY_CAP} bytes"
            )));
        }
        let mut read = 0u32;
        // SAFETY: a live client end opened for reading; `chunk` and `read` outlive the call.
        let done = unsafe { ReadFile(pipe.0, Some(&mut chunk), Some(&raw mut read), None) };
        match done {
            Ok(()) if read == 0 => break,
            Ok(()) => reply.extend_from_slice(chunk.get(..read as usize).unwrap_or_default()),
            Err(error) if reply.is_empty() => return Err(gone_or(&error)),
            Err(error) => return Err(Exchange::Failed(os_error(&error).to_string())),
        }
    }
    if reply.is_empty() {
        return Err(Exchange::Gone);
    }
    SetUpReply::parse(&reply).map_err(Exchange::Failed)
}

/// `error` as [`Exchange::Gone`] when it says the server closed, else a failure.
fn gone_or(error: &windows::core::Error) -> Exchange {
    if [ERROR_BROKEN_PIPE, ERROR_PIPE_NOT_CONNECTED]
        .iter()
        .any(|code| error.code() == code.to_hresult())
    {
        Exchange::Gone
    } else {
        Exchange::Failed(os_error(error).to_string())
    }
}

/// Who serves `pipe`: its session, and its process's user and image, read with no handle to it.
fn server_facts(pipe: &Handle) -> Result<ServerFacts, String> {
    let (mut session, mut pid) = (0u32, 0u32);
    // SAFETY: a live client end; `session` and `pid` are live out-pointers.
    unsafe {
        GetNamedPipeServerSessionId(pipe.0, &raw mut session)
            .map_err(|error| format!("its session: {}", os_error(&error)))?;
        GetNamedPipeServerProcessId(pipe.0, &raw mut pid)
            .map_err(|error| format!("its process: {}", os_error(&error)))?;
    }
    let user = listed_users()?
        .remove(&pid)
        .and_then(|(user, _session)| user);
    Ok(ServerFacts {
        session,
        user,
        image: image_of(pid),
    })
}

/// `SYSTEM_PROCESS_ID_INFORMATION`: a process id in, its image name out.
#[repr(C)]
struct ProcessIdInformation {
    process_id: HANDLE,
    image_name: UNICODE_STRING,
}

/// The image of process `pid` as the kernel names it (`\Device\HarddiskVolumeN\...`), read by
/// its id alone, so a process this user may not open still has one; `None` when not given.
fn image_of(pid: u32) -> Option<String> {
    let mut name = vec![0u16; 1024];
    let mut info = ProcessIdInformation {
        process_id: HANDLE(usize::try_from(pid).ok()? as *mut core::ffi::c_void),
        image_name: UNICODE_STRING {
            Length: 0,
            MaximumLength: u16::try_from(name.len() * 2).ok()?,
            Buffer: PWSTR(name.as_mut_ptr()),
        },
    };
    let size = u32::try_from(size_of::<ProcessIdInformation>()).ok()?;
    // SAFETY: `info` is live and `size` bytes long, and its buffer is `name`, live and
    // `MaximumLength` bytes long, for the call.
    let status = unsafe {
        nt::NtQuerySystemInformation(
            PROCESS_ID_INFORMATION,
            (&raw mut info).cast(),
            size,
            std::ptr::null_mut(),
        )
    };
    if status.is_err() {
        return None;
    }
    let units = usize::from(info.image_name.Length) / 2;
    String::from_utf16(name.get(..units)?).ok()
}

/// The admin copy's path and the device its drive names (`QueryDosDeviceW`).
fn admin_copy_device() -> Result<(PathBuf, String), String> {
    let program_files = os::program_files().map_err(|error| error.to_string())?;
    let exe = admin_copy::admin_exe(&program_files);
    let text = exe.to_string_lossy();
    let drive = text.get(..2).ok_or("the admin copy's path has no drive")?;
    let mut device = vec![0u16; 1024];
    // SAFETY: a live HSTRING; `device` is writable for its length.
    let written = unsafe { QueryDosDeviceW(&HSTRING::from(drive), Some(&mut device)) };
    if written == 0 {
        return Err(format!(
            "the drive {drive}'s device: {}",
            std::io::Error::last_os_error()
        ));
    }
    let end = device
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(device.len());
    let device = String::from_utf16(device.get(..end).unwrap_or_default())
        .map_err(|error| error.to_string())?;
    Ok((exe, device))
}
