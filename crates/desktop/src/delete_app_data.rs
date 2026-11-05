//! `Clauseo.exe --delete-app-data`: the Windows uninstaller's "Also delete conversations, saved
//! versions and settings" box, done by the app so that no link under the data folders is ever
//! followed. NSIS 3.11's `RMDir /r`, which Tauri's template runs for the box, follows directory
//! junctions: one under the data folder emptied the folder it pointed at
//! (`docs/decisions/0035-release-builds.md`, 2026-11-05). `windows/hooks.nsh` runs this instead
//! and clears the box, so the template's `RMDir /r` never runs.
//!
//! Contract:
//! - [`crate::launch`] answers the flag, the exact first argument, after the pre-write guard and
//!   the sandbox check and before the Tauri builder, its single-instance plugin, any window,
//!   logging or a settings read. Inside the agent sandbox it refuses as every start does; when
//!   the sandbox check fails it refuses with [`FAILED`] and deletes nothing. No command,
//!   capability or bridge path reaches it (a test reads their sources).
//! - It takes no path. The two folders are `FOLDERID_LocalAppData` and `FOLDERID_RoamingAppData`
//!   from the known-folder API (`SystemDirs`), each joined with `ai.clauseo.desktop`: never
//!   `CLAUSEO_DATA_DIR` or any other variable, so a moved data folder is never touched. Its one
//!   argument is [`SANDBOX_REMOVED`], which the hook passes only after S15's `--remove` exited 0;
//!   any other argument is refused.
//! - Refused, with [`FAILED`] and nothing deleted in either folder: an argument it does not know,
//!   a known folder the OS cannot give, a data folder that is itself a reparse point (a junction,
//!   a symbolic link, a mount point) or no folder, or one whose real path is not directly in its
//!   known folder's real path. Both are checked before either is touched.
//! - Nothing is deleted through a path that could have been swapped for a link. A folder is
//!   listed only while it is held open by a handle that refuses a reparse point and shares no
//!   delete or write access (`ops::pin_folder`): each data folder, and on the way to the folder
//!   that stays, `sandbox`. Each entry listed is removed by std alone: a reparse point as a link
//!   (`remove_dir` for a directory one, `remove_file` for a file one), a folder with
//!   `remove_dir_all` (it opens each entry by handle without following a reparse point; Rust 1.99,
//!   `rust-toolchain.toml`, link-safe since the CVE-2022-21658 fix in 1.58.1), a file with
//!   `remove_file` (std removes a read-only one; no attribute is changed). Whatever fails is
//!   reported and nothing below it is tried by path. No other folder is walked by hand.
//! - What stays, in the local folder only, found by name ignoring case (Windows file names):
//!   `sandbox`, whole and unlooked-into, unless [`SANDBOX_REMOVED`] (S15's records, which its
//!   `--remove` needs to take its access rules back off the lawyer's folders); with it,
//!   `sandbox\set-aside` (the lawyer's own files that tier B moved aside) unless it is a real,
//!   empty folder. A `sandbox` that is a link goes as a link and nothing behind it is kept.
//! - Exit codes: [`KEPT_SANDBOX`] or [`KEPT_SET_ASIDE`] whenever that folder stays, whatever else
//!   failed (the uninstaller then names the kept folder, never the whole one to delete by hand);
//!   else [`FAILED`] for a refusal or anything left (locked, or refused by an access rule an
//!   agent made: the rest is deleted, nothing is retried elevated); else [`DELETED`] when both
//!   folders are gone or never were. One stderr line per failure.
//! - Windows only: elsewhere it says so and exits [`FAILED`]. The walk builds on every OS for the
//!   tests, with symbolic links standing in for junctions off Windows.

#[cfg(any(windows, test))]
mod ops;
#[cfg(test)]
mod tests;
#[cfg(any(windows, test))]
mod walk;

use std::ffi::{OsStr, OsString};
use std::io::Write;

/// The flag, as the first argument after the program.
pub(crate) const FLAG: &str = "--delete-app-data";

/// S15's `--remove` exited 0: `<local data folder>\sandbox` may go (but a set-aside holding files).
pub(crate) const SANDBOX_REMOVED: &str = "--sandbox-removed";

/// Both folders are gone, or never were.
#[cfg(any(windows, test))]
pub(crate) const DELETED: i32 = 0;
/// Refused, or something that should have gone is still there.
pub(crate) const FAILED: i32 = 1;
/// `<local data folder>\sandbox` stays: no [`SANDBOX_REMOVED`].
#[cfg(any(windows, test))]
pub(crate) const KEPT_SANDBOX: i32 = 2;
/// `<local data folder>\sandbox\set-aside` stays: it holds the lawyer's files.
#[cfg(any(windows, test))]
pub(crate) const KEPT_SET_ASIDE: i32 = 3;

/// Whether `first`, the first argument after the program, is exactly [`FLAG`].
pub(crate) fn requested(first: &OsStr) -> bool {
    first == OsStr::new(FLAG)
}

/// What the arguments after [`FLAG`] ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mode {
    /// `sandbox` may go ([`SANDBOX_REMOVED`]).
    pub(crate) sandbox_removed: bool,
}

impl Mode {
    /// Nothing, or [`SANDBOX_REMOVED`] alone; anything else (a path included) is `None`.
    pub(crate) fn parse(args: &[OsString]) -> Option<Self> {
        match args {
            [] => Some(Self {
                sandbox_removed: false,
            }),
            [only] if only == SANDBOX_REMOVED => Some(Self {
                sandbox_removed: true,
            }),
            _ => None,
        }
    }
}

/// Runs the deletion for the arguments after [`FLAG`]; answers the exit code. Lines go to
/// `stderr`.
pub(crate) fn run(args: &[OsString], stderr: &mut dyn Write) -> i32 {
    let Some(mode) = Mode::parse(args) else {
        say(
            stderr,
            &format!(
                "{FLAG} takes no path; its only option is {SANDBOX_REMOVED}. Nothing deleted."
            ),
        );
        return FAILED;
    };
    #[cfg(windows)]
    return walk::delete(&clauseo_settings::SystemDirs, mode, stderr);
    #[cfg(not(windows))]
    {
        let _ = mode;
        say(
            stderr,
            &format!("{FLAG} is for the Windows uninstaller only. Nothing deleted."),
        );
        FAILED
    }
}

/// Writes `line` and a newline to `stderr`. The uninstaller shows no console; the exit code says
/// what happened.
fn say(stderr: &mut dyn Write, line: &str) {
    drop(writeln!(stderr, "{line}").and_then(|()| stderr.flush()));
}
