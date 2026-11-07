//! The runners' process calls (Windows): the outer's `CreateProcessWithLogonW` of the inner
//! runner and its kill-on-close job, and the inner's private desktop, error mode and
//! `CreateProcessAsUserW` of the agent with only its three pipes inherited.
#![expect(
    unsafe_code,
    reason = "FFI to the logon, job, desktop and process calls"
)]

use std::ffi::c_void;
use std::io;
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::System::Diagnostics::Debug::{
    SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX, SetErrorMode,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, CreateDesktopW, DESKTOP_CONTROL_FLAGS, HDESK,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW,
    CreateProcessWithLogonW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
    GetExitCodeProcess, InitializeProcThreadAttributeList, LOGON_WITH_PROFILE,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION,
    ResumeThread, STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
    TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use zeroize::Zeroizing;

use crate::windows::acl::object::{Handle, os_error};
use crate::windows::token::os::LocalDescriptor;

/// `GENERIC_ALL`, the access the runner asks for on its own desktop.
const GENERIC_ALL: u32 = 0x1000_0000;
/// `SW_HIDE`.
const SW_HIDE: u16 = 0;

/// A started process: its handle and its first thread's, closed on drop.
pub(super) struct Process {
    pub(super) handle: Handle,
    pub(super) thread: Handle,
    pub(super) pid: u32,
}

impl Process {
    fn from(info: PROCESS_INFORMATION) -> Process {
        Process {
            handle: Handle(info.hProcess),
            thread: Handle(info.hThread),
            pid: info.dwProcessId,
        }
    }

    /// Waits up to `timeout` (forever when `None`) and gives its exit code once it exited.
    pub(super) fn wait(&self, timeout: Option<Duration>) -> io::Result<Option<u32>> {
        let millis = timeout.map_or(u32::MAX, |t| {
            u32::try_from(t.as_millis()).unwrap_or(u32::MAX)
        });
        // SAFETY: the process handle is live.
        let woke = unsafe { WaitForSingleObject(self.handle.0, millis) };
        if woke == WAIT_TIMEOUT {
            return Ok(None);
        }
        if woke != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut code = 0u32;
        // SAFETY: the process handle is live; `code` is a live out-pointer.
        unsafe { GetExitCodeProcess(self.handle.0, &raw mut code) }.map_err(|e| os_error(&e))?;
        Ok(Some(code))
    }

    /// Ends it with `code`; one already gone is fine.
    pub(super) fn kill(&self, code: u32) {
        // SAFETY: the process handle is live; a failure (already exited) leaves nothing to do.
        drop(unsafe { TerminateProcess(self.handle.0, code) });
    }
}

/// The outer's call: starts `exe --inner <base>` as the pool user `name` (on this computer)
/// with its profile loaded, suspended, hidden, in `cwd`, with the pool user's own environment.
/// The password buffer is wiped when this returns. A failure gives the Win32 code.
pub(super) fn log_on(
    name: &str,
    password: Zeroizing<String>,
    exe: &str,
    base: &str,
    cwd: &str,
) -> Result<Process, u32> {
    let user = HSTRING::from(name);
    let mut secret: Zeroizing<Vec<u16>> = Zeroizing::new(Vec::with_capacity(password.len() + 1));
    secret.extend(password.encode_utf16());
    secret.push(0);
    drop(password);
    let mut line: Vec<u16> = format!("\"{exe}\" --inner {base}")
        .encode_utf16()
        .chain([0])
        .collect();
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or(0),
        dwFlags: STARTF_USESHOWWINDOW,
        wShowWindow: SW_HIDE,
        ..STARTUPINFOW::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: every string is NUL-terminated and outlives the call, the command line writable as
    // the call requires; no environment, so the pool user's own is made from its profile;
    // `info` is a live out-pointer whose handles `Process` owns.
    let started = unsafe {
        CreateProcessWithLogonW(
            &user,
            &HSTRING::from("."),
            PCWSTR(secret.as_ptr()),
            LOGON_WITH_PROFILE,
            &HSTRING::from(exe),
            Some(PWSTR(line.as_mut_ptr())),
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            None,
            &HSTRING::from(cwd),
            &raw const startup,
            &raw mut info,
        )
    };
    drop(secret);
    match started {
        Ok(()) => Ok(Process::from(info)),
        Err(error) => Err(u32::try_from(error.code().0 & 0xFFFF).unwrap_or(u32::MAX)),
    }
}

/// A job whose last handle closing ends every process in it, with no breakaway allowed: the
/// inner runner, the agent and all it starts.
pub(super) fn kill_on_close_job() -> io::Result<Handle> {
    // SAFETY: no attributes (an unnamed job no other process can open) and no name.
    let job = Handle(unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|e| os_error(&e))?);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: the job is live; `limits` is the structure the class names, for its size.
    unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast::<c_void>(),
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap_or(0),
        )
    }
    .map_err(|e| os_error(&e))?;
    Ok(job)
}

/// Puts the suspended `process` in `job`, then lets it run.
pub(super) fn assign_and_resume(job: &Handle, process: &Process) -> io::Result<()> {
    // SAFETY: both handles are live; the process handle has every right (it came from the
    // logon call).
    unsafe { AssignProcessToJobObject(job.0, process.handle.0) }.map_err(|e| os_error(&e))?;
    // SAFETY: the thread handle is live and the thread suspended once.
    if unsafe { ResumeThread(process.thread.0) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// No hard-error box from the runner or anything it starts: a failing start would otherwise
/// wait on a box no one sees (`s15-spikes.md` (a); the mode is inherited).
pub(super) fn quiet_errors() {
    // SAFETY: changes only this process's error mode.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX)
    };
}

/// The inner runner's private desktop on its window station, closed on drop.
pub(super) struct Desktop {
    handle: HDESK,
    /// `WinSta0\<name>`, for the agent's `lpDesktop`.
    pub(super) path: String,
}

impl Desktop {
    /// Makes the desktop `name` with the descriptor `sddl` (`token::desktop_sddl`).
    pub(super) fn create(name: &str, sddl: &str) -> io::Result<Desktop> {
        let descriptor = LocalDescriptor::new(sddl)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: false.into(),
        };
        // SAFETY: the name is a live HSTRING and `attributes` with its descriptor outlives the
        // call; the desktop is closed by `Drop`.
        let handle = unsafe {
            CreateDesktopW(
                &HSTRING::from(name),
                PCWSTR::null(),
                None,
                DESKTOP_CONTROL_FLAGS::default(),
                GENERIC_ALL,
                Some(&raw const attributes),
            )
        }
        .map_err(|e| os_error(&e))?;
        Ok(Desktop {
            handle,
            path: format!(r"WinSta0\{name}"),
        })
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        // SAFETY: the desktop came from CreateDesktopW and is closed once, here.
        drop(unsafe { CloseDesktop(self.handle) });
    }
}

/// What the agent is started with.
pub(super) struct Agent<'a> {
    pub(super) token: &'a Handle,
    pub(super) desktop: &'a str,
    pub(super) program: &'a str,
    pub(super) command_line: &'a str,
    pub(super) cwd: &'a str,
    pub(super) env: &'a [(String, String)],
    /// Its stdin, stdout and stderr, inheritable; the only handles it inherits.
    pub(super) stdio: [HANDLE; 3],
}

/// The inner's call: starts the agent under its token, on the private desktop, with only its
/// three pipe handles inherited (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`), no window, and its own
/// environment block.
pub(super) fn start_agent(agent: &Agent<'_>) -> io::Result<Process> {
    let mut list = AttributeList::new(1)?;
    let handles = agent.stdio;
    // SAFETY: the list was initialised for one attribute; `handles` outlives the list's use in
    // CreateProcessAsUserW below.
    unsafe {
        UpdateProcThreadAttribute(
            list.as_list(),
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            Some(handles.as_ptr().cast::<c_void>()),
            size_of_val(&handles),
            None,
            None,
        )
    }
    .map_err(|e| os_error(&e))?;
    let mut desktop: Vec<u16> = agent.desktop.encode_utf16().chain([0]).collect();
    let mut line: Vec<u16> = agent.command_line.encode_utf16().chain([0]).collect();
    let block = env_block(agent.env);
    let startup = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: u32::try_from(size_of::<STARTUPINFOEXW>()).map_err(io::Error::other)?,
            lpDesktop: PWSTR(desktop.as_mut_ptr()),
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: agent.stdio[0],
            hStdOutput: agent.stdio[1],
            hStdError: agent.stdio[2],
            ..STARTUPINFOW::default()
        },
        lpAttributeList: list.as_list(),
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: the token is a restricted copy of this process's own primary token; every string
    // and the environment block are NUL-terminated and outlive the call; the startup structure
    // is an extended one, as the flag says, and its list and handles are live; `info` is a live
    // out-pointer whose handles `Process` owns.
    unsafe {
        CreateProcessAsUserW(
            Some(agent.token.0),
            &HSTRING::from(agent.program),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            true,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            Some(block.as_ptr().cast::<c_void>()),
            &HSTRING::from(agent.cwd),
            (&raw const startup).cast::<STARTUPINFOW>(),
            &raw mut info,
        )
    }
    .map_err(|e| os_error(&e))?;
    Ok(Process::from(info))
}

/// `env` as a Unicode environment block: `NAME=VALUE\0` each, then `\0`.
pub(super) fn env_block(env: &[(String, String)]) -> Vec<u16> {
    let mut block: Vec<u16> = Vec::new();
    for (name, value) in env {
        block.extend(name.encode_utf16());
        block.push(u16::from(b'='));
        block.extend(value.encode_utf16());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// A process-thread attribute list, deleted on drop.
struct AttributeList {
    buffer: Vec<u64>,
}

impl AttributeList {
    fn new(count: u32) -> io::Result<AttributeList> {
        let mut size = 0usize;
        // SAFETY: no list asks only for the size; that call always fails with
        // ERROR_INSUFFICIENT_BUFFER, which is the expected answer.
        drop(unsafe { InitializeProcThreadAttributeList(None, count, None, &raw mut size) });
        let mut list = AttributeList {
            buffer: vec![0u64; size.div_ceil(size_of::<u64>()).max(1)],
        };
        // SAFETY: the buffer is at least `size` bytes and aligned for the list.
        unsafe {
            InitializeProcThreadAttributeList(Some(list.as_list()), count, None, &raw mut size)
        }
        .map_err(|e| os_error(&e))?;
        Ok(list)
    }

    fn as_list(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        LPPROC_THREAD_ATTRIBUTE_LIST(self.buffer.as_mut_ptr().cast())
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: the list was initialised in `new` and is deleted once, here.
        unsafe { DeleteProcThreadAttributeList(self.as_list()) };
    }
}
