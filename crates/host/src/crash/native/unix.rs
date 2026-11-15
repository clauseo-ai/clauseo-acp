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
/// The lowest `si_code` of a signal a process sent on macOS: `SI_USER`, `SI_QUEUE`, `SI_TIMER`,
/// `SI_ASYNCIO` and `SI_MESGQ` are `0x10001` to `0x10005`.
const APPLE_SENT_MIN: libc::c_int = 0x10000;

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

/// Whether `si_code` says a process sent the signal (`kill`, `sigqueue`, `raise`, a timer), not
/// the kernel for a fault. Linux: `si_code <= 0` is sent (`SI_USER` 0, `SI_QUEUE` -1, `SI_TKILL`
/// -6, …), a fault's codes are positive. macOS: the sent codes are `0x10001` and up, a fault's
/// are small and positive, and `0` (which `kill` has given) is taken as sent too.
const fn sent(si_code: libc::c_int, os: Os) -> bool {
    match os {
        Os::Linux => si_code <= 0,
        Os::Apple => si_code <= 0 || si_code >= APPLE_SENT_MIN,
    }
}

/// The decision for a signal on `os`: `abort` for SIGABRT, `si_code` none when the kernel gave
/// no siginfo. A fault the kernel raised is recorded and returns into the previous action.
/// SIGABRT is recorded however it came, except on Linux one another process sent (`SI_USER`).
/// Anything sent is raised again.
fn decide(abort: bool, si_code: Option<libc::c_int>, os: Os) -> Decision {
    let sent = si_code.is_none_or(|code| sent(code, os));
    let killed = match (os, si_code) {
        (Os::Linux, Some(code)) => code == LINUX_SI_USER,
        (Os::Linux, None) => true,
        (Os::Apple, _) => false,
    };
    Decision {
        record: !sent || (abort && !killed),
        raise_again: sent,
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

/// The handler. Records a kernel fault, or SIGABRT unless (on Linux) another process sent it
/// ([`decide`]), then puts the previous action back; a sent signal is raised again for it.
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
    const ABORTED: Decision = Decision {
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
        assert_eq!(decide(true, Some(-6), Os::Linux), ABORTED);
        assert_eq!(decide(true, Some(0), Os::Linux), PASSED_ON);
        assert_eq!(decide(true, None, Os::Linux), PASSED_ON);
    }

    #[test]
    fn macos_takes_0x10001_and_up_as_sent() {
        // SEGV_MAPERR, SEGV_ACCERR, BUS_ADRALN, ILL_ILLOPC: the kernel's.
        for code in [1, 2, 3, 4] {
            assert_eq!(decide(false, Some(code), Os::Apple), FAULT, "{code}");
        }
        // SI_USER (kill -SEGV from another process), SI_QUEUE, SI_TIMER, SI_ASYNCIO, SI_MESGQ,
        // the 0 kill has given, and no siginfo.
        for code in [
            Some(0x10001),
            Some(0x10002),
            Some(0x10003),
            Some(0x10004),
            Some(0x10005),
            Some(0),
            None,
        ] {
            assert_eq!(decide(false, code, Os::Apple), PASSED_ON, "{code:?}");
        }
        // Linux's rule would have recorded the sent SI_USER as a fault.
        assert_eq!(decide(false, Some(0x10001), Os::Linux), FAULT);
        // SIGABRT is recorded however it came, and raised again.
        for code in [Some(0x10001), Some(0), None] {
            assert_eq!(decide(true, code, Os::Apple), ABORTED, "{code:?}");
        }
    }
}
