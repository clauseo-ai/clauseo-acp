//! The refresh's fixed values and pure rules: the tasks' arguments and limits, the order the
//! uninstaller's removal takes them down in, the descriptors of the tasks and the staging folder
//! (the admin copy's is `admin_copy::LOCKED_FILE_SDDL`, which `--adopt` gives the first copy),
//! and the comparisons of paths, descriptors and the install folder against the owner's
//! profile. No Windows call; tested on every OS.

use std::path::Path;

use crate::windows::sids::Sid;

/// The task's one argument: the action is the admin copy's path with this, nothing else.
pub const REFRESH_ARG: &str = "--refresh";
/// The task's execution limit: 5 minutes.
pub const EXECUTION_LIMIT: &str = "PT5M";
/// The stop task's one argument (`setup::stop`): nothing else.
pub const STOP_ARG: &str = "--stop";
/// The stop task's execution limit: 2 minutes.
pub const STOP_EXECUTION_LIMIT: &str = "PT2M";
/// The set-up task's one argument (`setup::serve`): nothing else.
pub const SET_UP_ARG: &str = "--serve-set-up";
/// The set-up task's execution limit: 30 minutes, for a set-up (users, logons, filters) and
/// the requests queued behind it.
pub const SET_UP_EXECUTION_LIMIT: &str = "PT30M";

/// The SYSTEM tasks `--adopt` registers in `\Clauseo`, each running the admin copy with one
/// fixed argument. They share the principal (LOCAL SYSTEM) and the settings `task.rs` gives
/// every task; they differ only in what [`SystemTask`]'s methods say and in who may start them
/// ([`Starters`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTask {
    /// `\Clauseo\SandboxRefresh`: the admin copy's refresh (`--refresh`).
    Refresh,
    /// `\Clauseo\SandboxStop`: stops every process of the pool's users (`--stop`).
    Stop,
    /// `\Clauseo\SandboxSetUp`: the set-up a standard user asks for over its pipe
    /// (`--serve-set-up`, `setup::serve`), with no prompt.
    SetUp,
}

/// The tasks a per-user `--adopt` registers for the lawyer, in this order (`--place-only`
/// registers [`SystemTask::SetUp`] alone; a per-machine `--adopt` too, for interactive users).
pub const TASKS: [SystemTask; 2] = [SystemTask::Refresh, SystemTask::Stop];

impl SystemTask {
    /// Its name in `\Clauseo`.
    pub fn name(self) -> &'static str {
        match self {
            SystemTask::Refresh => "SandboxRefresh",
            SystemTask::Stop => "SandboxStop",
            SystemTask::SetUp => "SandboxSetUp",
        }
    }

    /// Its action's one argument.
    pub fn argument(self) -> &'static str {
        match self {
            SystemTask::Refresh => REFRESH_ARG,
            SystemTask::Stop => STOP_ARG,
            SystemTask::SetUp => SET_UP_ARG,
        }
    }

    /// How long a run may take before the Task Scheduler ends it.
    pub fn execution_limit(self) -> &'static str {
        match self {
            SystemTask::Refresh => EXECUTION_LIMIT,
            SystemTask::Stop => STOP_EXECUTION_LIMIT,
            SystemTask::SetUp => SET_UP_EXECUTION_LIMIT,
        }
    }

    /// Whether it also starts at the record owner's logon; otherwise only on demand.
    pub fn at_logon(self) -> bool {
        match self {
            SystemTask::Refresh => true,
            SystemTask::Stop | SystemTask::SetUp => false,
        }
    }

    /// Whether a start while it runs queues another run, which starts when this one ends
    /// (`TASK_INSTANCES_QUEUE`); otherwise it is ignored (`TASK_INSTANCES_IGNORE_NEW`). The
    /// set-up task queues, so a request made as its server is about to stop still gets one.
    pub fn queues(self) -> bool {
        match self {
            SystemTask::Refresh | SystemTask::Stop => false,
            SystemTask::SetUp => true,
        }
    }

    /// Its description, as the Task Scheduler shows it.
    pub fn description(self) -> &'static str {
        match self {
            SystemTask::Refresh => {
                "Keeps the admin copy of Clauseo's sandbox set-up program current."
            }
            SystemTask::Stop => "Stops the processes of Clauseo's sandbox users.",
            SystemTask::SetUp => "Sets up Clauseo's sandbox for the user who asks, with no prompt.",
        }
    }
}

/// One step of the uninstaller's removal of the admin copy (`setup::admin_remove`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Teardown {
    /// The record's admin-copy values, first, so no launcher trusts the copy from here on.
    AdminValues,
    /// One of the SYSTEM tasks (one that isn't there is fine).
    Task(SystemTask),
    /// The refresh's staging folder.
    Staging,
    /// Everything left in the admin folder, then the folder.
    AdminFolder,
}

/// The removal's steps, in order: every task goes before the copy it runs, the set-up task
/// first, so no set-up starts while the rest goes.
pub const TEARDOWN: [Teardown; 6] = [
    Teardown::AdminValues,
    Teardown::Task(SystemTask::SetUp),
    Teardown::Task(SystemTask::Refresh),
    Teardown::Task(SystemTask::Stop),
    Teardown::Staging,
    Teardown::AdminFolder,
];

/// The descriptor of every task in [`TASKS`] (and of the set-up task, per user): owned by
/// Administrators; protected; SYSTEM and Administrators full control; the record owner `owner`
/// read and execute (`FRFX`). Starting a task needs execute access: as the owner,
/// `schtasks /run` with `FR` alone was refused (CI run 42416074600). Execute also ends a running
/// task, so the owner can end a stop or a refresh: denial of service only, which code running as
/// the owner could do to its own sessions anyway. An ended stop never writes `StopCompleted`
/// (`setup::stop`), and an ended refresh keeps the old copy. The owner can't change or delete a
/// task.
pub fn task_sddl(owner: &Sid) -> String {
    format!("O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;{owner})")
}

/// The folder `\Clauseo`'s own descriptor, whatever task is registered in it
/// (`task_folder::prepare`): owned by Administrators; protected; SYSTEM and Administrators full
/// control; interactive users (`IU`) read, so a user a task's descriptor names can reach it. No
/// one else may add, change or delete anything in it.
pub const FOLDER_SDDL: &str = "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;IU)";

/// Who may start a task: what its descriptor grants read and execute (`FRFX`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Starters {
    /// The record owner alone ([`task_sddl`]): the refresh and stop tasks, and the set-up task
    /// of a per-user install.
    Owner(Sid),
    /// Every interactive user (`IU`), but members of `denied`, the pool's group once it exists:
    /// a per-machine install's set-up task. Pool users log on interactively, so they hold `IU`
    /// too; the deny comes first and keeps them out.
    Interactive {
        /// `GroupSid`, when the record has one.
        denied: Option<Sid>,
    },
}

impl Starters {
    /// The task's descriptor: [`task_sddl`] for an owner; for interactive users, owned by
    /// Administrators, protected, `denied` refused read and execute first, SYSTEM and
    /// Administrators full control, `IU` read and execute.
    pub fn sddl(&self) -> String {
        match self {
            Starters::Owner(owner) => task_sddl(owner),
            Starters::Interactive { denied } => {
                let deny = denied
                    .as_ref()
                    .map(|group| format!("(D;;FRFX;;;{group})"))
                    .unwrap_or_default();
                format!("O:BAD:P{deny}(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;IU)")
            }
        }
    }

    /// The user whose logon starts a task with a logon trigger: the owner; `None` for
    /// interactive users, whose task has no such trigger.
    pub fn owner(&self) -> Option<&Sid> {
        match self {
            Starters::Owner(owner) => Some(owner),
            Starters::Interactive { .. } => None,
        }
    }
}

/// The task action's program: `exe`'s path in double quotes, as a command line takes a path
/// with spaces (`C:\Program Files\...`), so no prefix of it can be read as the program.
pub fn action_path(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// The staging folder's owner and DACL: SYSTEM and Administrators, full control, inherited by
/// what is made inside; protected. No one else: not the lawyer, not a pool user.
///
/// Why Administrators don't weaken #668's "verified as read back, no TOCTOU": the folder is shut
/// to the lawyer and the pool users (who aren't administrators) so that none of them can swap
/// the staged exe between its signature check and the replace. Administrators are already inside
/// that boundary: they can write `Program Files\Clauseo\Sandbox` and the admin copy directly,
/// and change or replace the SYSTEM tasks. Granting them the staging folder adds no capability
/// they lack. Without it, an elevated `--adopt` (a repair or reinstall) and `--remove` were
/// denied a staging folder a refresh had made, and failed (the VM reinstall finding, 0049).
pub const STAGING_SDDL: &str = "O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// Whether `found`, an owner and DACL read back as SDDL, is `expected`: the same text but for
/// the DACL's `AI` and `AR` flags, which Windows adds when it sets a descriptor through
/// `SetSecurityInfo` and which grant nothing. A string compare alone: on Windows, `expected`
/// first goes through `staging::as_read_back`, so both write a SID the same way (`LA`, not the
/// built-in Administrator's `S-1-5-21-...-500`).
pub fn sddl_matches(found: &str, expected: &str) -> bool {
    let plain = |sddl: &str| match sddl.split_once("D:") {
        Some((owner, dacl)) => {
            let flags = dacl.find('(').unwrap_or(dacl.len());
            let (flags, aces) = dacl.split_at(flags);
            let protected = if flags.contains('P') { "P" } else { "" };
            format!("{owner}D:{protected}{aces}")
        }
        None => sddl.to_owned(),
    };
    plain(found) == plain(expected)
}

/// Whether `a` and `b` name the same path, as Windows compares them: case folded, with any
/// `\\?\` prefix and either separator.
pub fn same_path(a: &Path, b: &Path) -> bool {
    let fold = |path: &Path| {
        let text = path.to_string_lossy();
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
        text.replace('/', "\\").to_lowercase()
    };
    fold(a) == fold(b)
}

/// `bytes` as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _infallible = write!(text, "{byte:02x}");
        text
    })
}

/// Whether `dir` lies strictly inside `profile` (the owner's `ProfileImagePath`), compared part
/// by part as Windows compares names: case folded, any `\\?\` prefix and either separator, a
/// trailing separator ignored. The walk opens every part of `dir` without following a reparse
/// point, so its text is where it leads.
pub fn inside_profile(dir: &Path, profile: &Path) -> bool {
    let parts = |path: &Path| -> Vec<String> {
        let text = path.to_string_lossy();
        let text = text
            .strip_prefix(r"\\?\")
            .unwrap_or(&text)
            .replace('/', "\\");
        text.split('\\')
            .filter(|part| !part.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let (dir, profile) = (parts(dir), parts(profile));
    let drive = profile
        .first()
        .is_some_and(|first| first.len() == 2 && first.ends_with(':'));
    drive && dir.len() > profile.len() && dir.starts_with(&profile)
}
