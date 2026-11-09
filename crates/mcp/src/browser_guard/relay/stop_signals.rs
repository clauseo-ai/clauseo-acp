//! Unix, listen mode only: SIGTERM and SIGINT end the guard as its normal end does, so the
//! browser is stopped, the profile deleted and the listen socket removed (`Socket`'s drop, under
//! its device and inode check). A handler writes one byte to a pipe, the only thing it does; a
//! thread waiting on the pipe's other end then calls the caller's function (the relay sends its
//! event loop the end). That thread puts both signals back to their default first, so a second
//! one ends the guard at once. A guard killed outright (SIGKILL) leaves the socket: the host
//! (J4) removes it before it starts another guard, which refuses a path that exists.
//!
//! The handlers do not reach a child: a caught signal goes back to its default at `exec`, and the
//! pipe is close-on-exec, made under the spawn lock (`transport::unix::hold_accepts`).
#![expect(
    unsafe_code,
    reason = "sigaction for SIGTERM and SIGINT, and the handler's write to its pipe; nix's \
              sigaction is unsafe and libc's write is the async-signal-safe call"
)]

use std::io::{self, Read};
use std::os::fd::{IntoRawFd, OwnedFd};
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;

use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

/// The pipe's write end, for the handler; -1 until [`watch`] has made it.
static WRITE_END: AtomicI32 = AtomicI32::new(-1);

/// The two signals a graceful stop is asked with.
const SIGNALS: [Signal; 2] = [Signal::SIGTERM, Signal::SIGINT];

/// Writes one byte to the pipe; async-signal-safe (an atomic load and `write`), errno kept.
extern "C" fn on_signal(_signal: nix::libc::c_int) {
    let saved = nix::errno::Errno::last_raw();
    let fd = WRITE_END.load(Ordering::SeqCst);
    if fd >= 0 {
        // SAFETY: `fd` is the pipe's write end, never closed; the byte lives through the call.
        // A full pipe (non-blocking) fails the write, and a byte is waiting already.
        let _written = unsafe { nix::libc::write(fd, [1_u8].as_ptr().cast(), 1) };
    }
    nix::errno::Errno::set_raw(saved);
}

/// Calls `then` on a thread of its own at the first SIGTERM or SIGINT. Call it once.
pub(crate) fn watch(then: impl FnOnce() + Send + 'static) -> io::Result<()> {
    let (read_end, write_end) = {
        let _spawns = crate::transport::unix::hold_accepts();
        let (read_end, write_end) = nix::unistd::pipe()?;
        for end in [&read_end, &write_end] {
            fcntl(end, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
        }
        fcntl(&write_end, FcntlArg::F_SETFL(OFlag::O_NONBLOCK))?;
        (read_end, write_end)
    };
    // Kept open for the guard's life: the handler may run at any time.
    WRITE_END.store(write_end.into_raw_fd(), Ordering::SeqCst);
    let action = SigAction::new(
        SigHandler::Handler(on_signal),
        SaFlags::SA_RESTART,
        SigSet::empty(),
    );
    for signal in SIGNALS {
        // SAFETY: `on_signal` is async-signal-safe (above).
        unsafe { sigaction(signal, &action) }?;
    }
    drop(thread::spawn(move || wait(read_end, then)));
    Ok(())
}

/// Waits for the handler's byte, puts the signals back to their default, then calls `then`.
fn wait(read_end: OwnedFd, then: impl FnOnce()) {
    let mut pipe = std::fs::File::from(read_end);
    let mut byte = [0_u8; 1];
    loop {
        match pipe.read(&mut byte) {
            Ok(1) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            // The write end never closes; nothing else ends the wait.
            _ => return,
        }
    }
    let default = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());
    for signal in SIGNALS {
        // SAFETY: the default action installs no handler.
        if unsafe { sigaction(signal, &default) }.is_err() {}
    }
    then();
}
