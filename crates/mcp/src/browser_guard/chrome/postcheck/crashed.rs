//! Windows: a failed start reads as the sandbox's failure when Chrome crashed (decision record
//! 0029, "A silent start failure on Windows"). In stage C's Windows run (chrome-headless-shell
//! 157, sandbox on) Chrome's main process crashed about 70 ms after its start and printed
//! nothing. In the agent's sandbox job (`JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION`) such a
//! crash ends the process at once, its exit code the exception's; so a Chrome that exits before
//! its endpoint with a [`crash`] code is the sandbox's refusal (`postcheck.rs`,
//! `crashed_at_start`). A sandboxed child counts too: while the guard waits for the endpoint it
//! looks at Chrome's children every poll ([`Crashes::sample`]), holds each one open so its exit
//! code stays readable, and notes those it saw sandboxed (the check's own token rule,
//! `judge.rs`); [`Crashes::sandbox_failed`] says whether one of those exited with a [`crash`]
//! code. Anything else (a timeout, an ordinary exit code) stays the ordinary failed start: a slow
//! first start, an antivirus scanning a fresh unzip, never claims the protection failed.
//!
//! Plain logic over [`Source`] and [`Exits`], tested on every OS; `windows.rs` reads the OS.

use std::collections::BTreeSet;

use super::Source;

/// What the watch reads beyond [`Source`].
pub(crate) trait Exits {
    /// Keeps `pid` open, so its exit code stays readable once it has exited.
    fn hold(&mut self, pid: u32);
    /// The exit code of `pid`, held before, once it has exited; `None` while it runs or when the
    /// code cannot be read.
    fn exit_code(&mut self, pid: u32) -> Option<u32>;
}

/// What the watch has seen of Chrome's children during one wait for its endpoint.
#[derive(Debug, Default)]
pub(crate) struct Crashes {
    seen: BTreeSet<u32>,
    sandboxed: BTreeSet<u32>,
}

impl Crashes {
    /// One look: holds each new child, and notes each child that is sandboxed now.
    pub(crate) fn sample(&mut self, source: &mut dyn Source, exits: &mut dyn Exits, chrome: u32) {
        for pid in source.below(chrome).unwrap_or_default() {
            if self.seen.insert(pid) {
                exits.hold(pid);
            }
            if !self.sandboxed.contains(&pid) && source.sandboxed(chrome, pid).is_ok() {
                self.sandboxed.insert(pid);
            }
        }
    }

    /// Whether a child seen sandboxed has exited with a [`crash`] code.
    pub(crate) fn sandbox_failed(&self, exits: &mut dyn Exits) -> bool {
        self.sandboxed
            .iter()
            .any(|&pid| exits.exit_code(pid).is_some_and(crash))
    }
}

/// Whether `code` reads as the sandbox's failure: `0x80000003`, a breakpoint (Chromium's
/// `CHECK` and `IMMEDIATE_CRASH`, the code stage C measured for Chrome's crash at start under
/// the agent's token); `0xC0000005`, an access violation (a sandboxed process touching what its
/// token no longer reaches); and the sandbox's own termination codes, `SBOX_FATAL_INTEGRITY`
/// (7006) to `SBOX_FATAL_MITIGATION` (7011) and `SBOX_FATAL_WARMUP` (7013), with which a target
/// that cannot lock itself down ends (`sandbox/win/src/sandbox_types.h`, `TerminationCodes`).
/// Not `SBOX_FATAL_MEMORY_EXCEEDED` (7012, a job memory limit) or
/// `SBOX_FATAL_BROKER_SHUTDOWN_HUNG` (7014, at shutdown), and not the loader's codes
/// (`0xC0000135` a DLL not found, `0xC0000142` a DLL's start failed, `0xC000007B` a bad image),
/// which a broken or half-unpacked Chrome gives with any sandbox: every other code is the
/// ordinary failed start.
pub(crate) fn crash(code: u32) -> bool {
    matches!(code, 0x8000_0003 | 0xC000_0005 | 7006..=7011 | 7013)
}
