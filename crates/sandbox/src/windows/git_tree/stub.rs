//! The Git Bash stub (0049, "The Git tree"): the runner's exe copied as
//! `git\_off\bash.exe`. When a run has Git Bash off, the outer runner (part 2) points
//! `CLAUDE_CODE_GIT_BASH_PATH` at it, so Claude Code never probes the lawyer's own Git Bash; run
//! under that name, the runner prints one line and exits 1. Claude Code takes the variable only
//! for a file that exists and is named `bash.exe`, so the stub needs nothing but its name.
//!
//! The set-up and the SYSTEM refresh place it ([`place_stub`], given the runner `runner_for_stub`
//! found: beside the set-up, else in the record's `UserInstallDir`); with none found the stub
//! there stays. Those folders are the lawyer's to write, and `_off\bash.exe` is one file the
//! whole machine shares: every pool account's runs with Git Bash off start it. So the runner is
//! copied only when its `clauseo-sandbox-run.exe.sig` verifies with the updater key built in
//! (`refresh::signature::verify_file`, as the refresh checks the set-up exe), over the bytes
//! read through the one handle that are then written; anything else is logged and not copied,
//! and the stub there stays. Placing it never fails the set-up. Pure: tested with the fake.

use std::path::Path;

use super::build::{fresh_level, level};
use super::discard::{Discarded, discard};
use super::{GIT_DIR, Owner, READ_SECURITY, TreeFs, git_dir_sddl, source_sddl};
use crate::windows::admin_copy::Found;
use crate::windows::refresh::signature::verify_file;
use crate::windows::runner::RUNNER_EXE;
use crate::windows::sids::Sid;

/// The stub's folder in [`GIT_DIR`].
pub const OFF_DIR: &str = "_off";
/// The stub's name: one Claude Code accepts for `CLAUDE_CODE_GIT_BASH_PATH`.
pub const STUB_NAME: &str = "bash.exe";

/// The DACL of [`OFF_DIR`]: [`source_sddl`]'s (the pool's group and, on the stub, Users read and
/// execute), and any signed-in user the folder's own security ([`READ_SECURITY`]), so the
/// unelevated outer runner can check the level and find the stub a regular file.
pub fn off_sddl(group: &Sid) -> String {
    format!("{}{READ_SECURITY}", source_sddl(group))
}

/// Copies `runner` to `git\_off\bash.exe` under a temporary name, then renames it into place;
/// `None` (no runner found) leaves what is there. `git` is the trees' folder when this run checked
/// it; else it is found, made when missing, and left alone when it isn't SYSTEM's (the next
/// run with a Git clears it). Anything else in `_off` is deleted. Logs, never fails.
pub(super) fn place_stub(
    fs: &mut impl TreeFs,
    git: Option<&Path>,
    runner: Option<&Path>,
    group: &Sid,
) {
    let Some(runner) = runner else {
        return;
    };
    if place(fs, git, runner, group).is_err() {
        fs.log(
            false,
            "the Git Bash stub couldn't be placed; a run with Git Bash off may find the lawyer's",
        );
    }
}

/// [`place_stub`]'s work; its failure is logged by the caller.
fn place(fs: &mut impl TreeFs, git: Option<&Path>, runner: &Path, group: &Sid) -> Result<(), ()> {
    let git = if let Some(git) = git {
        git.to_path_buf()
    } else {
        let git = fs.admin_dir().map_err(|_why| ())?.join(GIT_DIR);
        match fs.inspect(&git) {
            Ok(None) => level(fs, &git, &git_dir_sddl(group)).map_err(|_reason| ())?,
            Ok(Some(found)) if found.kind == Found::Folder && found.owner == Owner::System => {}
            Ok(Some(_)) | Err(_) => return Err(()),
        }
        git
    };
    let off = git.join(OFF_DIR);
    fresh_level(fs, &git, &off, &off_sddl(group)).map_err(|_reason| ())?;
    for name in fs.entries(&off).unwrap_or_default() {
        let path = off.join(&name);
        let stub = name.eq_ignore_ascii_case(STUB_NAME)
            && matches!(fs.inspect(&path), Ok(Some(found)) if found.kind == Found::File);
        // A stub set aside while it ran may still run: it goes at a later placing.
        let aside = Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("old"));
        if !stub && discard(fs, &git, &path) == Discarded::Left && !aside {
            return Err(());
        }
    }
    let (exe, sig) = fs
        .read_signed(runner)
        .map_err(|refused| tracing::warn!(?refused, "the Git Bash stub's source can't be read"))?;
    if let Err(error) = verify_file(&exe, &sig, &fs.updater_key(), RUNNER_EXE) {
        tracing::warn!(%error, "the Git Bash stub's source isn't signed with the updater key");
        fs.log(
            true,
            "the Git Bash stub's source isn't signed with the updater key; it isn't copied",
        );
        return Err(());
    }
    let temp = format!("{STUB_NAME}.{}.tmp", fs.unique());
    let aside = fs.unique();
    fs.place_bytes(&exe, &off, &temp, STUB_NAME, &aside)
        .map_err(|refused| tracing::warn!(?refused, "the Git Bash stub couldn't be placed"))
}
