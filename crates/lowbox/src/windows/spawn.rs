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

use ::windows::Win32::Foundation::HANDLE;
use ::windows::Win32::Security::{PSID, TOKEN_ACCESS_MASK};
use ::windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use ::windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW,
    CreateProcessW, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, OpenProcessToken,
    PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
};
use ::windows::core::{PCWSTR, PWSTR};

use super::inherit::{HandleList, Marked};
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

    /// Passes `handle` to the child at the same value, for a value the child is told on its
    /// command line (`--remote-debugging-io-pipes`, for one). The child inherits exactly these
    /// handles and its three standard ones, through `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, and no
    /// other inheritable handle of this process. Each is made inheritable for the spawn only and
    /// put back as it was. The spawn refuses a null or pseudo handle, or one listed twice.
    pub fn inherit_handle(&mut self, handle: RawHandle) -> &mut LowBoxCommand {
        self.inherit.push(handle);
        self
    }

    /// Makes the `LowBox` token of `namespace`'s package over this process's token and starts the
    /// program in it, suspended, on this process's station and desktop. The caller puts the
    /// child in its Job Object, then calls [`Spawned::resume`].
    ///
    /// The browser's `DevTools` over two anonymous pipes, with no TCP listener in the `LowBox`
    /// (Chromium reads `--remote-debugging-io-pipes=<in>,<out>` with `--remote-debugging-pipe`;
    /// the values are the handles, in decimal):
    ///
    /// ```no_run
    /// use std::os::windows::io::AsRawHandle;
    ///
    /// use clauseo_lowbox::{LowBoxCommand, NamespaceHandles, PackageName, Stdio};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let (package, values) = (PackageName::new("Clauseo.Browser.B01")?, [0_usize; 6]);
    /// let namespace = NamespaceHandles::from_inherited(&package, &values)?;
    /// // Commands go in on one pipe, replies and events come back on the other.
    /// let (chrome_reads, guard_writes) = std::io::pipe()?;
    /// let (guard_reads, chrome_writes) = std::io::pipe()?;
    /// let (r, w) = (chrome_reads.as_raw_handle(), chrome_writes.as_raw_handle());
    /// let child = LowBoxCommand::new(r"C:\shell\chrome-headless-shell.exe")
    ///     .args(["--headless", "--remote-debugging-pipe"])
    ///     .arg(format!("--remote-debugging-io-pipes={},{}", r as usize, w as usize))
    ///     .stdout(Stdio::Null)
    ///     .inherit_handle(r)
    ///     .inherit_handle(w)
    ///     .spawn_suspended(&namespace)?;
    /// // The child has its own copies now; the guard keeps only its ends.
    /// drop((chrome_reads, chrome_writes));
    /// child.resume()?;
    /// // Write NUL-terminated JSON commands to `guard_writes`, read replies from `guard_reads`.
    /// # drop((guard_writes, guard_reads));
    /// # Ok(())
    /// # }
    /// ```
    pub fn spawn_suspended(&self, namespace: &NamespaceHandles) -> Result<Spawned, Error> {
        let token = lowbox_token(namespace)?;
        self.spawn_as(Some(&token))
    }

    /// Starts the program suspended with `token` (`CreateProcessAsUserW`), or with this process's
    /// own token when `None` (`CreateProcessW`, for the tests of the handle list), inheriting
    /// only the standard handles and the listed ones.
    pub(super) fn spawn_as(&self, token: Option<&OwnedHandle>) -> Result<Spawned, Error> {
        let mut line = command_line(&self.program, &self.args)?;
        line.push(0);
        let program = wide_nul(&self.program);
        let cwd = self.cwd.as_deref().map(|d| wide_nul(d.as_os_str()));
        let environment = self.environment(token)?;
        let marked = Marked::new(&self.inherit)?;

        let stdin = self.stdin.ends(Which::Input)?;
        let stdout = self.stdout.ends(Which::Output)?;
        let stderr = self.stderr.ends(Which::Error)?;
        let listed: Vec<HANDLE> = [&stdin.child, &stdout.child, &stderr.child]
            .iter()
            .map(|h| HANDLE(h.as_raw_handle()))
            .chain(marked.handles())
            .collect();
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
        let flags = CREATE_SUSPENDED
            | CREATE_UNICODE_ENVIRONMENT
            | CREATE_NO_WINDOW
            | EXTENDED_STARTUPINFO_PRESENT;
        let block = Some(environment.as_ptr().cast::<c_void>());
        let folder = cwd.as_ref().map_or(PCWSTR::null(), |c| PCWSTR(c.as_ptr()));
        let mut info = PROCESS_INFORMATION::default();
        // Both calls: every pointer is to a live buffer that outlives the call: the NUL-terminated
        // program, command line (writable, as the calls require) and folder, the environment
        // block (UTF-16, double-NUL-terminated), and the extended startup info whose attribute
        // list names only open handles marked inheritable. `info` gets two new handles.
        let created = match token {
            // SAFETY: as above, and `token` is an open primary token.
            Some(token) => unsafe {
                CreateProcessAsUserW(
                    Some(HANDLE(token.as_raw_handle())),
                    PCWSTR(program.as_ptr()),
                    Some(PWSTR(line.as_mut_ptr())),
                    None,
                    None,
                    true,
                    flags,
                    block,
                    folder,
                    &raw const startup.StartupInfo,
                    &raw mut info,
                )
            }
            .map_err(|e| win32("CreateProcessAsUserW", &e)),
            // SAFETY: as above.
            None => unsafe {
                CreateProcessW(
                    PCWSTR(program.as_ptr()),
                    Some(PWSTR(line.as_mut_ptr())),
                    None,
                    None,
                    true,
                    flags,
                    block,
                    folder,
                    &raw const startup.StartupInfo,
                    &raw mut info,
                )
            }
            .map_err(|e| win32("CreateProcessW", &e)),
        };
        drop(attributes);
        drop(marked);
        created?;
        Ok(Spawned {
            pid: info.dwProcessId,
            // SAFETY: the create call succeeded, so both handles are open and ours alone.
            process: unsafe { OwnedHandle::from_raw_handle(info.hProcess.0) },
            // SAFETY: as above.
            thread: unsafe { OwnedHandle::from_raw_handle(info.hThread.0) },
            stdin: stdin.parent,
            stdout: stdout.parent,
            stderr: stderr.parent,
        })
    }

    /// The child's environment block: the token's, or this process's without a token (unless
    /// cleared), with the changes on top.
    fn environment(&self, token: Option<&OwnedHandle>) -> Result<Vec<u16>, Error> {
        let base = match (self.env_clear, token) {
            (true, _) => Vec::new(),
            (false, Some(token)) => token_environment(token)?,
            (false, None) => std::env::vars_os()
                .map(|(name, value)| {
                    let mut entry = units(&name);
                    entry.push(u16::from(b'='));
                    entry.extend(units(&value));
                    entry
                })
                .collect(),
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
