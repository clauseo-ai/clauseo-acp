//! The browser over a `DevTools` pipe instead of a debugging port, on every system (route 2,
//! decision record 0060).
//!
//! A sandboxed command can reach 127.0.0.1, and a debugging port has no authentication: any such
//! command could drive the browser past the guard. So the browser gets no port.
//! `--remote-debugging-pipe` makes it read CDP from one pipe and write to another
//! (`remote_debugging_server.cc` opens the pipe only with that switch), each message ended by a
//! NUL. The guard makes both pipes and keeps its two ends in the relay ([`Devtools`]), which
//! agent-browser reaches; the browser's ends are, on each system:
//!
//! - macOS and Linux: fds 3 (it reads) and 4 (it writes), Chromium's `kReadFD` and `kWriteFD`.
//!   The guard's copies sit at fds of 10 or more, close-on-exec, so the child's `dup2` onto 3 and
//!   4 (between fork and exec, the only code there) never meets one of its own.
//! - Windows: two synchronous anonymous pipes (`CreatePipe`; the browser's `ReadFile` and
//!   `WriteFile` block), the browser's ends duplicated as inheritable and named by their decimal
//!   handle values in `--remote-debugging-io-pipes=<read>,<write>`
//!   (`devtools_agent_host_impl.cc`, `AdoptPipes`, `AdoptHandle`: `StringToUint`,
//!   `Uint32ToHandle`, a `FILE_TYPE_PIPE` check). The two go in the child's handle list with its
//!   standard handles and nothing else (`job/spawn.rs`), and the browser starts in the guard's
//!   job with the user's default process DACL.
//!
//! The browser is ready once it answers the guard's `Browser.getVersion` on the pipe within the
//! start wait; one that exits first or never answers (a firm policy that turned debugging off)
//! fails the start (`session/browser.rs` then tries the next one on Windows). One that says on
//! stderr, before or with its answer, that its sandbox cannot start is refused
//! ([`StartError::Sandbox`], `chrome/sandbox.rs`) and never started again without it.

use std::io;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

use serde_json::json;

use super::super::devtools::Devtools;
use super::super::parent::{self, Parent};
use super::super::session::StartError;
use super::{Child, Seen, Spec, quote, sandbox};

/// How long a start that failed on the pipe waits for the browser's stderr to say why (its
/// helpers can hold stderr open after the browser exits, so it is not waited out).
const EXIT_SETTLE: Duration = Duration::from_secs(2);

/// Starts the browser at `spec`'s executable with a `DevTools` pipe, the guard's ends in the
/// returned relay, with its arguments for `host`. On Windows in `job`.
pub(super) fn spawn(
    spec: &Spec<'_>,
    profile: &Path,
    host: sandbox::Host,
    #[cfg(windows)] job: &super::super::job::Job,
) -> io::Result<(Child, Devtools)> {
    #[cfg(unix)]
    return unix::spawn(spec, profile, host);
    #[cfg(windows)]
    return win::spawn(spec, profile, host, job);
}

/// Waits for the browser's answer to `Browser.getVersion` on the pipe, at most `wait`, looking
/// at its stderr reader's `starting` (an exit, or a sandbox that cannot start) and the guard's
/// parent every [`parent::POLL`].
pub(super) fn wait_ready(
    devtools: &Devtools,
    starting: &Receiver<Seen>,
    wait: Duration,
    parent: &Parent,
) -> Result<(), StartError> {
    // A browser that exited at once closed its end before the guard's first write.
    let (_, reply) = devtools
        .command("Browser.getVersion", json!({}), None)
        .map_err(|error| match ended(starting) {
            StartError::Other(reason) => format!("{reason} (writing to it: {error})").into(),
            sandbox @ StartError::Sandbox(_) => sandbox,
        })?;
    let polls = wait.as_millis() / parent::POLL.as_millis();
    for _ in 0..polls {
        let answered = match reply.recv_timeout(parent::POLL) {
            Ok(Ok(_)) => true,
            Ok(Err(error)) => {
                return Err(match seen(starting) {
                    Some(Seen::Sandbox(said)) => StartError::Sandbox(Some(said)),
                    _ => format!("the browser refused Browser.getVersion: {error}").into(),
                });
            }
            Err(RecvTimeoutError::Disconnected) => return Err(ended(starting)),
            Err(RecvTimeoutError::Timeout) => false,
        };
        match seen(starting) {
            // Chrome said its sandbox cannot start, before or with its answer: refused, even
            // though it would go on.
            Some(Seen::Sandbox(said)) => return Err(StartError::Sandbox(Some(said))),
            Some(Seen::Failed(_)) | None if answered => return Ok(()),
            Some(Seen::Failed(tail)) => return Err(stopped(&tail)),
            None => {}
        }
        if parent.gone() {
            return Err(super::super::PARENT_GONE.to_owned().into());
        }
    }
    Err(StartError::Other(format!(
        "the browser did not answer on its DevTools pipe within {} s (a firm policy may turn \
         its remote debugging off)",
        wait.as_secs()
    )))
}

/// Why the browser's pipe ended before its answer, once its stderr reader says (at most
/// [`EXIT_SETTLE`]): its sandbox could not start, or it stopped, with its last lines.
fn ended(starting: &Receiver<Seen>) -> StartError {
    match starting.recv_timeout(EXIT_SETTLE) {
        Ok(Seen::Sandbox(said)) => StartError::Sandbox(Some(said)),
        Ok(Seen::Failed(tail)) => stopped(&tail),
        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => stopped(&[]),
    }
}

/// The start's failure for a browser that stopped before it answered; `tail` its last lines.
fn stopped(tail: &[String]) -> StartError {
    StartError::Other(format!(
        "the browser stopped before it answered on its DevTools pipe{}",
        quote(tail)
    ))
}

/// What the browser's stderr reader reported, if anything yet: a sandbox failure, or its exit
/// (its last lines; none when the reader is gone).
fn seen(starting: &Receiver<Seen>) -> Option<Seen> {
    match starting.try_recv() {
        Ok(seen) => Some(seen),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Seen::Failed(Vec::new())),
    }
}

#[cfg(unix)]
mod unix {
    use std::io::{self, PipeReader, PipeWriter};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::super::super::devtools::Devtools;
    use super::super::{Child, Spec, chrome_args, sandbox};

    /// The fd the browser reads CDP from, and the one it writes to (Chromium's `kReadFD`,
    /// `kWriteFD`).
    const BROWSER_READS: i32 = 3;
    const BROWSER_WRITES: i32 = 4;
    /// The lowest fd the browser's ends are moved to before the fork.
    const ABOVE: i32 = 10;

    #[expect(
        clippy::disallowed_methods,
        reason = "the guard's second fixed child: the host's Chrome, whose tree the guard kills \
                  (docs/decisions/0029-browser.md)"
    )]
    #[expect(
        unsafe_code,
        reason = "pre_exec, to put the DevTools pipe's ends at fds 3 and 4 in the browser"
    )]
    pub(super) fn spawn(
        spec: &Spec<'_>,
        profile: &Path,
        host: sandbox::Host,
    ) -> io::Result<(Child, Devtools)> {
        let (browser_reads, guard_writes) = io::pipe()?;
        let (guard_reads, browser_writes) = io::pipe()?;
        let theirs_read = above(&OwnedFd::from(browser_reads))?;
        let theirs_write = above(&OwnedFd::from(browser_writes))?;
        let (from, to) = (theirs_read.as_raw_fd(), theirs_write.as_raw_fd());
        let mut command = Command::new(spec.executable);
        command
            .args(chrome_args(spec, profile, host, None))
            .current_dir(spec.cwd)
            .env_clear()
            .envs(spec.env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .process_group(0);
        // SAFETY: the closure runs in the child between fork and exec and calls only dup2,
        // which is async-signal-safe. `from` and `to` are open fds of 10 or more (so neither is
        // 3 or 4), kept open by `theirs_read` and `theirs_write` until the spawn returns; dup2
        // clears close-on-exec on 3 and 4 only.
        unsafe {
            command.pre_exec(move || {
                if nix::libc::dup2(from, BROWSER_READS) < 0
                    || nix::libc::dup2(to, BROWSER_WRITES) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        drop((theirs_read, theirs_write));
        let devtools = Devtools::start(
            Box::new(PipeWriter::from(OwnedFd::from(guard_writes))),
            Box::new(PipeReader::from(OwnedFd::from(guard_reads))),
        )?;
        Ok((child, devtools))
    }

    /// A close-on-exec copy of `fd` at fd [`ABOVE`] or more.
    #[expect(unsafe_code, reason = "taking ownership of the fd fcntl just made")]
    fn above(fd: &OwnedFd) -> io::Result<OwnedFd> {
        let raw = nix::fcntl::fcntl(fd, nix::fcntl::FcntlArg::F_DUPFD_CLOEXEC(ABOVE))
            .map_err(io::Error::from)?;
        // SAFETY: `raw` is the new fd fcntl just returned, which nothing else owns.
        Ok(unsafe { OwnedFd::from_raw_fd(raw) })
    }
}

#[cfg(windows)]
mod win {
    use std::fs::File;
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::path::Path;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Pipes::CreatePipe;

    use super::super::super::devtools::Devtools;
    use super::super::super::job::{Io, Job, Launch, inheritable};
    use super::super::{Child, Spec, chrome_args, sandbox};

    pub(super) fn spawn(
        spec: &Spec<'_>,
        profile: &Path,
        host: sandbox::Host,
        job: &Job,
    ) -> io::Result<(Child, Devtools)> {
        let (browser_reads, guard_writes) = anonymous_pipe()?;
        let (guard_reads, browser_writes) = anonymous_pipe()?;
        let theirs_read = inheritable(HANDLE(browser_reads.as_raw_handle()))?;
        let theirs_write = inheritable(HANDLE(browser_writes.as_raw_handle()))?;
        drop((browser_reads, browser_writes));
        let io_pipes = (handle_value(&theirs_read)?, handle_value(&theirs_write)?);
        let args = chrome_args(spec, profile, host, Some(io_pipes));
        let inherit = [theirs_read.as_raw_handle(), theirs_write.as_raw_handle()];
        let child = job.spawn(&Launch {
            program: spec.executable,
            args: &args,
            cwd: spec.cwd,
            env: spec.env,
            stdin: Io::Null,
            stdout: Io::Null,
            stderr: Io::Piped,
            inherit: &inherit,
        })?;
        // The browser has its own copies now.
        drop((theirs_read, theirs_write));
        let devtools = Devtools::start(
            Box::new(File::from(guard_writes)),
            Box::new(File::from(guard_reads)),
        )?;
        Ok((child, devtools))
    }

    /// A synchronous anonymous pipe, neither end inheritable: (read end, write end).
    #[expect(
        unsafe_code,
        reason = "FFI to CreatePipe, for the browser's DevTools pipe"
    )]
    fn anonymous_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
        let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
        // SAFETY: both are live HANDLEs the call writes; no security attributes (not
        // inheritable), the default buffer size.
        unsafe { CreatePipe(&raw mut read, &raw mut write, None, 0) }.map_err(io::Error::from)?;
        // SAFETY: both are open handles CreatePipe just returned, which nothing else owns; each
        // `OwnedHandle` closes its own once.
        Ok(unsafe {
            (
                OwnedHandle::from_raw_handle(read.0),
                OwnedHandle::from_raw_handle(write.0),
            )
        })
    }

    /// The handle's value as Chromium reads it: a decimal `u32` (`base::win::Uint32ToHandle`).
    fn handle_value(handle: &OwnedHandle) -> io::Result<u32> {
        u32::try_from(handle.as_raw_handle().addr()).map_err(io::Error::other)
    }
}
