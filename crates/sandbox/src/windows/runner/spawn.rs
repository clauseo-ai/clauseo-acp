//! The runners' process calls (Windows): the outer's `CreateProcessWithLogonW` of the inner
//! runner and of the end sweeper, the kill-on-close job (whose one handle is the outer's), and the
//! inner's private desktop, error mode and `CreateProcessAsUserW` of the agent with only its three
//! pipes inherited.
#![expect(
    unsafe_code,
    reason = "FFI to the logon, job, desktop and process calls"
)]

use std::ffi::c_void;
use std::io;
use std::time::Duration;

use windows::Win32::Foundation::{
    HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAGS, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::{SECURITY_ATTRIBUTES, TOKEN_QUERY};
use windows::Win32::System::Diagnostics::Debug::{
    SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX, SetErrorMode,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_UILIMIT, JOB_OBJECT_UILIMIT_DESKTOP, JOB_OBJECT_UILIMIT_DISPLAYSETTINGS,
    JOB_OBJECT_UILIMIT_EXITWINDOWS, JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES,
    JOB_OBJECT_UILIMIT_READCLIPBOARD, JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS,
    JOB_OBJECT_UILIMIT_WRITECLIPBOARD, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectBasicUIRestrictions,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject,
};
use windows::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories, SetDllDirectoryW,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW,
    CreateProcessWithLogonW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
    GetExitCodeProcess, InitializeProcThreadAttributeList, LOGON_WITH_PROFILE,
    LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcessToken, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROCESS_INFORMATION, ResumeThread, STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    STARTUPINFOW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use zeroize::Zeroizing;

use crate::windows::acl::object::{Handle, os_error};

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

    /// The user of its token.
    pub(super) fn user(&self) -> io::Result<crate::windows::sids::Sid> {
        let mut token = HANDLE::default();
        // SAFETY: the process handle has every right (it came from the logon call); `token` is
        // a live out-pointer, owned by `Handle` once written.
        unsafe { OpenProcessToken(self.handle.0, TOKEN_QUERY, &raw mut token) }
            .map_err(|e| os_error(&e))?;
        crate::windows::token::os::user(&Handle(token))
    }

    /// Ends it with `code`; one already gone is fine.
    pub(super) fn kill(&self, code: u32) {
        // SAFETY: the process handle is live; a failure (already exited) leaves nothing to do.
        drop(unsafe { TerminateProcess(self.handle.0, code) });
    }
}

// SAFETY: the handles are kernel handles any thread may use; the end sweeper's is kept behind a
// mutex for the outer's last wait.
unsafe impl Send for Process {}

/// The launch's kill-on-close job, held where every end path reaches it (the outer's end
/// sweeper's steps), on the stdin relay's thread too.
pub(super) struct Job(pub(super) Handle);

// SAFETY: a job handle is a kernel handle any thread may use; it is closed once, when its owner
// drops at this process's exit.
unsafe impl Send for Job {}
// SAFETY: as above; the calls made through a shared one (`TerminateJobObject`,
// `QueryInformationJobObject`, `AssignProcessToJobObject`) may run on two threads at once.
unsafe impl Sync for Job {}

/// The outer's call: starts `exe <args>` (`--inner <base>`, or `--sweep`) as the pool user
/// `name` (on this computer) with its profile loaded, suspended, hidden, in `cwd`, with the pool
/// user's own environment; with `output`, its stdout and stderr are that handle, which the
/// Secondary Logon service duplicates from this process into the new one. The password buffer is
/// wiped when this returns. A failure gives the Win32 code.
pub(super) fn log_on(
    name: &str,
    password: Zeroizing<String>,
    exe: &str,
    args: &str,
    cwd: &str,
    output: Option<HANDLE>,
) -> Result<Process, u32> {
    let user = HSTRING::from(name);
    let mut secret: Zeroizing<Vec<u16>> = Zeroizing::new(Vec::with_capacity(password.len() + 1));
    secret.extend(password.encode_utf16());
    secret.push(0);
    drop(password);
    let mut line: Vec<u16> = format!("\"{exe}\" {args}")
        .encode_utf16()
        .chain([0])
        .collect();
    let mut startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or(0),
        dwFlags: STARTF_USESHOWWINDOW,
        wShowWindow: SW_HIDE,
        ..STARTUPINFOW::default()
    };
    if let Some(output) = output {
        startup.dwFlags |= STARTF_USESTDHANDLES;
        startup.hStdOutput = output;
        startup.hStdError = output;
    }
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

/// The job's limits. `KILL_ON_JOB_CLOSE`: the outer's one handle closing ends every process in
/// it. `DIE_ON_UNHANDLED_EXCEPTION`: a process in it that crashes (the agent, msys bash, a
/// headless Chrome) just ends, with no Windows Error Reporting; otherwise `WerFault` runs as the
/// desktop's user, the lawyer, and opens the crashed sandbox process from the lawyer's side
/// (decision 0049). The pool user's own WER is also off (`wer.rs`).
pub(super) const JOB_LIMITS: JOB_OBJECT_LIMIT = JOB_OBJECT_LIMIT(
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.0 | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION.0,
);

/// The job's UI limits, as Chromium's sandbox sets them. `WinSta0`, not the desktop, owns the
/// clipboard and the global atom table, so the private desktop alone doesn't keep the agent from
/// the lawyer's clipboard or atoms. `DESKTOP`: no process in the job makes or switches desktops,
/// so the agent can't make its own desktop the one the lawyer sees; the outer runner makes the
/// agent's desktop before the inner runner joins the job.
pub(super) const UI_LIMITS: JOB_OBJECT_UILIMIT = JOB_OBJECT_UILIMIT(
    JOB_OBJECT_UILIMIT_READCLIPBOARD.0
        | JOB_OBJECT_UILIMIT_WRITECLIPBOARD.0
        | JOB_OBJECT_UILIMIT_GLOBALATOMS.0
        | JOB_OBJECT_UILIMIT_HANDLES.0
        | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS.0
        | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS.0
        | JOB_OBJECT_UILIMIT_EXITWINDOWS.0
        | JOB_OBJECT_UILIMIT_DESKTOP.0,
);

/// A job with [`JOB_LIMITS`] (its last handle closing ends every process in it; a crash ends
/// the process without WER), no breakaway allowed and [`UI_LIMITS`]: the inner runner, the
/// agent and all it starts.
pub(super) fn kill_on_close_job() -> io::Result<Handle> {
    // SAFETY: no attributes (an unnamed job no other process can open) and no name.
    let job = Handle(unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|e| os_error(&e))?);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_LIMITS;
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
    let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: UI_LIMITS,
    };
    // SAFETY: the job is live; `ui` is the structure the class names, for its size.
    unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectBasicUIRestrictions,
            (&raw const ui).cast::<c_void>(),
            u32::try_from(size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>()).unwrap_or(0),
        )
    }
    .map_err(|e| os_error(&e))?;
    Ok(job)
}

/// Ends every process in `job` now, with `code`; their pipe ends close with them.
pub(super) fn end_job(job: &Handle, code: u32) {
    // SAFETY: the job is live; a failure (nothing left in it) leaves nothing to do.
    drop(unsafe { TerminateJobObject(job.0, code) });
}

/// How many processes are still in `job` (`JobObjectBasicAccountingInformation`'s
/// `ActiveProcesses`): a process ended by [`end_job`] counts until it has exited.
pub(super) fn active_processes(job: &Handle) -> io::Result<u32> {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    // SAFETY: the job is live with every right (this process made it); `info` is the structure
    // the class names, writable for its size; the returned length isn't needed.
    unsafe {
        QueryInformationJobObject(
            Some(job.0),
            JobObjectBasicAccountingInformation,
            (&raw mut info).cast::<c_void>(),
            u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>()).unwrap_or(0),
            None,
        )
    }
    .map_err(|e| os_error(&e))?;
    Ok(info.ActiveProcesses)
}

/// Keeps this process's later DLL loads to System32 and takes the current folder out of the
/// search: the inner runner (Medium, the pool user) must never load a DLL its Low agent could
/// plant. Called first in both runners, which end at once ([`super::RunExit::DllSearch`]) when
/// either call fails.
pub(super) fn restrict_dll_search() -> io::Result<()> {
    // SAFETY: a flag-only call.
    unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) }.map_err(|e| os_error(&e))?;
    // SAFETY: the empty string is a live HSTRING for the call.
    unsafe { SetDllDirectoryW(&HSTRING::new()) }.map_err(|e| os_error(&e))
}

/// System32, the inner runner's current folder: one the agent can't write.
pub(super) fn system_folder() -> io::Result<String> {
    let mut buffer = [0u16; 260];
    // SAFETY: `buffer` is writable for its length.
    let len = unsafe { GetSystemDirectoryW(Some(&mut buffer)) };
    let len = usize::try_from(len).map_err(io::Error::other)?;
    if len == 0 || len >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    String::from_utf16(buffer.get(..len).unwrap_or_default()).map_err(io::Error::other)
}

/// Puts `process` (the suspended inner runner's handle) in `job`.
pub(super) fn assign(job: &Handle, process: HANDLE) -> io::Result<()> {
    // SAFETY: the job is live with every right (this process made it); the caller passes a live
    // process handle with `PROCESS_SET_QUOTA` and `PROCESS_TERMINATE` (the logon call's has
    // every right).
    unsafe { AssignProcessToJobObject(job.0, process) }.map_err(|e| os_error(&e))
}

/// An anonymous pipe for the end sweeper's output: the read end, not inheritable, as a `File`,
/// and the write end, inheritable as the Secondary Logon service needs its standard handles.
pub(super) fn output_pipe() -> io::Result<(std::fs::File, Handle)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
    // SAFETY: both out-pointers are live, each owned by `Handle` once written; `attributes`
    // outlives the call.
    unsafe {
        CreatePipe(
            &raw mut read,
            &raw mut write,
            Some(&raw const attributes),
            0,
        )
    }
    .map_err(|e| os_error(&e))?;
    let (read, write) = (Handle(read), Handle(write));
    // SAFETY: the read end is live; this clears its inherit flag alone.
    unsafe { SetHandleInformation(read.0, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) }
        .map_err(|e| os_error(&e))?;
    Ok((crate::windows::pipe::os::into_file(read), write))
}

/// Lets the suspended `process` run.
pub(super) fn resume(process: &Process) -> io::Result<()> {
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
    // Declared before the list, so it outlives the list's use and its deletion.
    let handles = agent.stdio;
    let mut list = AttributeList::new(1)?;
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
        let mut buffer = vec![0u64; size.div_ceil(size_of::<u64>()).max(1)];
        let raw = LPPROC_THREAD_ATTRIBUTE_LIST(buffer.as_mut_ptr().cast());
        // SAFETY: the buffer is at least `size` bytes and aligned for the list. Only once this
        // succeeds is the buffer an `AttributeList`, whose drop deletes it.
        unsafe { InitializeProcThreadAttributeList(Some(raw), count, None, &raw mut size) }
            .map_err(|e| os_error(&e))?;
        Ok(AttributeList { buffer })
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
