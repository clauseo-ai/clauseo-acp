//! macOS and Linux: `sigaction` for SIGSEGV, SIGBUS, SIGILL, SIGFPE and SIGABRT, on the
//! alternate stack (Rust gives each of its threads one, so a stack overflow is still handled).
//! The handler records, puts back the action that was there before (Rust's own stack overflow
//! handler, or the default), and returns: a fault runs again under it, and a signal that was
//! sent is raised again.

#![expect(
    unsafe_code,
    reason = "sigaction installs a signal handler, and the handler reads the kernel's siginfo"
)]

use std::sync::OnceLock;

use nix::libc;
use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, raise, sigaction};

/// The signals that mean a crash.
const SIGNALS: [Signal; 5] = [
    Signal::SIGSEGV,
    Signal::SIGBUS,
    Signal::SIGILL,
    Signal::SIGFPE,
    Signal::SIGABRT,
];

/// The action each signal had before ours, in `SIGNALS`' order.
static PREVIOUS: [OnceLock<SigAction>; 5] = [const { OnceLock::new() }; 5];

/// `si_code` of a signal another process sent with `kill`.
const SI_USER: libc::c_int = 0;

/// Installs the handler for each signal. Returns whether every one took it.
pub(super) fn install() -> bool {
    let action = SigAction::new(
        SigHandler::SigAction(handle),
        SaFlags::SA_SIGINFO | SaFlags::SA_ONSTACK,
        SigSet::empty(),
    );
    let mut all = true;
    for (signal, previous) in SIGNALS.iter().zip(&PREVIOUS) {
        // SAFETY: `handle` calls only async-signal-safe code (atomics, `write`, `sigaction`,
        // `raise`), and the previous action is kept to be put back.
        match unsafe { sigaction(*signal, &action) } {
            Ok(old) => {
                let _first = previous.set(old);
            }
            Err(_) => all = false,
        }
    }
    all
}

/// The handler. Records a kernel fault, or SIGABRT unless (on Linux) another process sent it,
/// then puts the previous action back; a sent signal is raised again for it.
extern "C" fn handle(signum: libc::c_int, info: *mut libc::siginfo_t, _context: *mut libc::c_void) {
    let Some(index) = SIGNALS
        .iter()
        .position(|signal| *signal as libc::c_int == signum)
    else {
        return;
    };
    // SAFETY: with SA_SIGINFO the kernel passes a valid siginfo_t, checked for null anyway.
    let si_code = unsafe { info.as_ref() }.map_or(SI_USER, |info| info.si_code);
    let from_kernel = si_code > 0;
    let abort = signum == libc::SIGABRT && !(cfg!(target_os = "linux") && si_code == SI_USER);
    if from_kernel || abort {
        super::on_fault(signum.cast_unsigned());
    }
    if let (Some(signal), Some(previous)) = (SIGNALS.get(index), PREVIOUS.get(index)) {
        if let Some(previous) = previous.get() {
            // SAFETY: puts back the action that was in place before ours.
            let _restored = unsafe { sigaction(*signal, previous) };
        }
        if !from_kernel {
            // A sent signal (or abort's) does not come back by returning: raise it again, to
            // be delivered under the previous action once this handler returns.
            let _raised = raise(*signal);
        }
    }
}
