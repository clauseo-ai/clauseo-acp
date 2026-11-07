//! Windows: a child created in the guard's job, so at no moment does it run, or wait, outside
//! it.
//!
//! `CreateProcessW` takes the job in a `PROC_THREAD_ATTRIBUTE_JOB_LIST` attribute (Windows 10 and
//! later), so Windows puts the new process in the job as it creates it, before its first thread
//! runs, and a failed join fails the creation. Std's `Command` cannot pass the attribute (its
//! `raw_attribute` is unstable), so this module creates the process itself, with what `Command`
//! gave the guard's children before:
//! - stdin, stdout and stderr each `NUL`, a pipe or the guard's own handle ([`Io`]); only these
//!   three are inherited, named in a `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`;
//! - `CREATE_NO_WINDOW` (agent-browser issue #1644), and exactly the given environment;
//! - the working folder, and std's quoting of the program and arguments (`command_line.rs`).
//!
//! The child starts running, not suspended: it is in the job from its creation, so nothing has
//! to wait for it to join. The guard writes Chrome's `running` record after this returns, as
//! before; a guard killed in between leaves `starting`, and the job ends that Chrome with the
//! guard (`record.rs`).
//!
//! The child's three handles are inheritable only from just before the call to just after it.
//! Any process started meanwhile with every inheritable handle could get them; on Windows the
//! guard starts no process but through here.
#![expect(
    unsafe_code,
    reason = "FFI to CreateProcessW and its attribute list, to create a child in the guard's job"
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, PipeReader, PipeWriter};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;

use windows::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, GENERIC_READ, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    UpdateProcThreadAttribute,
};
use windows::core::{PCWSTR, PWSTR, w};

use super::super::command_line::{command_line, current_dir, env_block, is_batch};
use super::child::Child;

/// What one of a child's stdin, stdout and stderr is.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Io {
    /// The `NUL` device.
    Null,
    /// A new pipe, whose other end the guard keeps ([`Child`]'s `stdin`, `stdout`, `stderr`).
    Piped,
    /// The guard's own.
    Inherit,
}

/// What a child is started with.
#[derive(Debug)]
pub(crate) struct Launch<'a> {
    /// An absolute path, used as it is: no `PATH` search and no `.exe` added. A relative path
    /// and a batch file are refused ([`spawn`]).
    pub(crate) program: &'a Path,
    pub(crate) args: &'a [OsString],
    pub(crate) cwd: &'a Path,
    /// The child's whole environment.
    pub(crate) env: &'a BTreeMap<String, OsString>,
    pub(crate) stdin: Io,
    pub(crate) stdout: Io,
    pub(crate) stderr: Io,
}

/// Starts `launch` in `job`, running. Every handle made here closes on every way out.
///
/// Refuses a relative program, which `CreateProcessW` would resolve against the guard's own
/// folder, and a batch file (`is_batch`), which it would run through `cmd.exe`, whose quoting
/// is not the one `command_line.rs` applies (CVE-2024-24576); std quotes for `cmd.exe` there or
/// refuses. Both callers pass absolute executables the guard checked; this keeps it so.
pub(super) fn spawn(job: HANDLE, launch: &Launch<'_>) -> io::Result<Child> {
    let program: Vec<u16> = launch.program.as_os_str().encode_wide().collect();
    if !launch.program.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the program's path is not absolute",
        ));
    }
    if is_batch(&program) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the program is a batch file, which Windows runs through cmd.exe",
        ));
    }
    let args: Vec<Vec<u16>> = launch
        .args
        .iter()
        .map(|arg| arg.encode_wide().collect())
        .collect();
    // Checks the program for a NUL, so the copy below needs no check of its own.
    let mut line = command_line(&program, &args)?;
    let application: Vec<u16> = program.into_iter().chain([0]).collect();
    let cwd = current_dir(&launch.cwd.as_os_str().encode_wide().collect::<Vec<_>>())?;
    let vars = launch
        .env
        .iter()
        .map(|(name, value)| (name.clone(), value.encode_wide().collect()))
        .collect();
    let env = env_block(vars)?;
    let stdin = Stdio::new(launch.stdin, true, io::stdin().as_raw_handle())?;
    let stdout = Stdio::new(launch.stdout, false, io::stdout().as_raw_handle())?;
    let stderr = Stdio::new(launch.stderr, false, io::stderr().as_raw_handle())?;
    let inherited: Vec<HANDLE> = [&stdin, &stdout, &stderr]
        .iter()
        .filter_map(|stdio| stdio.theirs.as_ref())
        .map(|handle| HANDLE(handle.as_raw_handle()))
        .collect();
    let inherit = !inherited.is_empty();
    let mut attributes = Attributes::new(job, inherited)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb =
        u32::try_from(size_of::<STARTUPINFOEXW>()).map_err(io::Error::other)?;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = stdin.handle();
    startup.StartupInfo.hStdOutput = stdout.handle();
    startup.StartupInfo.hStdError = stderr.handle();
    startup.lpAttributeList = attributes.list();
    let flags = CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT;
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: `application`, `cwd` and `env` are NUL-terminated UTF-16 buffers (`env` a whole
    // block of `name=value` entries, as CREATE_UNICODE_ENVIRONMENT says), and `line` a
    // NUL-terminated buffer the call may write into, all live until it returns. `startup` is a
    // STARTUPINFOEXW whose `cb` is its size (EXTENDED_STARTUPINFO_PRESENT), its standard
    // handles open or null, and its attribute list initialised and live in `attributes`, with
    // the job and handle values it points to. `info` is a live PROCESS_INFORMATION it writes.
    unsafe {
        CreateProcessW(
            PCWSTR(application.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            inherit,
            flags,
            Some(env.as_ptr().cast()),
            PCWSTR(cwd.as_ptr()),
            &raw const startup.StartupInfo,
            &raw mut info,
        )
    }
    .map_err(io::Error::from)?;
    // SAFETY: both are open handles CreateProcessW just returned, which nothing else owns; each
    // `OwnedHandle` closes its own once.
    let (process, thread) = unsafe {
        (
            OwnedHandle::from_raw_handle(info.hProcess.0),
            OwnedHandle::from_raw_handle(info.hThread.0),
        )
    };
    drop(thread);
    drop(attributes);
    Ok(Child::new(
        process,
        info.dwProcessId,
        stdin.ours.map(PipeWriter::from),
        stdout.ours.map(PipeReader::from),
        stderr.ours.map(PipeReader::from),
    ))
}

/// One of a child's standard handles: the child's end, inheritable and closed once the child
/// has its copy (none when the guard has no such handle to pass on), and the guard's end of a
/// pipe.
struct Stdio {
    theirs: Option<OwnedHandle>,
    ours: Option<OwnedHandle>,
}

impl Stdio {
    /// `io` for the child's stdin (`input`) or an output; `own` is the guard's same handle.
    fn new(io: Io, input: bool, own: RawHandle) -> io::Result<Stdio> {
        match io {
            Io::Null => Ok(Stdio {
                theirs: Some(null_device()?),
                ours: None,
            }),
            Io::Piped => {
                let (reader, writer) = io::pipe()?;
                let (theirs, ours) = if input {
                    (OwnedHandle::from(reader), OwnedHandle::from(writer))
                } else {
                    (OwnedHandle::from(writer), OwnedHandle::from(reader))
                };
                Ok(Stdio {
                    theirs: Some(inheritable(HANDLE(theirs.as_raw_handle()))?),
                    ours: Some(ours),
                })
            }
            Io::Inherit => {
                let own = HANDLE(own);
                // As std: a guard with no such handle passes none on.
                let theirs = if own.is_invalid() || own == INVALID_HANDLE_VALUE {
                    None
                } else {
                    Some(inheritable(own)?)
                };
                Ok(Stdio { theirs, ours: None })
            }
        }
    }

    /// The child's handle for STARTUPINFO, null if none.
    fn handle(&self) -> HANDLE {
        self.theirs
            .as_ref()
            .map_or(HANDLE::default(), |handle| HANDLE(handle.as_raw_handle()))
    }
}

/// An inheritable copy of `handle`, with its access.
fn inheritable(handle: HANDLE) -> io::Result<OwnedHandle> {
    let mut copy = HANDLE::default();
    // SAFETY: the current process's pseudo handle needs no closing; `handle` is open for the
    // call (the caller holds it), and `copy` is a live HANDLE the call writes.
    unsafe {
        let me = GetCurrentProcess();
        DuplicateHandle(
            me,
            handle,
            me,
            &raw mut copy,
            0,
            true,
            DUPLICATE_SAME_ACCESS,
        )
    }
    .map_err(io::Error::from)?;
    // SAFETY: `copy` is the open handle DuplicateHandle just made, which nothing else owns; the
    // `OwnedHandle` closes it once.
    Ok(unsafe { OwnedHandle::from_raw_handle(copy.0) })
}

/// An inheritable handle to `NUL`, for reading and writing.
fn null_device() -> io::Result<OwnedHandle> {
    let security = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    // SAFETY: the name is a static NUL-terminated UTF-16 literal and `security` a live
    // SECURITY_ATTRIBUTES whose `nLength` is its size; no template file.
    let raw = unsafe {
        CreateFileW(
            w!("NUL"),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            Some(&raw const security),
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map_err(io::Error::from)?;
    // SAFETY: `raw` is the open handle CreateFileW just returned, which nothing else owns; the
    // `OwnedHandle` closes it once.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw.0) })
}

/// A `PROC_THREAD_ATTRIBUTE_LIST` holding the job list and the inherited handle list. The list
/// keeps pointers to its values, not copies, so it owns them, and its drop deletes the list
/// before they go.
struct Attributes {
    /// The list's storage, in pointer-sized words so it is aligned as the list needs.
    storage: Box<[usize]>,
    /// Whether `InitializeProcThreadAttributeList` succeeded, so the drop must delete it.
    initialised: bool,
    jobs: Box<[HANDLE]>,
    handles: Box<[HANDLE]>,
}

impl Attributes {
    /// The list for `job` and, unless none, `handles`.
    fn new(job: HANDLE, handles: Vec<HANDLE>) -> io::Result<Attributes> {
        let count = if handles.is_empty() { 1 } else { 2 };
        let mut size = 0_usize;
        // SAFETY: with no list the call only writes the size a list of `count` needs into the
        // live `size`; it reports ERROR_INSUFFICIENT_BUFFER, which is expected.
        drop(unsafe { InitializeProcThreadAttributeList(None, count, None, &raw mut size) });
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = Attributes {
            storage: vec![0; size.div_ceil(size_of::<usize>())].into_boxed_slice(),
            initialised: false,
            jobs: Box::new([job]),
            handles: handles.into_boxed_slice(),
        };
        // SAFETY: `storage` is at least `size` writable bytes, aligned for pointers, which
        // live (and stay put) as long as `attributes`; `size` is the live size it says.
        unsafe {
            InitializeProcThreadAttributeList(Some(attributes.list()), count, None, &raw mut size)
        }
        .map_err(io::Error::from)?;
        attributes.initialised = true;
        let job_list = usize::try_from(PROC_THREAD_ATTRIBUTE_JOB_LIST).map_err(io::Error::other)?;
        // SAFETY: the list is initialised for `count` attributes; `jobs` is an array of one job
        // handle, open while the caller's `Job` lives, of the size given, owned by
        // `attributes`, so it outlives the list, which its drop deletes first.
        unsafe {
            UpdateProcThreadAttribute(
                attributes.list(),
                0,
                job_list,
                Some(attributes.jobs.as_ptr().cast()),
                size_of_val(&*attributes.jobs),
                None,
                None,
            )
        }
        .map_err(io::Error::from)?;
        if attributes.handles.is_empty() {
            return Ok(attributes);
        }
        let handle_list =
            usize::try_from(PROC_THREAD_ATTRIBUTE_HANDLE_LIST).map_err(io::Error::other)?;
        // SAFETY: as for the job list; `handles` holds distinct open inheritable handles.
        unsafe {
            UpdateProcThreadAttribute(
                attributes.list(),
                0,
                handle_list,
                Some(attributes.handles.as_ptr().cast()),
                size_of_val(&*attributes.handles),
                None,
                None,
            )
        }
        .map_err(io::Error::from)?;
        Ok(attributes)
    }

    fn list(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        LPPROC_THREAD_ATTRIBUTE_LIST(self.storage.as_mut_ptr().cast())
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialised {
            // SAFETY: the list was initialised, and is deleted here only, once, before the
            // storage and the values it points to drop.
            unsafe { DeleteProcThreadAttributeList(self.list()) };
        }
    }
}
