//! The runner's own failures as exit codes ([`RunExit`]), from [`RUN_EXIT_BASE`]: the host reads
//! them, so they are appended, never renumbered, and a reserved number stays reserved.

use super::{PASSWORD_LOST, RUN_EXIT_BASE, SECLOGON_DISABLED};

/// The runner's own failures, as its exit code (`RUN_EXIT_BASE + n`). Appended, never
/// renumbered: the host reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExit {
    /// The command line isn't `--launch <file>`, `--inner <base>` or `--sweep`.
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
    /// kept starting processes. Also the end sweeper's exit code when anything it did failed but
    /// for an escapee. A process that couldn't be opened or ended is [`RunExit::Escapee`].
    Sweep,
    /// The runner's DLL search couldn't be kept to System32.
    DllSearch,
    /// A sweep found a process of the pool user's it couldn't end, even after the owner's
    /// `WRITE_DAC` reopen: one that shut its DACL to its own user, alive outside every job (M2).
    /// The inner's first act exits with it and starts no agent; the end sweeper exits with it.
    /// The outer exits with it whenever any sweep of its session found one: the first act's, the
    /// inner's ticks and end (reported on `ctl`), or the end sweeper's (its exit code, when the
    /// outer still waits for it). It wins over the agent's own code, even 0, which the outer then
    /// says on stderr: the run's output came through, but the pool user has a process no sweep of
    /// its own can end, and only the host can act on that (0049).
    Escapee,
    /// Reserved, no longer produced. The end sweeper once exited with it when everything went
    /// but BITS jobs of the pool user's it couldn't cancel (1245). It makes no BITS call now
    /// (`leftovers.rs`), and the outer takes this code as any failed sweeper's. Kept, with its
    /// number, since the codes are explicit and may be read downstream.
    PoolStop,
    /// The session's unclean-session marker, or its slot's unclean mark (`quarantine.rs`),
    /// couldn't be written, or the launch file's name gives it no id (`marker::marked`): the
    /// outer refuses the launch before any session of the pool user, so no session runs that no
    /// marker names.
    MarkerUnwritable,
    /// The inner runner couldn't grant the desktop user `TOKEN_QUERY` on its own token, its
    /// first act (`token::runner_token_first_sddl`), so the outer can't make its full check: the
    /// launch is refused before the inner does anything else.
    TokenGrant,
    /// The pool user was left marked unclean by an earlier session that didn't end clean, and the
    /// verify-only pass this launch ran before granting it anything found a process of it still
    /// left, or failed (`quarantine.rs`): the launch is refused, the mark stays, and the lawyer is
    /// told to restart the computer. Its number is reserved ([`QUARANTINED_OFFSET`]).
    Quarantined,
}

/// [`RunExit::TokenGrant`]'s reserved number past [`RUN_EXIT_BASE`]; `0x12` to `0x14` are other
/// branches' codes (`AwaitingStop`, `AwaitingSweep`, `LeftoverTask`), none here.
const TOKEN_GRANT_OFFSET: u32 = 0x15;
/// [`RunExit::Quarantined`]'s reserved number past [`RUN_EXIT_BASE`]; `0x16` and `0x17` are
/// other branches' codes, none here.
const QUARANTINED_OFFSET: u32 = 0x18;

impl RunExit {
    /// The variants numbered by position, from [`RUN_EXIT_BASE`]; [`RunExit::TokenGrant`] and
    /// [`RunExit::Quarantined`] have explicit, reserved numbers instead ([`TOKEN_GRANT_OFFSET`],
    /// [`QUARANTINED_OFFSET`]).
    pub(super) const ALL: [RunExit; 18] = [
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
    ];

    /// The exit code.
    pub fn code(self) -> u32 {
        if self == RunExit::TokenGrant {
            return RUN_EXIT_BASE + TOKEN_GRANT_OFFSET;
        }
        if self == RunExit::Quarantined {
            return RUN_EXIT_BASE + QUARANTINED_OFFSET;
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
        match offset {
            TOKEN_GRANT_OFFSET => return Some(RunExit::TokenGrant),
            QUARANTINED_OFFSET => return Some(RunExit::Quarantined),
            _ => {}
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
            | RunExit::TokenGrant
            | RunExit::Quarantined => None,
        }
    }
}
