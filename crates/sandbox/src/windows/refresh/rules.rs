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

/// The SYSTEM tasks `--adopt` registers in `\Clauseo`, each running the admin copy with one
/// fixed argument. Both share the principal (LOCAL SYSTEM), the descriptor ([`task_sddl`]),
/// one instance at a time and the settings `task.rs` gives every task; they differ only in
/// what [`SystemTask`]'s methods say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTask {
    /// `\Clauseo\SandboxRefresh`: the admin copy's refresh (`--refresh`).
    Refresh,
    /// `\Clauseo\SandboxStop`: stops every process of the pool's users (`--stop`).
    Stop,
}

/// Every task, in the order `--adopt` registers them.
pub const TASKS: [SystemTask; 2] = [SystemTask::Refresh, SystemTask::Stop];

impl SystemTask {
    /// Its name in `\Clauseo`.
    pub fn name(self) -> &'static str {
        match self {
            SystemTask::Refresh => "SandboxRefresh",
            SystemTask::Stop => "SandboxStop",
        }
    }

    /// Its action's one argument.
    pub fn argument(self) -> &'static str {
        match self {
            SystemTask::Refresh => REFRESH_ARG,
            SystemTask::Stop => STOP_ARG,
        }
    }

    /// How long a run may take before the Task Scheduler ends it.
    pub fn execution_limit(self) -> &'static str {
        match self {
            SystemTask::Refresh => EXECUTION_LIMIT,
            SystemTask::Stop => STOP_EXECUTION_LIMIT,
        }
    }

    /// Whether it also starts at the record owner's logon; otherwise only on demand.
    pub fn at_logon(self) -> bool {
        match self {
            SystemTask::Refresh => true,
            SystemTask::Stop => false,
        }
    }

    /// Its description, as the Task Scheduler shows it.
    pub fn description(self) -> &'static str {
        match self {
            SystemTask::Refresh => {
                "Keeps the admin copy of Clauseo's sandbox set-up program current."
            }
            SystemTask::Stop => "Stops the processes of Clauseo's sandbox users.",
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
pub const TEARDOWN: [Teardown; 5] = [
    Teardown::AdminValues,
    Teardown::Task(SystemTask::Refresh),
    Teardown::Task(SystemTask::Stop),
    Teardown::Staging,
    Teardown::AdminFolder,
];

/// The descriptor of every task in [`TASKS`] and of their folder: owned by Administrators;
/// protected; SYSTEM and Administrators full control; the record owner `owner` read alone
/// (`FR`). MS-TSCH lets a caller with read or execute access start a task (`SchRpcRun`) and
/// asks execute access to end one (`SchRpcStop`), so the app can ask for a run but can't end a
/// running task, change it or delete it.
pub fn task_sddl(owner: &Sid) -> String {
    format!("O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;{owner})")
}

/// The task action's program: `exe`'s path in double quotes, as a command line takes a path
/// with spaces (`C:\Program Files\...`), so no prefix of it can be read as the program.
pub fn action_path(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// The staging folder's owner and DACL: SYSTEM, alone, full control, inherited by what is made
/// inside; protected.
pub const STAGING_SDDL: &str = "O:SYD:P(A;OICI;FA;;;SY)";

/// Whether `found`, an owner and DACL read back as SDDL, is `expected`: the same text but for
/// the DACL's `AI` and `AR` flags, which Windows adds when it sets a descriptor through
/// `SetSecurityInfo` and which grant nothing.
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
