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

/// Linux's `si_code` of a signal another process sent with `kill`.
const LINUX_SI_USER: libc::c_int = 0;

/// Whose `si_code` values the handler reads: they differ between macOS and Linux.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Os {
    Apple,
    Linux,
}

/// The OS this build runs on.
const OS: Os = if cfg!(target_vendor = "apple") {
    Os::Apple
} else {
    Os::Linux
};

/// What the handler does with one signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Decision {
    /// Whether it is a crash, recorded.
    record: bool,
    /// Whether it is raised again once the previous action is back: a sent signal (or abort's)
    /// does not come back by returning, a fault does.
    raise_again: bool,
}

/// The decision for a signal on `os`: `abort` for SIGABRT, `si_code` none when the kernel gave
/// no siginfo.
///
/// Linux: `si_code <= 0` means a process sent it (`SI_USER` 0, `SI_QUEUE` -1, `SI_TKILL` -6, …);
/// a fault's codes are positive. A fault is recorded and returns into the previous action; a
/// sent signal is raised again, and recorded only if it is SIGABRT that was not another
/// process's `kill` (`SI_USER`).
///
/// macOS cannot tell a sent SIGSEGV, SIGBUS or SIGILL from a fault: XNU's `sendsig` sets fixed
/// codes on arm64 (SIGSEGV always `SEGV_ACCERR`, SIGBUS `BUS_ADRALN`, SIGILL `ILL_ILLTRP`), and
/// on Intel a real general protection fault gets 0 (`bsd/dev/arm/unix_signal.c` and
/// `bsd/dev/i386/unix_signal.c`). So every one is recorded and raised again under the previous
/// action: a fault ends as it would have, and an outside `kill -SEGV` leaves one report (Rust's
/// handler then resets to the default and returns, so with no fault to repeat the process may
/// carry on).
fn decide(abort: bool, si_code: Option<libc::c_int>, os: Os) -> Decision {
    match os {
        Os::Apple => Decision {
            record: true,
            raise_again: true,
        },
        Os::Linux => {
            let sent = si_code.is_none_or(|code| code <= 0);
            let killed = si_code.is_none_or(|code| code == LINUX_SI_USER);
            Decision {
                record: !sent || (abort && !killed),
                raise_again: sent,
            }
        }
    }
}

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

/// The handler. Records what [`decide`] says (on Linux a kernel fault, or SIGABRT unless another
/// process sent it; on macOS every one), then puts the previous action back and raises the
/// signal again when [`decide`] says so.
extern "C" fn handle(signum: libc::c_int, info: *mut libc::siginfo_t, _context: *mut libc::c_void) {
    let Some(index) = SIGNALS
        .iter()
        .position(|signal| *signal as libc::c_int == signum)
    else {
        return;
    };
    // SAFETY: with SA_SIGINFO the kernel passes a valid siginfo_t, checked for null anyway.
    let si_code = unsafe { info.as_ref() }.map(|info| info.si_code);
    let decision = decide(signum == libc::SIGABRT, si_code, OS);
    if decision.record {
        super::on_fault(signum.cast_unsigned());
    }
    if let (Some(signal), Some(previous)) = (SIGNALS.get(index), PREVIOUS.get(index)) {
        if let Some(previous) = previous.get() {
            // SAFETY: puts back the action that was in place before ours.
            let _restored = unsafe { sigaction(*signal, previous) };
        }
        if decision.raise_again {
            // A sent signal (or abort's) does not come back by returning: raise it again, to
            // be delivered under the previous action once this handler returns.
            let _raised = raise(*signal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Decision, Os, decide};

    const FAULT: Decision = Decision {
        record: true,
        raise_again: false,
    };
    const PASSED_ON: Decision = Decision {
        record: false,
        raise_again: true,
    };
    const RECORDED_RAISED: Decision = Decision {
        record: true,
        raise_again: true,
    };

    #[test]
    fn linux_records_kernel_faults_and_passes_sent_signals_on() {
        // SEGV_MAPERR, SEGV_ACCERR, BUS_ADRERR, FPE_INTDIV: the kernel's.
        for code in [1, 2, 3, 4] {
            assert_eq!(decide(false, Some(code), Os::Linux), FAULT, "{code}");
        }
        // SI_USER, SI_QUEUE, SI_TIMER, SI_MESGQ, SI_ASYNCIO, SI_TKILL, and no siginfo.
        for code in [
            Some(0),
            Some(-1),
            Some(-2),
            Some(-3),
            Some(-4),
            Some(-6),
            None,
        ] {
            assert_eq!(decide(false, code, Os::Linux), PASSED_ON, "{code:?}");
        }
        // abort() raises with SI_TKILL; another process's kill -ABRT is SI_USER.
        assert_eq!(decide(true, Some(-6), Os::Linux), RECORDED_RAISED);
        assert_eq!(decide(true, Some(0), Os::Linux), PASSED_ON);
        assert_eq!(decide(true, None, Os::Linux), PASSED_ON);
    }

    #[test]
    fn macos_records_every_code_and_raises_it_again() {
        // XNU gives a sent SIGSEGV SEGV_ACCERR (2) on arm64, and a real Intel general protection
        // fault 0, so no code is taken as sent: faults, SI_USER and friends, and no siginfo.
        for code in [
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            Some(0),
            Some(-1),
            Some(0x10001),
            Some(0x10005),
            None,
        ] {
            for abort in [false, true] {
                assert_eq!(
                    decide(abort, code, Os::Apple),
                    RECORDED_RAISED,
                    "{code:?} {abort}"
                );
            }
        }
    }
}
