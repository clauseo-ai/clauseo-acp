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
/// The set-up task's execution limit: two hours (`serve::RUN_LIMIT`), the backstop for a run
/// that hangs. The server takes a client only while 30 minutes are left for its set-up
/// (`serve::CLIENT_LIMIT`); the app waits a minute past the limit (`serve::REPLY_WAIT`).
pub const SET_UP_EXECUTION_LIMIT: &str = "PT2H";
/// `TASK_INSTANCES_QUEUE`: a start while it runs queues another run.
pub const INSTANCES_QUEUE: i32 = 1;
/// `TASK_INSTANCES_IGNORE_NEW`: a start while it runs is dropped.
pub const INSTANCES_IGNORE_NEW: i32 = 2;

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
    /// (`TASK_INSTANCES_QUEUE`); otherwise it is ignored (`TASK_INSTANCES_IGNORE_NEW`). None
    /// queues. The Task Scheduler has no limit on a queue, so any interactive user could pile
    /// up set-up runs; a start during a set-up run is dropped instead, as the running server
    /// serves every client that connects to it, and the app starts the task again itself when
    /// the pipe stays missing or the server goes before replying (`windows::setup_task`): at
    /// most one start waits, and it is the app's.
    pub fn queues(self) -> bool {
        match self {
            SystemTask::Refresh | SystemTask::Stop | SystemTask::SetUp => false,
        }
    }

    /// Its instances policy, as the Task Scheduler numbers it (`TASK_INSTANCES_POLICY`): queue or
    /// ignore new, as [`SystemTask::queues`] says.
    pub fn instances(self) -> i32 {
        if self.queues() {
            INSTANCES_QUEUE
        } else {
            INSTANCES_IGNORE_NEW
        }
    }

    /// Whether `found` is this task as `register_task` would register it now for `starters`, its
    /// action `path` (quoted, [`action_path`]) and its descriptor `sddl` as Windows writes it
    /// back: the one action and argument with no working folder, LOCAL SYSTEM at the highest level,
    /// the instances policy exactly (parallel or stop-existing is no ignore-new), the limit,
    /// enabled and not hidden, a logon trigger only where one belongs, and the descriptor.
    /// Anything else is a definition that differs, which an elevated set-up registers again.
    pub fn registered_as(
        self,
        found: &Registered,
        starters: &Starters,
        path: &str,
        sddl: &str,
    ) -> bool {
        let system = ["S-1-5-18", "SYSTEM", "NT AUTHORITY\\SYSTEM"]
            .iter()
            .any(|name| found.user.eq_ignore_ascii_case(name));
        let triggers = i32::from(self.at_logon() && starters.owner().is_some());
        found.actions == 1
            && found.path.eq_ignore_ascii_case(path)
            && found.arguments == self.argument()
            && system
            && found.service_highest
            && found.working_dir.is_empty()
            && found.instances == self.instances()
            && found.limit == self.execution_limit()
            && found.enabled
            && !found.hidden
            && found.triggers == triggers
            && task_sddl_matches(&found.sddl, sddl)
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

/// `ERROR_FILE_NOT_FOUND` as an HRESULT.
const HRESULT_FILE_NOT_FOUND: u32 = 0x8007_0002;
/// `ERROR_PATH_NOT_FOUND` as an HRESULT.
const HRESULT_PATH_NOT_FOUND: u32 = 0x8007_0003;

/// Whether the Task Scheduler's `hresult` says the task, or its folder, isn't there: a missing
/// task is `ERROR_FILE_NOT_FOUND`, and a missing folder may be that or `ERROR_PATH_NOT_FOUND`.
/// Either is `TaskError::NotRegistered`, which leaves the app the prompted set-up.
pub fn not_registered(hresult: u32) -> bool {
    [HRESULT_FILE_NOT_FOUND, HRESULT_PATH_NOT_FOUND].contains(&hresult)
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

/// One step of taking a registered `\Clauseo` task down (`task::unregister_task`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retire {
    /// `IRegisteredTask::put_Enabled(false)`: no start lands from here on.
    Disable,
    /// `IRegisteredTask::Stop`: its running instance ended, as deleting a task leaves its run
    /// going, which for the set-up task would set up a pool the removal is taking down.
    Stop,
    /// `ITaskFolder::DeleteTask`.
    Delete,
}

/// The order every `\Clauseo` task is taken down in: disabled, then stopped, then deleted.
/// Stopped first, a start could land between the stop and the delete and run on.
pub const RETIRE: [Retire; 3] = [Retire::Disable, Retire::Stop, Retire::Delete];

/// Takes a task down by `step`, each of [`RETIRE`] in order; the first failure ends it.
pub fn retire<E>(mut step: impl FnMut(Retire) -> Result<(), E>) -> Result<(), E> {
    RETIRE.into_iter().try_for_each(&mut step)
}

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

/// A registered task as the Task Scheduler gives it back, for [`SystemTask::registered_as`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registered {
    /// The action's program (`IExecAction::Path`), as registered: quoted.
    pub path: String,
    /// The action's arguments.
    pub arguments: String,
    /// How many actions it has.
    pub actions: i32,
    /// The principal's user, as read back (`S-1-5-18`, or a name for it).
    pub user: String,
    /// Whether it runs as a service account at the highest run level.
    pub service_highest: bool,
    /// Its instances policy, as the Task Scheduler numbers it (`TASK_INSTANCES_POLICY`).
    pub instances: i32,
    /// Its execution limit.
    pub limit: String,
    /// The action's working folder (`IExecAction::WorkingDirectory`); empty for none.
    pub working_dir: String,
    /// Whether it is enabled (`ITaskSettings::Enabled`).
    pub enabled: bool,
    /// Whether it is hidden (`ITaskSettings::Hidden`).
    pub hidden: bool,
    /// How many triggers it has.
    pub triggers: i32,
    /// Its owner and DACL, as read back.
    pub sddl: String,
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

/// The one entry the Task Scheduler adds to a task's DACL as it stores it: read for the task's
/// principal, LOCAL SYSTEM for every SYSTEM task, appended after the entries given. CI run
/// 42504256341 registered `O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)` and read back
/// `O:BAD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)(A;;FR;;;SY)`. SYSTEM already has full
/// control in every task descriptor, so it grants nothing.
pub const TASK_SCHEDULER_ADDS: &str = "(A;;FR;;;SY)";

/// Whether `found`, a task's owner and DACL read back as SDDL, is `expected` as the Task
/// Scheduler stores it. The Task Scheduler drops the DACL's protected flag and appends
/// [`TASK_SCHEDULER_ADDS`], so neither makes a task read as changed. Everything else must be
/// exactly as given: the same owner, every entry of `expected` in its order with the same
/// rights, and nothing more than that one entry, once, at the end. A missing or reordered entry,
/// a broader mask, any other added grant, an inherited entry or a null DACL reads as changed. The
/// flags may be only `P`, `AI` and `AR`, which grant nothing when every entry is compared. As
/// with [`sddl_matches`], `expected` goes through `staging::as_read_back` first on Windows.
pub fn task_sddl_matches(found: &str, expected: &str) -> bool {
    let split = |sddl: &str| -> Option<(String, String)> {
        let (owner, dacl) = sddl.split_once("D:")?;
        let start = dacl.find('(').unwrap_or(dacl.len());
        let (flags, aces) = dacl.split_at(start);
        let mut rest = flags;
        while !rest.is_empty() {
            rest = ["P", "AI", "AR"]
                .iter()
                .find_map(|flag| rest.strip_prefix(flag))?;
        }
        // Entries alone, each `(...)`, and nothing after them (no SACL or group).
        let whole = aces
            .split_inclusive(')')
            .all(|ace| ace.starts_with('(') && ace.ends_with(')') && ace[1..].find('(').is_none());
        whole.then(|| (owner.to_owned(), aces.to_owned()))
    };
    let (Some((found_owner, found_aces)), Some((owner, aces))) = (split(found), split(expected))
    else {
        return false;
    };
    let added = found_aces.strip_prefix(aces.as_str());
    found_owner == owner && matches!(added, Some("" | TASK_SCHEDULER_ADDS))
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
/// trailing separator ignored. A check of the text alone: it opens nothing and follows no link,
/// so a junction or symlink in the profile can still send `dir` elsewhere. The protection
/// against that is `exe::owners_data_dir`, which resolves the folder as the owner and takes
/// only one the owner owns.
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
