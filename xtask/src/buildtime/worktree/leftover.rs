//! What is left at the worktree's path: the refusal before a run when something is there, the
//! removal after each run, and the one fix line both give (`leftover_fix`).

use super::super::verdict::Failure;
use super::Worktree;
use super::steps::{GIT_TIMEOUT, Where, ran, same_path, with_output};
use crate::process::{Cmd, Program, QuietRunner};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// `git worktree remove` deletes the copied build output, gigabytes on a slow disk.
const REMOVE_TIMEOUT: Duration = Duration::from_mins(10);

/// The fix of whatever is left at the worktree's `path`, in the order its steps apply. An
/// interrupted run of this command can leave each case it names (git 2.54): a `git worktree add`
/// killed mid-checkout leaves a worktree locked as `initializing`, which `git worktree remove`
/// refuses even with `--force`; one killed during `git worktree remove --force` leaves a folder
/// missing tracked files, which plain `git worktree remove` refuses as holding "modified or
/// untracked files", or one whose `.git` file is already gone, which `git worktree list` shows
/// as prunable and `git worktree remove` cannot remove ("validation failed").
pub(crate) fn leftover_fix(path: &Path) -> String {
    let path = path.display();
    format!(
        "run `git worktree list` from the repo root and clear {path} in this order: if it shows \
         {path} locked, `git worktree unlock {path}`; if it shows it prunable or its folder is \
         gone, `git worktree prune`, which forgets every worktree whose folder is gone unless it \
         is locked; if it still names {path}, `git worktree remove {path}`, which refuses a \
         worktree with modified or untracked files, and when that refuses and the folder holds \
         only a checkout of HEAD and target/ (what this command makes), `git worktree remove \
         --force {path}`; move away a folder left there that git does not name (delete it only \
         if it holds nothing but a checkout of HEAD and target/). xtask never deletes what this \
         run did not make; then run again"
    )
}

/// Fails when something is at the worktree's path, or when `git worktree list` names it
/// though its folder is gone: xtask never deletes what this run did not make.
pub(super) fn refuse(worktree: &Worktree<'_>, runner: &mut QuietRunner<'_>) -> Result<(), Failure> {
    let path = worktree.path.display();
    let left = "an interrupted `cargo xtask buildtime worktree` run may have left it";
    if fs::symlink_metadata(&worktree.path).is_ok() {
        return Err(Failure::new(
            format!("{path} is already there; {left}"),
            leftover_fix(&worktree.path),
        ));
    }
    if listed(worktree, runner)? {
        return Err(Failure::new(
            format!("`git worktree list` names {path}, whose folder is gone; {left}"),
            leftover_fix(&worktree.path),
        ));
    }
    Ok(())
}

/// After a run: removes the worktree with `git worktree remove --force` when this run's
/// `git worktree add` succeeded (`added`), then makes sure it is gone from disk and from
/// `git worktree list`. Nothing was at its path right before the add (`refuse`), so after a
/// successful add what is there this run made. When the add failed, xtask removes nothing: git
/// undoes a failed add itself, and xtask cannot tell whether git or someone else made what is
/// left, so it only names it. When nothing is on disk but `git worktree list` failed, the failure
/// says only that. Every failure gets `leftover_fix`.
pub(super) fn remove(
    worktree: &Worktree<'_>,
    added: bool,
    runner: &mut QuietRunner<'_>,
) -> Result<(), Failure> {
    let mut why = Vec::new();
    if added {
        let path = worktree.path.to_string_lossy();
        let remove = ["worktree", "remove", "--force", &path];
        let cmd = Cmd::new(Program::Git, remove, REMOVE_TIMEOUT);
        if let Err(failure) = ran(runner, &cmd, worktree.root, Where::Checkout) {
            why.push(with_output(failure));
        }
    }
    let list_failed = match listed(worktree, runner) {
        Ok(true) => {
            why.push("`git worktree list` names it".to_owned());
            None
        }
        Ok(false) => None,
        Err(failure) => Some(failure.message),
    };
    if fs::symlink_metadata(&worktree.path).is_ok() {
        why.push("its folder is on disk".to_owned());
    }
    let path = worktree.path.display();
    let message = match (why.is_empty(), list_failed) {
        (true, None) => return Ok(()),
        (true, Some(list_failed)) => format!(
            "xtask could not check that `git worktree list` no longer names {path}: \
             {list_failed}"
        ),
        (false, list_failed) => {
            why.extend(list_failed);
            let why = why.join("; ");
            if added {
                format!("could not remove the worktree {path} that this run made: {why}")
            } else {
                format!(
                    "this run's `git worktree add` failed and something is left at {path}: \
                     {why}. xtask cannot tell whether git or someone else made it, so it \
                     leaves it as it is"
                )
            }
        }
    };
    Err(Failure::new(message, leftover_fix(&worktree.path)))
}

/// Whether `git worktree list --porcelain` names the worktree.
fn listed(worktree: &Worktree<'_>, runner: &mut QuietRunner<'_>) -> Result<bool, Failure> {
    let list = ["worktree", "list", "--porcelain"];
    let cmd = Cmd::new(Program::Git, list, GIT_TIMEOUT);
    let listed = ran(runner, &cmd, worktree.root, Where::Checkout)?;
    Ok(listed
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .any(|path| same_path(Path::new(path), &worktree.path)))
}
