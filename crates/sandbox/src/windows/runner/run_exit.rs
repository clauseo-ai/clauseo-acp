//! The runner's exit codes ([`RunExit`]) and the agent's code as the runner passes it on.

use super::{PASSWORD_LOST, RUN_EXIT_BASE, SECLOGON_DISABLED};

/// The runner's own failures, as its exit code (`RUN_EXIT_BASE + n`). Appended, never
/// renumbered: the host reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExit {
    /// The command line isn't `--launch <file>` or `--inner <base>`.
    Usage,
    /// The launch file is missing, isn't the desktop user's alone, or breaks a rule.
    BadLaunch,
    /// `secrets.bin` has no password for the pool user: set-up must run again.
    NoSecret,
    /// The Secondary Logon service is disabled (1058).
    SecondaryLogon,
    /// "Allow log on locally" doesn't include the pool user (1385).
    LogonRight,
    /// The pool user's password was refused (1326): set-up must run again.
    Password,
    /// Another logon failure, or what the outer needs before its logon failed to be read: the
    /// pool user's secret, System32's path. A marker that can't be written is
    /// [`RunExit::MarkerUnwritable`].
    Logon,
    /// M3: the launch would reuse the lawyer's `~\.codex`.
    CodexReuse,
    /// A runner pipe couldn't be made, connected or checked.
    Pipe,
    /// The inner runner couldn't be put in the job, or the job made.
    Job,
    /// The inner runner's token, desktop or folders couldn't be made, or the outer's window
    /// station and desktop for the agent (`desktop.rs`): any of their calls but a refused
    /// `LogonUserW` password or right, which are [`RunExit::Password`] and
    /// [`RunExit::LogonRight`] as the inner's logon's are.
    Token,
    /// The agent couldn't be started.
    Spawn,
    /// The agent exited with a code in the runner's own range, which the outer runner never
    /// passes on as it is ([`RUN_EXIT_BASE`] to `+0xff`).
    AgentReserved,
    /// The inner runner's first-act sweep of the pool user's processes failed (`sweep.rs`): the
    /// process list couldn't be read, or a process of the pool user's couldn't be checked, or it
    /// kept starting processes. A process that couldn't be opened or ended is
    /// [`RunExit::Escapee`].
    Sweep,
    /// The runner's DLL search couldn't be kept to System32.
    DllSearch,
    /// A sweep found a process of the pool user's it couldn't end, even after the owner's
    /// `WRITE_DAC` reopen: one that shut its DACL to its own user, alive outside every job (M2).
    /// The inner's first act exits with it and starts no agent. The outer exits with it whenever
    /// any sweep of its session found one: the first act's, or the inner's ticks and end
    /// (reported on `ctl`). It wins over the agent's own code, even 0, which the outer then says
    /// on stderr: the run's output came through, but the pool user has a process no sweep of its
    /// own can end, and only the host can act on that (0049).
    Escapee,
    /// Reserved, no longer produced: a pool user's BITS jobs are cancelled by the `SYSTEM` tasks
    /// alone (`SandboxSweep` at each session's end, `SandboxStop`). Kept, with its number, since
    /// the codes are explicit and may be read downstream.
    PoolStop,
    /// The session's unclean-session marker couldn't be written, or the launch file's name gives
    /// it no id (`marker::marked`): the outer refuses the launch before any logon, so no session
    /// runs that no marker names.
    MarkerUnwritable,
    /// F4: a marker of this pool user's (or one that names none, or can't be read) awaits a stop
    /// that hasn't completed: a session a sweep found an escapee in, or one whose outer died
    /// (`unclean::logon_check`), or one whose sweep after that stop completed and which now
    /// awaits its final stop. The outer requested `\Clauseo\SandboxStop` and refuses the
    /// launch before its own marker or any logon; a launch once the stop completed goes ahead.
    AwaitingStop,
    /// F4: a marker of this pool user's awaits a sweep that didn't complete within 15 s of the
    /// outer's wait, or its stop (or the sweep after it) completed but it couldn't be re-marked
    /// to await the sweep (or the final stop) (`unclean::logon_check`). No stop is awaited, so
    /// none was requested; the outer refuses the launch before its own marker or any logon, and
    /// a later launch checks again.
    AwaitingSweep,
    /// The inner runner couldn't grant the desktop user `TOKEN_QUERY` on its own token, its
    /// first act (`token::runner_token_first_sddl`), so the outer can't make its full check: the
    /// launch is refused before the inner does anything else.
    TokenGrant,
    /// F4: an open marker of this pool user's names an outer that still runs, a live session's
    /// (`unclean::outer_alive`). The outer refuses the launch before its own marker or any
    /// logon, requests no stop and ends nothing; a launch once that session ended goes ahead.
    PoolBusy,
}

/// The variants with an explicit, reserved number past [`RUN_EXIT_BASE`] instead of their
/// position: [`RunExit::TokenGrant`] `0x15` and [`RunExit::PoolBusy`] `0x20`. The numbers
/// between are other branches' codes (`0x14` `LeftoverTask`, `0x16` to `0x1f`), none here.
const RESERVED: [(RunExit, u32); 2] = [(RunExit::TokenGrant, 0x15), (RunExit::PoolBusy, 0x20)];

impl RunExit {
    /// The variants numbered by position, from [`RUN_EXIT_BASE`]; the rest have an explicit,
    /// reserved number instead ([`RESERVED`]).
    const ALL: [RunExit; 20] = [
        RunExit::Usage,
        RunExit::BadLaunch,
        RunExit::NoSecret,
        RunExit::SecondaryLogon,
        RunExit::LogonRight,
        RunExit::Password,
        RunExit::Logon,
        RunExit::CodexReuse,
        RunExit::Pipe,
        RunExit::Job,
        RunExit::Token,
        RunExit::Spawn,
        RunExit::AgentReserved,
        RunExit::Sweep,
        RunExit::DllSearch,
        RunExit::Escapee,
        RunExit::PoolStop,
        RunExit::MarkerUnwritable,
        RunExit::AwaitingStop,
        RunExit::AwaitingSweep,
    ];

    /// The exit code.
    pub fn code(self) -> u32 {
        if let Some((_, offset)) = RESERVED.iter().find(|(exit, _)| *exit == self) {
            return RUN_EXIT_BASE + offset;
        }
        let index = RunExit::ALL
            .iter()
            .position(|exit| *exit == self)
            .unwrap_or(0);
        RUN_EXIT_BASE + u32::try_from(index).unwrap_or(0)
    }

    /// The failure an exit code names, if it is one.
    pub fn from_code(code: u32) -> Option<RunExit> {
        let offset = code.checked_sub(RUN_EXIT_BASE)?;
        if let Some((exit, _)) = RESERVED.iter().find(|(_, reserved)| *reserved == offset) {
            return Some(*exit);
        }
        RunExit::ALL.get(usize::try_from(offset).ok()?).copied()
    }

    /// What tier detection makes of it: the set-up's failures, which running the set-up again or
    /// IT fixes, as `SetUpFailed` or `SetUpBlocked` with a named cause; `None` for the rest,
    /// which fail this launch only.
    pub fn no_sandbox(self) -> Option<crate::NoSandbox> {
        use crate::NoSandbox;
        match self {
            RunExit::SecondaryLogon => Some(NoSandbox::SetUpFailed(SECLOGON_DISABLED.to_owned())),
            RunExit::LogonRight => Some(NoSandbox::SetUpBlocked(crate::Blocked::LogonRight)),
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
            | RunExit::AwaitingSweep
            | RunExit::TokenGrant
            | RunExit::PoolBusy => None,
        }
    }
}

/// The agent's exit code as the runner passes it on: one in the runner's own range
/// (`RUN_EXIT_BASE` to `+0xff`) becomes [`RunExit::AgentReserved`], so an agent can never pass
/// for a runner failure (a set-up one among them) in the host's eyes.
pub fn agent_code(code: u32) -> u32 {
    if (RUN_EXIT_BASE..=RUN_EXIT_BASE + 0xff).contains(&code) {
        RunExit::AgentReserved.code()
    } else {
        code
    }
}

#[cfg(test)]
mod tests;
