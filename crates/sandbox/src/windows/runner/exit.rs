//! The runner's exit codes ([`RunExit`]), each an explicit value above [`RUN_EXIT_BASE`], and the
//! values open PRs have reserved ([`RunExit::RESERVED`]).

use super::{PASSWORD_LOST, RUN_EXIT_BASE, SECLOGON_DISABLED};

/// The runner's own failures, as its exit code ([`RUN_EXIT_BASE`] plus the variant's explicit
/// value). Never renumbered: the host reads them. A new variant takes the value its PR reserved
/// in [`RunExit::RESERVED`], and moves its row from there into the enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RunExit {
    /// The command line isn't `--launch <file>` or `--inner <base>`.
    Usage = 0x00,
    /// The launch file is missing, isn't the desktop user's alone, or breaks a rule.
    BadLaunch = 0x01,
    /// `secrets.bin` has no password for the pool user: set-up must run again.
    NoSecret = 0x02,
    /// The Secondary Logon service is disabled (1058).
    SecondaryLogon = 0x03,
    /// "Allow log on locally" doesn't include the pool user (1385), and no policy is known to
    /// leave the pool out ([`logon_refusal`](super::logon_refusal)): setting up again usually
    /// fixes it.
    LogonRight = 0x04,
    /// The pool user's password was refused (1326): set-up must run again.
    Password = 0x05,
    /// Another logon failure, or what the outer needs before its logon failed to be read: the
    /// pool user's secret, System32's path. A marker that can't be written is
    /// [`RunExit::MarkerUnwritable`].
    Logon = 0x06,
    /// M3: the launch would reuse the lawyer's `~\.codex`.
    CodexReuse = 0x07,
    /// A runner pipe couldn't be made, connected or checked.
    Pipe = 0x08,
    /// The inner runner couldn't be put in the job, or the job made.
    Job = 0x09,
    /// The inner runner's token, desktop or folders couldn't be made.
    Token = 0x0A,
    /// The agent couldn't be started.
    Spawn = 0x0B,
    /// The agent exited with a code in the runner's own range, which the outer runner never
    /// passes on as it is ([`RUN_EXIT_BASE`] to `+0xff`).
    AgentReserved = 0x0C,
    /// The inner runner's first-act sweep of the pool user's processes failed (`sweep.rs`): the
    /// process list couldn't be read, or a process of the pool user's couldn't be checked, or it
    /// kept starting processes. A process that couldn't be opened or ended is
    /// [`RunExit::Escapee`].
    Sweep = 0x0D,
    /// The runner's DLL search couldn't be kept to System32.
    DllSearch = 0x0E,
    /// A sweep found a process of the pool user's it couldn't end, even after the owner's
    /// `WRITE_DAC` reopen: one that shut its DACL to its own user, alive outside every job (M2).
    /// The inner's first act exits with it and starts no agent. The outer exits with it whenever
    /// any sweep of its session found one: the first act's, or the inner's ticks and end
    /// (reported on `ctl`). It wins over the agent's own code, even 0, which the outer then says
    /// on stderr: the run's output came through, but the pool user has a process no sweep of its
    /// own can end, and only the host can act on that (0049).
    Escapee = 0x0F,
    /// Reserved, no longer produced: a pool user's BITS jobs are cancelled by the `SYSTEM` tasks
    /// alone (`SandboxSweep` at each session's end, `SandboxStop`). Kept, with its number, since
    /// the codes are explicit and may be read downstream.
    PoolStop = 0x10,
    /// The session's unclean-session marker couldn't be written, or the launch file's name gives
    /// it no id (`marker::marked`): the outer refuses the launch before any logon, so no session
    /// runs that no marker names.
    MarkerUnwritable = 0x11,
    /// F4: a marker of this pool user's (or one that names none, or can't be read) awaits a stop
    /// that hasn't completed: a session a sweep found an escapee in, or one whose outer died
    /// (`unclean::logon_check`), or one whose sweep after that stop completed and which now
    /// awaits its final stop. The outer requested `\Clauseo\SandboxStop` and refuses the
    /// launch before its own marker or any logon; a launch once the stop completed goes ahead.
    AwaitingStop = 0x12,
    /// F4: a marker of this pool user's awaits a sweep that didn't complete within 15 s of the
    /// outer's wait, or its stop (or the sweep after it) completed but it couldn't be re-marked
    /// to await the sweep (or the final stop) (`unclean::logon_check`). No stop is awaited, so
    /// none was requested; the outer refuses the launch before its own marker or any logon, and
    /// a later launch checks again.
    AwaitingSweep = 0x13,
    /// A 1385 where a Group Policy or Intune setting defines "Allow log on locally" without the
    /// pool, as the record's `LogonPolicy` (or the cached policy read) says
    /// ([`logon_refusal`](super::logon_refusal)): setting up again doesn't fix it.
    LogonPolicy = 0x16,
}

/// [`RunExit::ALL`] and an exhaustive `match` (no `_` arm) made from one list, so a new variant
/// doesn't build until it is listed, and being listed puts it in `ALL`. A variant listed twice is
/// an unreachable pattern, which `-D warnings` refuses.
macro_rules! every_run_exit {
    ($($variant:ident),+ $(,)?) => {
        impl RunExit {
            /// Every variant, in the order of the list below.
            pub(crate) const ALL: [RunExit; [$(RunExit::$variant),+].len()] =
                [$(RunExit::$variant),+];
        }

        const _: () = {
            const fn listed(exit: RunExit) {
                match exit {
                    $(RunExit::$variant)|+ => {}
                }
            }
            listed(RunExit::Usage);
        };
    };
}

every_run_exit!(
    Usage,
    BadLaunch,
    NoSecret,
    SecondaryLogon,
    LogonRight,
    Password,
    Logon,
    CodexReuse,
    Pipe,
    Job,
    Token,
    Spawn,
    AgentReserved,
    Sweep,
    DllSearch,
    Escapee,
    PoolStop,
    MarkerUnwritable,
    AwaitingStop,
    AwaitingSweep,
    LogonPolicy,
);

impl RunExit {
    /// The values other open PRs have taken for their exits, held so no two PRs give the same
    /// code two meanings. Each later PR moves its row into the enum at its reserved value (and
    /// lists it in `every_run_exit!`, which builds [`RunExit::ALL`], and in the shipped list its
    /// tests pin); no row is ever reused.
    pub const RESERVED: &[(u32, &str)] = &[
        (0x14, "LeftoverTask #755"),
        (0x15, "TokenGrant #786"),
        (0x17, "PoolProfile #817"),
        (0x18, "retired, never shipped (was Quarantined, #819)"),
        (0x19, "GitBashUnsafe #792"),
        (0x1A, "PR6, unassigned"),
        (0x1B, "PR6, unassigned"),
        (0x1C, "PR6, unassigned"),
        (0x1D, "PR6, unassigned"),
        (0x1E, "PR6, unassigned"),
        (0x1F, "PR6, unassigned"),
    ];

    /// The exit code: [`RUN_EXIT_BASE`] plus the variant's value.
    pub fn code(self) -> u32 {
        RUN_EXIT_BASE + self as u32
    }

    /// The failure an exit code names, if it is one.
    pub fn from_code(code: u32) -> Option<RunExit> {
        RunExit::ALL.into_iter().find(|exit| exit.code() == code)
    }

    /// What tier detection makes of it: the set-up's failures, which running the set-up again or
    /// IT fixes, as `SetUpFailed` or `SetUpBlocked` with a named cause; `None` for the rest,
    /// which fail this launch only.
    pub fn no_sandbox(self) -> Option<crate::NoSandbox> {
        use crate::NoSandbox;
        match self {
            RunExit::SecondaryLogon => Some(NoSandbox::SetUpFailed(SECLOGON_DISABLED.to_owned())),
            RunExit::LogonRight => Some(NoSandbox::SetUpBlocked(crate::Blocked::LogonRight)),
            RunExit::LogonPolicy => Some(NoSandbox::SetUpBlocked(crate::Blocked::LogonPolicy)),
            RunExit::NoSecret | RunExit::Password => {
                Some(NoSandbox::SetUpFailed(PASSWORD_LOST.to_owned()))
            }
            RunExit::Usage
            | RunExit::BadLaunch
            | RunExit::Logon
            | RunExit::CodexReuse
            | RunExit::Pipe
            | RunExit::Job
            | RunExit::Token
            | RunExit::Spawn
            | RunExit::AgentReserved
            | RunExit::Sweep
            | RunExit::DllSearch
            | RunExit::Escapee
            | RunExit::PoolStop
            | RunExit::MarkerUnwritable
            | RunExit::AwaitingStop
            | RunExit::AwaitingSweep => None,
        }
    }
}

#[cfg(test)]
mod tests;
