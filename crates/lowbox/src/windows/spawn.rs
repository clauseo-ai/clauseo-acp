//! The guard's side: the `LowBox` token over the guard's own token, and the suspended child.
//!
//! As the W2 (j) recipe did (`/tmp/stagec/w2-j-recipe.md`, step 2): `NtCreateLowBoxToken` over
//! this process's primary token with the package SID, no capabilities and the namespace's six
//! handles, then `CreateProcessAsUserW` with that token on this process's station and desktop
//! (`lpDesktop` null), the token's own environment, and only the listed handles inherited.
#![expect(
    unsafe_code,
    reason = "FFI to the token, environment, attribute list and process calls"
)]

use std::ffi::{OsStr, OsString, c_void};
use std::fs::File;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};

use ::windows::Win32::Foundation::{HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation};
use ::windows::Win32::Security::{PSID, TOKEN_ACCESS_MASK};
use ::windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use ::windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcessToken,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, STARTUPINFOW, UpdateProcThreadAttribute,
};
use ::windows::core::{PCWSTR, PWSTR};

use super::namespace::NamespaceHandles;
use super::nt::NtCreateLowBoxToken;
use super::stdio::{Stdio, Which};
use super::{last_error, nt_ok, wide_nul, win32};
use crate::envblock::{self, Change};
use crate::error::Error;
use crate::quote::command_line;

/// `TOKEN_ALL_ACCESS`, as the recipe opened its token and asked for the `LowBox` one.
const TOKEN_ALL_ACCESS: u32 = 0x000F_01FF;

/// A program to start in a `LowBox` token, suspended. Built like `std::process::Command`.
#[derive(Debug, Clone)]
pub struct LowBoxCommand {
    /// The program's path.
    program: OsString,
    /// Its arguments, quoted by std's rules.
    args: Vec<OsString>,
    /// Changes to the token's environment, in order.
    env: Vec<Change>,
    /// Start from an empty environment instead of the token's.
    env_clear: bool,
    /// The working folder; the guard's when `None`.
    cwd: Option<PathBuf>,
    /// Standard input.
    stdin: Stdio,
    /// Standard output.
    stdout: Stdio,
    /// Standard error.
    stderr: Stdio,
    /// Further handles the child inherits, at the same values.
    inherit: Vec<RawHandle>,
}

/// A started, still suspended child.
#[derive(Debug)]
pub struct Spawned {
    /// Its process id.
    pub pid: u32,
    /// Its process handle (all access): for the Job Object, the wait and the exit code.
    pub process: OwnedHandle,
    /// Its first thread, suspended until [`Spawned::resume`].
    pub thread: OwnedHandle,
    /// The guard's end of the child's input, with [`Stdio::Piped`].
    pub stdin: Option<File>,
    /// The guard's end of the child's output, with [`Stdio::Piped`].
    pub stdout: Option<File>,
    /// The guard's end of the child's error output, with [`Stdio::Piped`].
    pub stderr: Option<File>,
}

impl LowBoxCommand {
    /// `program`, with no arguments, the token's environment, the guard's folder and standard
    /// handles.
    pub fn new(program: impl AsRef<OsStr>) -> LowBoxCommand {
        LowBoxCommand {
            program: program.as_ref().to_owned(),
            args: Vec::new(),
            env: Vec::new(),
            env_clear: false,
            cwd: None,
            stdin: Stdio::Inherit,
            stdout: Stdio::Inherit,
            stderr: Stdio::Inherit,
            inherit: Vec::new(),
        }
    }

    /// Adds an argument.
    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut LowBoxCommand {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    /// Adds arguments.
    pub fn args<I, S>(&mut self, args: I) -> &mut LowBoxCommand
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_owned()));
        self
    }

    /// Sets a variable over the token's environment.
    pub fn env(&mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut LowBoxCommand {
        self.env
            .push(Change::Set(units(name.as_ref()), units(value.as_ref())));
        self
    }

    /// Removes a variable from the token's environment.
    pub fn env_remove(&mut self, name: impl AsRef<OsStr>) -> &mut LowBoxCommand {
        self.env.push(Change::Remove(units(name.as_ref())));
        self
    }

    /// Starts from an empty environment: only the variables set after this.
    pub fn env_clear(&mut self) -> &mut LowBoxCommand {
        self.env_clear = true;
        self.env.clear();
        self
    }

    /// Sets the working folder.
    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut LowBoxCommand {
        self.cwd = Some(dir.as_ref().to_owned());
        self
    }

    /// Sets standard input.
    pub fn stdin(&mut self, stdio: Stdio) -> &mut LowBoxCommand {
        self.stdin = stdio;
        self
    }

    /// Sets standard output.
    pub fn stdout(&mut self, stdio: Stdio) -> &mut LowBoxCommand {
        self.stdout = stdio;
        self
    }

    /// Sets standard error.
    pub fn stderr(&mut self, stdio: Stdio) -> &mut LowBoxCommand {
        self.stderr = stdio;
        self
    }

    /// Passes `handle` to the child at the same value. The spawn turns its `HANDLE_FLAG_INHERIT`
    /// on and leaves it on; only children that list it (as this one does) get it.
    pub fn inherit_handle(&mut self, handle: RawHandle) -> &mut LowBoxCommand {
        self.inherit.push(handle);
        self
    }

    /// Makes the `LowBox` token of `namespace`'s package over this process's token and starts the
    /// program in it, suspended, on this process's station and desktop. The caller puts the
    /// child in its Job Object, then calls [`Spawned::resume`].
    pub fn spawn_suspended(&self, namespace: &NamespaceHandles) -> Result<Spawned, Error> {
        let mut line = command_line(&self.program, &self.args)?;
        line.push(0);
        let program = wide_nul(&self.program);
        let cwd = self.cwd.as_deref().map(|d| wide_nul(d.as_os_str()));
        let token = lowbox_token(namespace)?;
        let environment = self.environment(&token)?;

        let stdin = self.stdin.ends(Which::Input)?;
        let stdout = self.stdout.ends(Which::Output)?;
        let stderr = self.stderr.ends(Which::Error)?;
        let mut listed: Vec<HANDLE> = [&stdin.child, &stdout.child, &stderr.child]
            .iter()
            .map(|h| HANDLE(h.as_raw_handle()))
            .collect();
        for &extra in &self.inherit {
            // SAFETY: a handle the caller passed to be inherited; the call only sets its flag,
            // and fails on a value that is no handle.
            unsafe {
                SetHandleInformation(HANDLE(extra), HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT)
            }
            .map_err(|e| win32("SetHandleInformation", &e))?;
            listed.push(HANDLE(extra));
        }
        let mut attributes = HandleList::new(&listed)?;

        let startup = STARTUPINFOEXW {
            StartupInfo: STARTUPINFOW {
                cb: u32::try_from(size_of::<STARTUPINFOEXW>()).unwrap_or(u32::MAX),
                dwFlags: STARTF_USESTDHANDLES,
                hStdInput: listed[0],
                hStdOutput: listed[1],
                hStdError: listed[2],
                ..STARTUPINFOW::default()
            },
            lpAttributeList: attributes.list(),
        };
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: every pointer is to a live buffer that outlives the call: the NUL-terminated
        // program, command line (writable, as the call requires) and folder, the environment
        // block (UTF-16, double-NUL-terminated), and the extended startup info whose attribute
        // list names only open, inheritable handles. `info` gets two new handles, owned below.
        unsafe {
            CreateProcessAsUserW(
                Some(HANDLE(token.as_raw_handle())),
                PCWSTR(program.as_ptr()),
                Some(PWSTR(line.as_mut_ptr())),
                None,
                None,
                true,
                CREATE_SUSPENDED
                    | CREATE_UNICODE_ENVIRONMENT
                    | CREATE_NO_WINDOW
                    | EXTENDED_STARTUPINFO_PRESENT,
                Some(environment.as_ptr().cast::<c_void>()),
                cwd.as_ref().map_or(PCWSTR::null(), |c| PCWSTR(c.as_ptr())),
                &raw const startup.StartupInfo,
                &raw mut info,
            )
        }
        .map_err(|e| win32("CreateProcessAsUserW", &e))?;
        drop(attributes);
        Ok(Spawned {
            pid: info.dwProcessId,
            // SAFETY: CreateProcessAsUserW succeeded, so both handles are open and ours alone.
            process: unsafe { OwnedHandle::from_raw_handle(info.hProcess.0) },
            // SAFETY: as above.
            thread: unsafe { OwnedHandle::from_raw_handle(info.hThread.0) },
            stdin: stdin.parent,
            stdout: stdout.parent,
            stderr: stderr.parent,
        })
    }

    /// The child's environment block: the token's (unless cleared) with the changes on top.
    fn environment(&self, token: &OwnedHandle) -> Result<Vec<u16>, Error> {
        let base = if self.env_clear {
            Vec::new()
        } else {
            token_environment(token)?
        };
        envblock::block(base, &self.env)
    }
}

impl Spawned {
    /// Lets the child's first thread run.
    pub fn resume(&self) -> Result<(), Error> {
        // SAFETY: `thread` is the child's open thread handle, with THREAD_SUSPEND_RESUME.
        let previous = unsafe { ResumeThread(HANDLE(self.thread.as_raw_handle())) };
        if previous == u32::MAX {
            return Err(last_error("ResumeThread"));
        }
        Ok(())
    }
}

/// `text` in UTF-16 without a NUL (the block's checks refuse one inside).
fn units(text: &OsStr) -> Vec<u16> {
    text.encode_wide().collect()
}

/// The `LowBox` token of `namespace`'s package over this process's primary token: no
/// capabilities, the namespace's six handles.
fn lowbox_token(namespace: &NamespaceHandles) -> Result<OwnedHandle, Error> {
    let mut own = HANDLE::default();
    // SAFETY: the pseudo handle of this process; `own` gets a new handle, owned below.
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ACCESS_MASK(TOKEN_ALL_ACCESS),
            &raw mut own,
        )
    }
    .map_err(|e| win32("OpenProcessToken", &e))?;
    // SAFETY: OpenProcessToken succeeded, so `own` is open and ours alone.
    let own = unsafe { OwnedHandle::from_raw_handle(own.0) };
    let mut sid = namespace.package_sid().to_bytes();
    let handles = namespace.handles();
    let count = u32::try_from(handles.len()).unwrap_or(u32::MAX);
    let mut token = HANDLE::default();
    // SAFETY: `own` is an open primary token; `sid` is a binary SID and `handles` six open
    // handles, both alive for the call; no object attributes and no capabilities. `token` gets
    // a new handle, owned below.
    let status = unsafe {
        NtCreateLowBoxToken(
            &raw mut token,
            HANDLE(own.as_raw_handle()),
            TOKEN_ALL_ACCESS,
            std::ptr::null(),
            PSID(sid.as_mut_ptr().cast::<c_void>()),
            0,
            std::ptr::null(),
            count,
            handles.as_ptr(),
        )
    };
    nt_ok("NtCreateLowBoxToken", status)?;
    // SAFETY: the call succeeded, so `token` is open and ours alone.
    Ok(unsafe { OwnedHandle::from_raw_handle(token.0) })
}

/// The token's own environment (`CreateEnvironmentBlock` with no inheritance), as entries.
fn token_environment(token: &OwnedHandle) -> Result<Vec<Vec<u16>>, Error> {
    let mut block: *mut c_void = std::ptr::null_mut();
    // SAFETY: `token` is an open token with TOKEN_QUERY; `block` gets a block freed below.
    unsafe { CreateEnvironmentBlock(&raw mut block, Some(HANDLE(token.as_raw_handle())), false) }
        .map_err(|e| win32("CreateEnvironmentBlock", &e))?;
    let mut len = 0_usize;
    // SAFETY: the block is a run of NUL-terminated UTF-16 strings ended by an empty one; the
    // walk stops at the first double NUL, inside the block.
    let entries = unsafe {
        let start = block.cast::<u16>().cast_const();
        while *start.add(len) != 0 || *start.add(len + 1) != 0 {
            len += 1;
        }
        envblock::entries(std::slice::from_raw_parts(start, len + 2))
    };
    // SAFETY: the block came from CreateEnvironmentBlock and is freed once, here.
    unsafe { DestroyEnvironmentBlock(block) }.map_err(|e| win32("DestroyEnvironmentBlock", &e))?;
    Ok(entries)
}

/// A one-attribute list: `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` over handles that outlive it.
struct HandleList {
    /// The list's storage, 8-byte aligned.
    storage: Vec<u64>,
}

impl HandleList {
    /// The list naming `handles`, which must outlive it and stay put.
    fn new(handles: &[HANDLE]) -> Result<HandleList, Error> {
        let mut size = 0_usize;
        // SAFETY: the size query: no list, `size` gets the bytes needed. It reports
        // ERROR_INSUFFICIENT_BUFFER by design, so its result is not an error here.
        let _sized: ::windows::core::Result<()> =
            unsafe { InitializeProcThreadAttributeList(None, 1, None, &raw mut size) };
        let mut storage = vec![0_u64; size.div_ceil(8)];
        let raw = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast::<c_void>());
        // SAFETY: `storage` holds at least `size` bytes, 8-byte aligned.
        unsafe { InitializeProcThreadAttributeList(Some(raw), 1, None, &raw mut size) }
            .map_err(|e| win32("InitializeProcThreadAttributeList", &e))?;
        // From here Drop deletes the initialised list, on success and on failure alike.
        let mut list = HandleList { storage };
        // SAFETY: the list is initialised; `handles` outlives it (the caller's contract) and
        // the size is the slice's.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                list.list(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(handles.as_ptr().cast::<c_void>()),
                size_of_val(handles),
                None,
                None,
            )
        };
        // On failure `list` drops and deletes the initialised list.
        updated.map_err(|e| win32("UpdateProcThreadAttribute", &e))?;
        Ok(list)
    }

    /// The list's pointer.
    fn list(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        LPPROC_THREAD_ATTRIBUTE_LIST(self.storage.as_mut_ptr().cast::<c_void>())
    }
}

impl Drop for HandleList {
    fn drop(&mut self) {
        // SAFETY: the list was initialised in `new` (a HandleList exists only after that) and
        // is deleted once, here.
        unsafe { DeleteProcThreadAttributeList(self.list()) };
    }
}
