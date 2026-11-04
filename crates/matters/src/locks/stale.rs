//! When an Office owner file beside a document no longer shows it open: the stale rule, so an
//! owner file a crashed Word left cannot block undo, restore and the agent's writes for good
//! (private #224). [`is_stale`] reads the facts and [`mac_rule`] or [`windows_rule`] decides;
//! anything it cannot read counts as held, so a doubt never lets a write through.
//!
//! - macOS (research 38, point 3): stale only when `F_GETLK` sees no lock on the document, the
//!   owner file's birthtime is older than the start of every running `Microsoft Word` process
//!   (or none runs), and the owner file's folder is on a local APFS or HFS+ volume (`statfs`:
//!   `MNT_LOCAL`, and `apfs` or `hfs` as its type). Word locks the document while it has it
//!   open, and a Word that started after the owner file was made may be saving it (Word swaps in
//!   a new file at each save and locks it a moment later). On a network volume a colleague's
//!   Word elsewhere leaves an owner file and no lock here, so there it stays held.
//! - Windows (the orchestrator's conservative rule): stale only when a no-share open of the
//!   document succeeds and no `WINWORD.EXE` runs. A missing document fails the open: held.
//! - Linux: no Word, so an owner file always means held, as before the rule.
//!
//! Each decision is logged at debug by its category only (`owner_stale`, `owner_held`,
//! `query_failed`), never a path (ADR 0047).

#[cfg(any(target_os = "macos", windows))]
use std::path::Path;
#[cfg(any(test, target_os = "macos", windows))]
use std::time::SystemTime;

/// Word's process name on macOS (`/Applications/Microsoft Word.app/Contents/MacOS/Microsoft
/// Word`, bundle `com.microsoft.Word`; research 38 opens it as `open -a "Microsoft Word"`).
#[cfg(target_os = "macos")]
const WORD_PROCESS: &str = "Microsoft Word";
/// Word's image name on Windows.
#[cfg(windows)]
const WORD_PROCESS: &str = "WINWORD.EXE";

#[cfg(any(test, target_os = "macos", windows))]
/// What the rule decided for one owner file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Every condition holds: the owner file no longer shows the document open.
    Stale,
    /// A condition fails: the document may be open.
    Held,
    /// A query the rule needs failed, so it cannot tell: held.
    QueryFailed,
}

#[cfg(any(test, target_os = "macos", windows))]
impl Decision {
    /// The category the debug log names.
    pub(crate) fn category(self) -> &'static str {
        match self {
            Self::Stale => "owner_stale",
            Self::Held => "owner_held",
            Self::QueryFailed => "query_failed",
        }
    }
}

#[cfg(any(test, target_os = "macos"))]
/// What the macOS rule reads; `None` is a query that failed.
#[derive(Debug, Clone, Default)]
pub(crate) struct MacFacts {
    /// Whether `F_GETLK` found a lock that conflicts with a write lock on the whole document.
    pub(crate) lock_seen: Option<bool>,
    /// The owner file's birthtime.
    pub(crate) owner_born: Option<SystemTime>,
    /// The start of every running Word process; empty when none runs.
    pub(crate) word_starts: Option<Vec<SystemTime>>,
    /// Whether the owner file's folder is on a local APFS or HFS+ volume.
    pub(crate) local_apfs_or_hfs: Option<bool>,
}

#[cfg(any(test, target_os = "macos"))]
/// The macOS rule: [`Decision::Stale`] only when no lock is seen, the owner file was born
/// before every Word process started (strictly), and the volume is local APFS or HFS+. Any
/// fact missing is [`Decision::QueryFailed`], whatever the others say.
pub(crate) fn mac_rule(facts: &MacFacts) -> Decision {
    let (Some(lock_seen), Some(born), Some(starts), Some(local)) = (
        facts.lock_seen,
        facts.owner_born,
        facts.word_starts.as_deref(),
        facts.local_apfs_or_hfs,
    ) else {
        return Decision::QueryFailed;
    };
    if lock_seen || !local || starts.iter().any(|&start| born >= start) {
        Decision::Held
    } else {
        Decision::Stale
    }
}

#[cfg(any(test, windows))]
/// What the Windows rule reads; `None` is a query that failed.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WindowsFacts {
    /// Whether the owner file exists.
    pub(crate) owner_exists: bool,
    /// Whether opening the document with no sharing succeeded (any failure: `false`).
    pub(crate) exclusive_open: bool,
    /// Whether a `WINWORD.EXE` process runs.
    pub(crate) word_running: Option<bool>,
}

#[cfg(any(test, windows))]
/// The Windows rule: [`Decision::Stale`] only when the owner file exists, the no-share open
/// succeeded and no Word runs. A failed process listing is [`Decision::QueryFailed`]; a failed
/// open or a running Word is [`Decision::Held`].
pub(crate) fn windows_rule(facts: WindowsFacts) -> Decision {
    match facts.word_running {
        None => Decision::QueryFailed,
        Some(running) if running || !facts.owner_exists || !facts.exclusive_open => Decision::Held,
        Some(_) => Decision::Stale,
    }
}

/// The starts of the running Word processes, `None` when unknown. With the `test-support`
/// feature and [`crate::test_support::pretend_word_running_since_epoch`] called, one Word
/// started at the Unix epoch instead, so a test's owner file stays held through the rule.
#[cfg(any(target_os = "macos", windows))]
pub(crate) fn word_starts() -> Option<Vec<SystemTime>> {
    #[cfg(feature = "test-support")]
    if crate::test_support::pretending() {
        return Some(vec![SystemTime::UNIX_EPOCH]);
    }
    clauseo_process::processes_named(WORD_PROCESS)
}

/// Whether the owner file `owner` in `parent`, beside `document`, is stale by this OS's rule,
/// logging the decision. Always `false` on Linux. Blocks: on macOS one open, one `fcntl`, one
/// stat, one `statfs` and one listing of every process; on Windows one open and one process
/// snapshot.
pub(crate) fn is_stale(parent: &std::path::Path, owner: &str, document: &std::path::Path) -> bool {
    #[cfg(any(target_os = "macos", windows))]
    {
        let decision = decide(parent, owner, document, word_starts);
        tracing::debug!(
            decision = decision.category(),
            "owner file checked for staleness"
        );
        decision == Decision::Stale
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (parent, owner, document);
        tracing::debug!(decision = "owner_held", "owner file checked for staleness");
        false
    }
}

/// macOS: the facts read, with Word's starts from `word`, and [`mac_rule`]'s decision.
#[cfg(target_os = "macos")]
pub(crate) fn decide(
    parent: &Path,
    owner: &str,
    document: &Path,
    word: impl FnOnce() -> Option<Vec<SystemTime>>,
) -> Decision {
    mac_rule(&MacFacts {
        lock_seen: mac::lock_seen(document),
        owner_born: parent
            .join(owner)
            .symlink_metadata()
            .and_then(|meta| meta.created())
            .ok(),
        word_starts: word(),
        local_apfs_or_hfs: mac::local_apfs_or_hfs(parent),
    })
}

/// Windows: the facts read, with Word's starts from `word`, and [`windows_rule`]'s decision.
#[cfg(windows)]
pub(crate) fn decide(
    parent: &Path,
    owner: &str,
    document: &Path,
    word: impl FnOnce() -> Option<Vec<SystemTime>>,
) -> Decision {
    windows_rule(WindowsFacts {
        owner_exists: parent.join(owner).symlink_metadata().is_ok(),
        exclusive_open: crate::write::open_exclusive(document).is_ok(),
        word_running: word().map(|starts| !starts.is_empty()),
    })
}

/// The macOS queries, through `nix`'s safe wrappers: no `unsafe` here.
#[cfg(target_os = "macos")]
mod mac {
    use std::fs::File;
    use std::path::Path;

    use nix::fcntl::{FcntlArg, fcntl};
    use nix::libc;
    use nix::mount::MntFlags;

    /// Whether another process holds an advisory lock on `document` that a write lock over the
    /// whole file would conflict with (Word holds a shared one while the document is open):
    /// `F_GETLK`, which takes no lock, on a read-only open. `None` when the open or the call
    /// fails. Closing the file drops any POSIX lock this process holds on the document; the app
    /// holds none on matter files.
    pub(super) fn lock_seen(document: &Path) -> Option<bool> {
        let file = File::open(document).ok()?;
        let mut query = libc::flock {
            l_start: 0,
            // 0: to the end of the file, however it grows.
            l_len: 0,
            l_pid: 0,
            l_type: libc::F_WRLCK,
            l_whence: libc::c_short::try_from(libc::SEEK_SET).ok()?,
        };
        fcntl(&file, FcntlArg::F_GETLK(&mut query)).ok()?;
        Some(query.l_type != libc::F_UNLCK)
    }

    /// Whether `folder` is on a local APFS or HFS+ volume: `MNT_LOCAL` in its `statfs` flags and
    /// `apfs` or `hfs` as its type. `None` when `statfs` fails.
    pub(super) fn local_apfs_or_hfs(folder: &Path) -> Option<bool> {
        let stat = nix::sys::statfs::statfs(folder).ok()?;
        Some(
            stat.flags().contains(MntFlags::MNT_LOCAL)
                && matches!(stat.filesystem_type_name(), "apfs" | "hfs"),
        )
    }
}

#[cfg(test)]
mod tests;
