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
/// The WSL deny task's one argument (`wsl_phase::wsl_deny`): it re-applies WSL's deny and does
/// nothing else.
pub const WSL_DENY_ARG: &str = "--wsl-deny";
/// The WSL deny task's execution limit: 6 minutes. Its run holds while a Windows Installer
/// install is in progress and ends by its own budget, 30 s inside this
/// (`wsl_phase/schedule.rs`).
pub const WSL_DENY_EXECUTION_LIMIT: &str = "PT6M";
/// The WSL upgrade task's one argument (`wsl_phase::wsl_upgrade`): it installs WSL's pending
/// upgrade when one is owed and no pool user has a process, then re-applies WSL's deny.
pub(super) const WSL_UPGRADE_ARG: &str = "--wsl-upgrade";
/// The WSL upgrade task's execution limit: 5 minutes, for WSL's MSI upgrade (msiexec waited for
/// up to 4 minutes, `wsl::msi::MSI_WAIT`), then its phase, which waits 10 s for the lock.
pub(super) const WSL_UPGRADE_EXECUTION_LIMIT: &str = "PT5M";

/// The Windows Installer events (`MsiInstaller` in the Application log) that start the WSL
/// deny task, so WSL's deny comes back within seconds of a WSL update, repair or removal: product
/// updated (1022), installed (1033), reconfigured (1035), a transaction begun and ended (1040,
/// 1042), and an install, removal or configuration completed (11707, 11724, 11728). The VM saw
/// 1040, 11728, 1035 and 1042 for a WSL repair (R5c, T3b).
pub(super) const MSI_EVENT_IDS: [u32; 8] = [1022, 1033, 1035, 1040, 1042, 11707, 11724, 11728];

/// What starts a task besides a demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// The record owner's logon.
    Logon,
    /// A Windows Installer event ([`MSI_EVENT_IDS`]), matched by [`msi_event_query`]. It
    /// passes the task nothing from the event: no value queries, and the action keeps its one
    /// fixed argument. Anyone who can log on can write such an event (INTERACTIVE may write to
    /// the Application log), so only [`SystemTask::WslDeny`] has it: a forged event re-applies
    /// the deny and does nothing else; it never starts WSL's upgrade.
    MsiEvents,
}

/// The event trigger's subscription: the Application log's `MsiInstaller` events with one of
/// [`MSI_EVENT_IDS`]. Fixed text, nothing from the record or the event.
pub(super) fn msi_event_query() -> String {
    let ids: Vec<String> = MSI_EVENT_IDS
        .iter()
        .map(|id| format!("EventID={id}"))
        .collect();
    format!(
        "<QueryList><Query Id=\"0\" Path=\"Application\"><Select Path=\"Application\">\
         *[System[Provider[@Name='MsiInstaller'] and ({})]]</Select></Query></QueryList>",
        ids.join(" or ")
    )
}

/// The SYSTEM tasks `--adopt` registers in `\Clauseo`, each running the admin copy with one
/// fixed argument. All share the principal (LOCAL SYSTEM), the descriptor ([`task_sddl`]),
/// one instance at a time and the settings `task.rs` gives every task; they differ only in
/// what [`SystemTask`]'s methods say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTask {
    /// `\Clauseo\SandboxRefresh`: the admin copy's refresh (`--refresh`).
    Refresh,
    /// `\Clauseo\SandboxStop`: stops every process of the pool's users (`--stop`).
    Stop,
    /// `\Clauseo\SandboxWslDeny`: re-applies WSL's deny and nothing else (`--wsl-deny`), on
    /// Windows Installer's events.
    WslDeny,
    /// `\Clauseo\SandboxWslUpgrade`: installs WSL's pending upgrade, then re-applies the deny
    /// (`--wsl-upgrade`), on demand only: the launch check (`clean_close::wsl_heal`) asks it.
    WslUpgrade,
}

/// Every task, in the order `--adopt` registers them.
pub const TASKS: [SystemTask; 4] = [
    SystemTask::Refresh,
    SystemTask::Stop,
    SystemTask::WslDeny,
    SystemTask::WslUpgrade,
];

/// The tasks a per-machine install's set-up registers for the owner it records, and its
/// `--remove` deletes (the refresh is `--adopt`'s per user only: a per-machine copy doesn't
/// update itself).
pub const MACHINE_TASKS: [SystemTask; 3] = [
    SystemTask::Stop,
    SystemTask::WslDeny,
    SystemTask::WslUpgrade,
];

impl SystemTask {
    /// Its name in `\Clauseo`.
    pub fn name(self) -> &'static str {
        match self {
            SystemTask::Refresh => "SandboxRefresh",
            SystemTask::Stop => "SandboxStop",
            SystemTask::WslDeny => "SandboxWslDeny",
            SystemTask::WslUpgrade => "SandboxWslUpgrade",
        }
    }

    /// Its action's one argument.
    pub fn argument(self) -> &'static str {
        match self {
            SystemTask::Refresh => REFRESH_ARG,
            SystemTask::Stop => STOP_ARG,
            SystemTask::WslDeny => WSL_DENY_ARG,
            SystemTask::WslUpgrade => WSL_UPGRADE_ARG,
        }
    }

    /// How long a run may take before the Task Scheduler ends it.
    pub fn execution_limit(self) -> &'static str {
        match self {
            SystemTask::Refresh => EXECUTION_LIMIT,
            SystemTask::Stop => STOP_EXECUTION_LIMIT,
            SystemTask::WslDeny => WSL_DENY_EXECUTION_LIMIT,
            SystemTask::WslUpgrade => WSL_UPGRADE_EXECUTION_LIMIT,
        }
    }

    /// Whether it also starts at the record owner's logon; otherwise only on demand.
    pub fn at_logon(self) -> bool {
        self.triggers().contains(&Trigger::Logon)
    }

    /// What starts it besides a demand: the refresh at the owner's logon; the WSL deny task on
    /// Windows Installer's events (`wsl_phase.rs`); the stop and the WSL upgrade only on demand.
    pub fn triggers(self) -> &'static [Trigger] {
        match self {
            SystemTask::Refresh => &[Trigger::Logon],
            SystemTask::Stop | SystemTask::WslUpgrade => &[],
            SystemTask::WslDeny => &[Trigger::MsiEvents],
        }
    }

    /// Its description, as the Task Scheduler shows it.
    pub fn description(self) -> &'static str {
        match self {
            SystemTask::Refresh => {
                "Keeps the admin copy of Clauseo's sandbox set-up program current."
            }
            SystemTask::Stop => "Stops the processes of Clauseo's sandbox users.",
            SystemTask::WslDeny => "Keeps WSL closed to Clauseo's sandbox users.",
            SystemTask::WslUpgrade => {
                "Installs WSL's pending update before Clauseo's sandbox starts, then keeps WSL \
                 closed to its users."
            }
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

/// The removal's steps, in order: every task in [`TASKS`] goes before the copy they run.
pub const TEARDOWN: [Teardown; 7] = [
    Teardown::AdminValues,
    Teardown::Task(SystemTask::Refresh),
    Teardown::Task(SystemTask::Stop),
    Teardown::Task(SystemTask::WslDeny),
    Teardown::Task(SystemTask::WslUpgrade),
    Teardown::Staging,
    Teardown::AdminFolder,
];

/// The descriptor of every task in [`TASKS`] and of their folder: owned by Administrators;
/// protected; SYSTEM and Administrators full control; the record owner `owner` read and execute
/// (`FRFX`). Starting a task needs execute access: as the owner, `schtasks /run` with `FR`
/// alone was refused (CI run 42416074600). Execute also ends a running task, so the owner can
/// end a stop, a refresh, a WSL deny or a WSL upgrade run: denial of service only, which code
/// running as the owner could do to its own sessions anyway. An ended stop never writes
/// `StopCompleted` (`setup::stop`), an ended refresh keeps the old copy, and a deny left off
/// refuses the launch. The owner can't change or delete a task.
pub fn task_sddl(owner: &Sid) -> String {
    format!("O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;{owner})")
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
