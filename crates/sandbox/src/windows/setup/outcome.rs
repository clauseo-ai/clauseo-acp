//! What a set-up gives back: its report ([`SetupReport`]), where and why it failed
//! ([`SetupFailure`], [`Step`]), the setup exe's exit codes ([`SetupExit`], D-B4), and the named
//! causes firm IT can act on.

use std::fmt;
use std::path::PathBuf;

use clauseo_protocol::MatterId;

use super::machine::MachineError;
use crate::windows::apply::Applied;
use crate::windows::pool::PoolUser;
use crate::windows::shared_writes::SharedWrites;
use crate::windows::sids::Sid;
use crate::windows::wfp::NetworkReport;
use crate::{Blocked, SetupError};

/// `ERROR_VIRUS_INFECTED`: Windows security blocked the setup exe as infected.
pub const ERROR_VIRUS_INFECTED: u32 = 225;
/// `ERROR_VIRUS_DELETED`: Windows security blocked the setup exe and may have deleted it.
pub const ERROR_VIRUS_DELETED: u32 = 226;
/// `ERROR_CANCELLED`: the prompt was declined.
pub const ERROR_CANCELLED: u32 = 1223;
/// `ERROR_ACCESS_DISABLED_BY_POLICY`: a policy blocks the setup exe.
pub const ERROR_ACCESS_DISABLED_BY_POLICY: u32 = 1260;
/// `ERROR_LOGON_TYPE_NOT_GRANTED`: the logon right isn't granted to the pool user.
pub const ERROR_LOGON_TYPE_NOT_GRANTED: u32 = 1385;

/// The cause when a pool user's logon gets error 1385 (D-B4).
pub const LOGON_NOT_GRANTED: &str = "Windows' \"Allow log on locally\" setting doesn't include \
     the sandbox users (error 1385): IT can add the Users group to it";
/// The cause when `ShellExecuteExW` itself gets error 1385: Windows refused the sign-in of the
/// account that approved the prompt, before the set-up program ran.
pub const APPROVER_LOGON_REFUSED: &str = "Windows refused the sign-in of the account that \
     approved the set-up (error 1385): ask whoever looks after this computer";
/// The cause when the data folder isn't owned by the user who asked
/// ([`SetupExit::DataDirNotOwned`]).
pub const DATA_DIR_NOT_OWNED: &str = "this computer's Clauseo data folder isn't owned by you: \
     ask whoever looks after this computer";
/// The cause when the Secondary Logon service is disabled.
pub const SECLOGON_DISABLED: &str = "the Secondary Logon service (seclogon) is disabled: IT can \
     set it to start manually";
/// The cause when local policy refuses the interactive logon right to `Users`.
pub const LOGON_REFUSED: &str = "Windows' \"Deny log on locally\" setting includes the Users \
     group, so the sandbox users can't sign in: IT can take Users out of it";

/// The cause of a run stopped by its deadline ([`SetupExit::DeadlineReached`]), at the step it
/// was about to take.
pub(super) const DEADLINE_REACHED: &str = "the set-up ran out of the time it was given, so it \
     stopped before this step and undid what it had made: run it again";

/// The setup exe's exit codes (D-B4). Codes are appended from here on, never renumbered: the
/// uninstall hook and the app read them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupExit {
    /// 0: done (for `--remove`, everything removed; the hook reads only this one).
    Done,
    /// 1: the arguments are wrong.
    Usage,
    /// 2: another Windows user set this computer up (D5).
    OtherOwner,
    /// 3: the set-up failed; the cause is in `setup-result.json`.
    Failed,
    /// 4: `--remove` removed only part of the set-up: the record stays, with `Removing` set and
    /// every pool SID, and so does the ledger, for the next run to finish.
    RemovedPartly,
    /// 5: another set-up holds the machine-wide lock (for `--remove`, still after its 60 s
    /// wait), and nothing was touched.
    LockHeld,
    /// 6: the data folder isn't owned by the user who asked (one an elevated run made, owned
    /// by Administrators, for one); no result file is written there.
    DataDirNotOwned,
    /// 7: `--adopt` ran without an elevated token (per user), or without an elevated or SYSTEM
    /// one (`--per-machine`); nothing was touched.
    NotElevated,
    /// 8: `--adopt` ran from anywhere but the admin copy's own path
    /// (`%ProgramFiles%\Clauseo\Sandbox\clauseo-sandbox-setup.exe`); nothing was touched.
    NotAdminCopy,
    /// 9: `--adopt` found a newer admin copy recorded (`AdminCopyVersion`) than its own
    /// version, and left it as it was.
    Downgrade,
    /// 10: `--remove --uninstall` removed the protection, but the admin copy, or part of it
    /// (its values, a task, its folder), stays; the step and code are in the Event Log. The
    /// uninstall hook counts it as removed, as only administrators can change what is left.
    AdminCopyLeft,
    /// 11: the run reached the deadline its `--deadline` set (`deadline.rs`). A set-up stopped
    /// before its next change and rolled back what it had made, or was ended at the deadline
    /// itself, leaving what its intent and record name for the next run (D-B7); `--remove`
    /// stopped between two steps with `Removing` set, for the next run to finish; `--adopt`
    /// stopped between two of its steps, which running it again repairs.
    DeadlineReached,
}

impl SetupExit {
    /// 0 for `--remove`: everything it set up is gone, or there was nothing of the asker's to
    /// remove (no record; a per-user uninstall by someone who isn't the owner, D-B9). The one
    /// code the uninstall hook reads as success (`$R8 = 0`). The same code as
    /// [`SetupExit::Done`], so no number is used twice for two meanings.
    #[expect(
        non_upper_case_globals,
        reason = "read as a variant beside RemovedPartly, as the hook's test names it"
    )]
    pub const Removed: SetupExit = SetupExit::Done;

    /// Every exit, in code order.
    pub const ALL: [SetupExit; 12] = [
        SetupExit::Done,
        SetupExit::Usage,
        SetupExit::OtherOwner,
        SetupExit::Failed,
        SetupExit::RemovedPartly,
        SetupExit::LockHeld,
        SetupExit::DataDirNotOwned,
        SetupExit::NotElevated,
        SetupExit::NotAdminCopy,
        SetupExit::Downgrade,
        SetupExit::AdminCopyLeft,
        SetupExit::DeadlineReached,
    ];

    /// The process exit code.
    pub fn as_code(self) -> u8 {
        match self {
            SetupExit::Done => 0,
            SetupExit::Usage => 1,
            SetupExit::OtherOwner => 2,
            SetupExit::Failed => 3,
            SetupExit::RemovedPartly => 4,
            SetupExit::LockHeld => 5,
            SetupExit::DataDirNotOwned => 6,
            SetupExit::NotElevated => 7,
            SetupExit::NotAdminCopy => 8,
            SetupExit::Downgrade => 9,
            SetupExit::AdminCopyLeft => 10,
            SetupExit::DeadlineReached => 11,
        }
    }

    /// The exit an exit code means, as `GetExitCodeProcess` gives it; `None` for any other
    /// code (a crash, or a code from a newer setup exe).
    pub fn from_code(code: u32) -> Option<SetupExit> {
        SetupExit::ALL
            .into_iter()
            .find(|exit| u32::from(exit.as_code()) == code)
    }
}

/// What a launch error from `ShellExecuteExW` means for the set-up (D-B4): 1223 (the prompt
/// declined) is [`SetupError::Refused`]; 1260 is [`Blocked::Policy`], named for IT; 225 and 226
/// are [`Blocked::ToolMissing`], Windows security refusing or removing the admin copy as it
/// starts (a copy missing before the start is `AdminCopyMissing`, from the check). A 1385 here
/// is the approving account's own sign-in, refused before the set-up program ran, so it is a
/// failure saying so ([`APPROVER_LOGON_REFUSED`]); [`Blocked::LogonRight`] is the exe's 1385 only,
/// from a sandbox user's logon. Any other code is a failure naming it.
pub fn launch_error(code: u32) -> SetupError {
    match code {
        ERROR_CANCELLED => SetupError::Refused,
        ERROR_ACCESS_DISABLED_BY_POLICY => SetupError::Blocked(Blocked::Policy),
        ERROR_VIRUS_INFECTED | ERROR_VIRUS_DELETED => SetupError::Blocked(Blocked::ToolMissing),
        ERROR_LOGON_TYPE_NOT_GRANTED => SetupError::Failed(APPROVER_LOGON_REFUSED.to_owned()),
        other => SetupError::Failed(format!(
            "Clauseo's set-up program didn't start (error {other})"
        )),
    }
}

/// The Win32 code in an `HRESULT` from `ShellExecuteExW`, for [`launch_error`]: the low 16 bits
/// of a `FACILITY_WIN32` failure (`0x8007xxxx`), or a plain Win32 code (0 to `0xFFFF`) as it
/// is. Any other `HRESULT` (`E_NOINTERFACE`, `E_POINTER`) is kept whole, so it never reads as
/// a small Win32 code such as 225.
pub fn launch_code(hresult: i32) -> u32 {
    let bits = hresult.cast_unsigned();
    if bits & 0xFFFF_0000 == 0x8007_0000 || bits <= 0xFFFF {
        bits & 0xFFFF
    } else {
        bits
    }
}

/// The step a set-up failed at. Variants are appended from here on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// 1: taking the machine-wide lock.
    Lock,
    /// 1: reading the machine's record.
    ReadRecord,
    /// 1: the record names another owner (D5), or names the owner as a pool user.
    Owner,
    /// 2: the Secondary Logon service.
    SecondaryLogon,
    /// 2: the interactive logon right.
    LogonRight,
    /// Reading the pool: its accounts, the marker, the ledger; or the pool is at its cap.
    Pool,
    /// A re-run would reset passwords while a session is open (D-B6).
    Sessions,
    /// 3: the group.
    Group,
    /// 4: making an account.
    CreateUser,
    /// 4: a recorded account's new password (a re-run).
    ResetPassword,
    /// 4: putting an account in the group.
    AddToGroup,
    /// 4: hiding an account from the sign-in screen.
    Hide,
    /// 5: writing the machine's record.
    WriteRecord,
    /// 6: the logon that makes the profile.
    LogOn,
    /// 6: the profile is temporary or missing.
    Profile,
    /// 7: the shared-write denies.
    SharedWrites,
    /// 8: fencing the registered matters against the grown pool.
    Fence,
    /// 10: `secrets.bin`.
    Secrets,
    /// 11: `setup.json`.
    Marker,
    /// 1: reading the registered matters (`matters.json`, the ledger) after the lock.
    Matters,
    /// 7: before the shared-write denies, no sandbox user may own a folder they protect.
    Owners,
    /// 4: deleting the profile of a recorded user released from a matter, before its logon
    /// makes it again (`resets.rs`).
    ResetProfile,
    /// 8a: the network filters, the owner's grants on them and the firewall rules (S15 PR7).
    Network,
    /// 5: on a per-machine install, registering the SYSTEM tasks `\Clauseo\SandboxStop` and
    /// `\Clauseo\SandboxSweep` for the owner just recorded (`late_steps::stop_task`).
    StopTask,
    /// 3: denying the group "log on through Remote Desktop Services"
    /// (`SeDenyRemoteInteractiveLogonRight`).
    RemoteLogon,
    /// 5a: once the record names the group, locking the admin copy's folder and file again with
    /// the group denied (`late_steps::admin_lock`), after step 5.
    AdminLock,
    /// 1: finishing what an interrupted run left: the accounts and the group its intent names
    /// that no record does, and a record it left partial (`intent.rs`).
    Pending,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Step::Lock => "taking the set-up lock",
            Step::ReadRecord => "reading the machine's record",
            Step::Owner => "checking who owns the set-up",
            Step::SecondaryLogon => "checking the Secondary Logon service",
            Step::LogonRight => "checking the logon right",
            Step::Pool => "reading the pool",
            Step::Sessions => "checking for open sessions",
            Step::Group => "the sandbox group",
            Step::CreateUser => "making a sandbox user",
            Step::ResetPassword => "resetting a sandbox user's password",
            Step::AddToGroup => "adding a sandbox user to its groups",
            Step::Hide => "hiding a sandbox user from the sign-in screen",
            Step::WriteRecord => "writing the machine's record",
            Step::LogOn => "logging a sandbox user on",
            Step::Profile => "a sandbox user's profile",
            Step::SharedWrites => "closing the shared folders",
            Step::Fence => "fencing the matters",
            Step::Secrets => "saving the sandbox users' passwords",
            Step::Marker => "writing setup.json",
            Step::Matters => "reading the registered matters",
            Step::Owners => "making sure no sandbox user owns a protected folder",
            Step::ResetProfile => "resetting a sandbox user's profile",
            Step::Network => "setting up the network filters",
            Step::StopTask => "registering the sandbox's stop and sweep tasks",
            Step::RemoteLogon => "denying the sandbox users Remote Desktop logon",
            Step::AdminLock => "locking the set-up program's admin copy",
            Step::Pending => "finishing an interrupted set-up",
        })
    }
}

/// Why a set-up failed, and whether what it made is gone again.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("sandbox set-up failed {step}: {cause}")]
pub struct SetupFailure {
    /// Where it failed.
    pub step: Step,
    /// Why, in words firm IT can act on where there are such words.
    pub cause: String,
    /// Everything this run made is gone again (nothing was made, or the rollback finished).
    /// `false`: the cause ends with what is left.
    pub rolled_back: bool,
    /// The exit code the setup exe gives for it.
    pub exit: SetupExit,
}

impl SetupFailure {
    /// The setting that blocked this failure, when one did (D-B4): a logon refused with 1385
    /// ([`LOGON_NOT_GRANTED`], the cause [`cause_of`] gives it). `None` for any other.
    pub fn blocked(&self) -> Option<Blocked> {
        self.cause
            .starts_with(LOGON_NOT_GRANTED)
            .then_some(Blocked::LogonRight)
    }

    /// A failure at `step` before anything was made.
    pub(super) fn early(step: Step, cause: impl Into<String>) -> SetupFailure {
        let exit = if step == Step::Owner {
            SetupExit::OtherOwner
        } else {
            SetupExit::Failed
        };
        SetupFailure {
            step,
            cause: cause.into(),
            rolled_back: true,
            exit,
        }
    }
}

/// The cause for a [`Machine`](super::Machine) error: 1385 from anywhere is
/// [`LOGON_NOT_GRANTED`], anything else the error's own words.
pub(super) fn cause_of(error: &MachineError) -> String {
    if let MachineError::Os {
        code: ERROR_LOGON_TYPE_NOT_GRANTED,
        ..
    } = error
    {
        LOGON_NOT_GRANTED.to_owned()
    } else {
        error.to_string()
    }
}

/// What happened to one volume's change journal (step 9). A journal that can't be enlarged only
/// costs full walks later (0049), so it never fails the set-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalOutcome {
    /// Large enough already; its size.
    Left(u64),
    /// Created or enlarged to the set-up's size.
    Grown,
    /// The query or the enlargement failed; why.
    Failed(String),
}

/// What a set-up did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupReport {
    /// Every pool user now, in record order.
    pub pool: Vec<PoolUser>,
    /// The users this run made.
    pub made: Vec<PoolUser>,
    /// The recorded users whose passwords this run reset (a re-run, or a profile reset).
    pub reset: Vec<PoolUser>,
    /// The recorded users whose profiles this run deleted and made again, as released users owe
    /// (`resets.rs`); `Err` when their marks couldn't be cleared after, and why: they stay held
    /// back until a set-up clears them.
    pub profiles_reset: Result<Vec<PoolUser>, String>,
    /// The group's SID.
    pub group: Sid,
    /// Whether this run made the group.
    pub group_made: bool,
    /// Recorded SIDs whose accounts are gone, dropped from the record: no ledger entry names
    /// them.
    pub dropped: Vec<Sid>,
    /// Recorded SIDs whose accounts are gone, kept in the record while a ledger entry names
    /// them, so their entries stay trusted for removal (D-B6).
    pub kept_dead: Vec<Sid>,
    /// Pool names skipped because an account by that name exists outside the record (D-B5),
    /// with its SID.
    pub skipped_names: Vec<(String, Sid)>,
    /// The shared-write denies.
    pub shared: SharedWrites,
    /// The fence of the registered matters, when this run made users and there are any.
    pub fenced: Option<Applied>,
    /// The registered matters step 8 left unfenced, and why: a sandbox user owns the matter's
    /// root and it couldn't be handed to the owner (S15 `PR4h`). The rest are fenced.
    pub fence_skipped: Vec<(MatterId, String)>,
    /// Each volume's change journal; `Err` when the volumes couldn't be listed, and why.
    pub journals: Result<Vec<(PathBuf, JournalOutcome)>, String>,
    /// What the run did to leave [`MIN_FREE_AFTER_RUN`](super::MIN_FREE_AFTER_RUN) free users
    /// (`top_up.rs`).
    pub top_up: TopUp,
    /// The network filters and the pool users they cover, and whether the firewall rules take
    /// effect: under Group Policy they may not, which leaves WFP's filters enforcing alone
    /// ([`FirewallOutcome::Ineffective`](crate::windows::wfp::FirewallOutcome::Ineffective)).
    pub network: NetworkReport,
}

/// What a run did to leave at least [`MIN_FREE_AFTER_RUN`](super::MIN_FREE_AFTER_RUN) free pool
/// users (`top_up.rs`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TopUp {
    /// The recorded users counted free before this run made any: their accounts exist and the
    /// reuse rule (`pool::assign`) would hand them out once this run's resets are done.
    pub free_before: usize,
    /// The users made for the rule, beyond those the set-up, the re-run or the requested `add`
    /// makes; they are in [`SetupReport::made`] too.
    pub added: usize,
    /// How many more the rule wanted that the cap ([`POOL_CAP`](crate::windows::pool::POOL_CAP)
    /// SIDs in the record) left out; 0 when none. Not a failure.
    pub short: usize,
}
