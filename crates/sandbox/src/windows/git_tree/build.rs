//! The order of the Git trees' build ([`ensure`]), as `git_tree.rs` says, over [`TreeFs`]: the
//! source copy, each account's tree, the record. What goes afterwards is `retire.rs`. Pure:
//! tested on every OS with the fake.

use std::path::{Path, PathBuf};

use super::discard::{Discarded, discard, empty_trash};
use super::discover::{discover, identity};
use super::retire::{remove, retire_all, write_record};
use super::stub::place_stub;
use super::{
    Account, BUILDING, Entry, GIT_DIR, GIT_EXE, MARKER, MSYS_DLL, Owner, Recorded, Refused,
    SOURCE_DIR, Source, TreeFs, Unavailable, account_sddl, folder_name_ok, git_dir_sddl, joined,
    marker_matches, marker_sddl, marker_text, source_sddl,
};
use crate::windows::admin_copy::Found;
use crate::windows::sids::Sid;

/// Builds or keeps every account's tree, records the outcome, then deletes the trees no run
/// uses, as the module says. Returns what it recorded, one entry per account, in order. Never
/// fails: a failure is logged and recorded as `Git Bash unavailable: <code>`.
pub fn ensure(fs: &mut impl TreeFs, accounts: &[Account], group: &Sid) -> Vec<(Sid, Recorded)> {
    ensure_with(fs, accounts, group, None)
}

/// [`ensure`], and the Git Bash stub copied from `runner` (the set-up's;
/// `stub::place_stub`) once `git\` is checked; `None` (the refresh) keeps the stub there.
pub fn ensure_with(
    fs: &mut impl TreeFs,
    accounts: &[Account],
    group: &Sid,
    runner: Option<&Path>,
) -> Vec<(Sid, Recorded)> {
    if accounts.is_empty() {
        // No pool (an in-app removal kept the admin copy and its refresh): nothing to serve.
        if let Err(why) = remove(fs) {
            tracing::warn!(%why, "the Git trees' record of a removed pool couldn't be deleted");
        }
        return Vec::new();
    }
    let (outcomes, plan) = match prepared(fs, group) {
        Ok(mut plan) => {
            let outcomes: Vec<(Sid, Recorded)> = accounts
                .iter()
                .map(|account| (account.sid.clone(), account_tree(fs, &mut plan, account)))
                .collect();
            (outcomes, Some(plan))
        }
        Err(reason) => {
            fs.log(true, &unavailable_line(reason));
            let outcomes = accounts
                .iter()
                .map(|account| (account.sid.clone(), Recorded::Unavailable(reason)))
                .collect();
            (outcomes, None)
        }
    };
    place_stub(
        fs,
        plan.as_ref().map(|plan| plan.git.as_path()),
        runner,
        group,
    );
    let values: Vec<(Sid, String)> = outcomes
        .iter()
        .map(|(sid, recorded)| (sid.clone(), recorded.text()))
        .collect();
    if let Err(why) = write_record(fs, &values) {
        // The record may still name an older tree: nothing is retired.
        tracing::warn!(%why, "the Git trees' record couldn't be written");
        fs.log(true, "the sandbox accounts' Git trees couldn't be recorded");
        return outcomes;
    }
    let current = plan.as_ref().map(|plan| plan.source.id.as_str());
    let git = plan.as_ref().map(|plan| plan.git.clone());
    retire_all(fs, git, current, accounts, &outcomes);
    outcomes
}

/// The Event Log's line for an account left without a tree: fixed words and the code.
pub(super) fn unavailable_line(reason: Unavailable) -> String {
    format!(
        "a sandbox account's Git tree isn't available (Git Bash unavailable: {})",
        reason.code()
    )
}

/// What the accounts' builds share: the folders, the source, and, made when first needed, the
/// copy in `git\_source\<id>\` and its walk.
struct Plan {
    /// `%ProgramFiles%\Clauseo\Sandbox\git`.
    git: PathBuf,
    /// The install, with its walk.
    source: Source,
    /// The pool's group, which the source copy grants.
    group: Sid,
    /// The copy's walk, once it is there for this tree id.
    copy: Option<Vec<Entry>>,
}

/// Steps 1 to 3: the admin folder's `git\` (made and secured, its trash emptied) and the
/// source.
fn prepared(fs: &mut impl TreeFs, group: &Sid) -> Result<Plan, Unavailable> {
    let admin = fs.admin_dir().map_err(|why| {
        tracing::warn!(%why, "the admin folder failed its check for the Git trees");
        Unavailable::FolderRefused
    })?;
    if fs.is_network(&admin).unwrap_or(true) {
        return Err(Unavailable::NetworkPath);
    }
    let source = discover(fs)?;
    let git = admin.join(GIT_DIR);
    match fs.inspect(&git) {
        Ok(None) => {}
        Ok(Some(found)) if found.kind == Found::Folder && found.owner == Owner::System => {}
        Ok(Some(found)) => {
            // Not the trees' own folder: nothing in it is trusted, and all of it is rebuilt.
            tracing::warn!(
                ?found,
                "the Git trees' folder isn't SYSTEM's; it is emptied"
            );
            clear(fs, &git, found.kind);
        }
        Err(why) => {
            tracing::warn!(%why, "the Git trees' folder couldn't be read");
            return Err(Unavailable::FolderRefused);
        }
    }
    level(fs, &git, &git_dir_sddl(group))?;
    empty_trash(fs, &git);
    Ok(Plan {
        git,
        source,
        group: group.clone(),
        copy: None,
    })
}

/// Empties the trees' folder `git`, of kind `kind`, by plain deletes; a link or a file there is
/// deleted as itself.
fn clear(fs: &mut impl TreeFs, git: &Path, kind: Found) {
    if kind != Found::Folder {
        let _left = fs.remove_tree(git);
        return;
    }
    for name in fs.entries(git).unwrap_or_default() {
        discard(fs, git, &git.join(&name));
    }
}

impl Plan {
    /// The copy's folder: `git\_source\<id>`.
    fn copy_dir(&self) -> PathBuf {
        self.git.join(SOURCE_DIR).join(&self.source.id)
    }

    /// Step 4: the copy for this tree id, kept when its marker names the id, else made from
    /// the install; its walk, made once, without its marker.
    fn copy(&mut self, fs: &mut impl TreeFs) -> Result<Vec<Entry>, Unavailable> {
        if let Some(copy) = &self.copy {
            return Ok(copy.clone());
        }
        let sddl = source_sddl(&self.group);
        fresh_level(fs, &self.git, &self.git.join(SOURCE_DIR), &sddl)?;
        let dir = self.copy_dir();
        let marker = marker_text(&self.source.id, None);
        if intact(fs, &dir, &marker)? {
            level(fs, &dir, &sddl)?;
        } else {
            let (root, listing) = (self.source.root.clone(), self.source.listing.clone());
            let files = self.source.files;
            fill(
                fs,
                &self.git,
                &dir,
                &sddl,
                (&marker, &marker_sddl(None)),
                |fs, temp| fs.copy_tree(&root, &listing, temp).map_err(put_failed),
                |fs| unchanged(fs, &root, &files),
            )?;
        }
        let walked = fs.walk(&dir).map_err(|refused| {
            tracing::warn!(?refused, "the Git install's copy couldn't be listed");
            Unavailable::FolderRefused
        })?;
        if walked
            .iter()
            .any(|entry| entry.reparse || !entry.admin_only)
        {
            // Ours alone, and changed: the next run makes it again.
            discard(fs, &self.git, &dir);
            return Err(Unavailable::FolderRefused);
        }
        let walked: Vec<Entry> = walked
            .into_iter()
            .filter(|entry| !entry.rel.as_os_str().eq_ignore_ascii_case(MARKER))
            .collect();
        self.copy = Some(walked.clone());
        Ok(walked)
    }
}

/// Step 5 for one account: its folder, its current tree, kept or built.
fn account_tree(fs: &mut impl TreeFs, plan: &mut Plan, account: &Account) -> Recorded {
    if !folder_name_ok(&account.name) {
        fs.log(true, &unavailable_line(Unavailable::BadAccount));
        return Recorded::Unavailable(Unavailable::BadAccount);
    }
    let folder = plan.git.join(&account.name);
    let tree = folder.join(&plan.source.id);
    match build_tree(fs, plan, &folder, &tree, &account.sid) {
        Ok(()) => Recorded::Tree(tree),
        Err(reason) => {
            fs.log(true, &unavailable_line(reason));
            Recorded::Unavailable(reason)
        }
    }
}

/// The account `account`'s folder and its tree `tree`: kept when SYSTEM's and its marker names
/// the id and the SID (secured again), else linked from the copy.
fn build_tree(
    fs: &mut impl TreeFs,
    plan: &mut Plan,
    folder: &Path,
    tree: &Path,
    account: &Sid,
) -> Result<(), Unavailable> {
    let sddl = account_sddl(account);
    fresh_level(fs, &plan.git, folder, &sddl)?;
    let marker = marker_text(&plan.source.id, Some(account));
    if intact(fs, tree, &marker)? {
        return level(fs, tree, &sddl);
    }
    let entries = plan.copy(fs)?;
    let copy = plan.copy_dir();
    let reader = marker_sddl(Some(account));
    let marked = (marker.as_str(), reader.as_str());
    fill(
        fs,
        &plan.git,
        tree,
        &sddl,
        marked,
        |fs, temp| fs.link_tree(&copy, &entries, temp).map_err(put_failed),
        |_fs| Ok(()),
    )
}

/// Whether `dir` is a finished tree or copy: a folder SYSTEM owns, its marker `marker`.
pub(super) fn intact(fs: &mut impl TreeFs, dir: &Path, marker: &str) -> Result<bool, Unavailable> {
    let found = fs.inspect(dir).map_err(|why| {
        tracing::warn!(%why, "a Git tree couldn't be read");
        Unavailable::FolderRefused
    })?;
    let Some(found) = found else {
        return Ok(false);
    };
    if found.kind != Found::Folder || found.owner != Owner::System {
        return Ok(false);
    }
    let found = fs.read_marker(dir).ok().flatten();
    Ok(found.is_some_and(|found| marker_matches(&found, marker)))
}

/// `to`, which isn't [`intact`], built afresh: whatever is there discarded, then built under
/// `<to>.tmp` by `put`, given `marker` (its text and descriptor) as the last file in it,
/// checked a last time by `recheck`, and renamed into place. A failed build discards its
/// temporary tree, and a tree with no marker is never kept: a reader that finds one finds a
/// finished tree.
fn fill<F: TreeFs>(
    fs: &mut F,
    git: &Path,
    to: &Path,
    sddl: &str,
    marker: (&str, &str),
    put: impl FnOnce(&mut F, &Path) -> Result<(), (Unavailable, String)>,
    recheck: impl FnOnce(&mut F) -> Result<(), (Unavailable, String)>,
) -> Result<(), Unavailable> {
    let temp = building(to);
    for stale in [to, temp.as_path()] {
        if discard(fs, git, stale) == Discarded::Left {
            return Err(Unavailable::BuildFailed);
        }
    }
    level(fs, &temp, sddl)?;
    let filled = put(fs, &temp)
        .and_then(|()| {
            fs.write_marker(&temp, marker.0, marker.1)
                .map_err(|why| (Unavailable::BuildFailed, why))
        })
        .and_then(|()| recheck(fs))
        .and_then(|()| {
            fs.rename(&temp, to)
                .map_err(|why| (Unavailable::BuildFailed, why))
        });
    match filled {
        Ok(()) => Ok(()),
        Err((reason, why)) => {
            tracing::warn!(%why, "a Git tree couldn't be built");
            discard(fs, git, &temp);
            Err(reason)
        }
    }
}

/// A refused copy or link as the build's failure.
fn put_failed(refused: Refused) -> (Unavailable, String) {
    match refused {
        Refused::Reparse => (Unavailable::ReparsePoint, "a reparse point".to_owned()),
        Refused::Changed => (
            Unavailable::BuildFailed,
            "a file changed since it was listed".to_owned(),
        ),
        Refused::Other(why) => (Unavailable::BuildFailed, why),
    }
}

/// Whether the install's `msys-2.0.dll` and `git.exe` are still the files the tree id names:
/// an update that ran during the copy fails it.
fn unchanged(
    fs: &mut impl TreeFs,
    root: &Path,
    files: &[super::FileId; 2],
) -> Result<(), (Unavailable, String)> {
    let now = [
        identity(fs, &joined(root, &MSYS_DLL)),
        identity(fs, &joined(root, &GIT_EXE)),
    ];
    if now == [Ok(files[0]), Ok(files[1])] {
        Ok(())
    } else {
        let why = "Git changed while it was copied".to_owned();
        Err((Unavailable::BuildFailed, why))
    }
}

/// The temporary name `to` is built under.
fn building(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(BUILDING);
    to.with_file_name(name)
}

/// A folder of the trees' chain below `git\`: discarded first when it is there but isn't a
/// folder SYSTEM owns (what someone else made, or a link), then [`level`].
pub(super) fn fresh_level(
    fs: &mut impl TreeFs,
    git: &Path,
    path: &Path,
    sddl: &str,
) -> Result<(), Unavailable> {
    match fs.inspect(path) {
        Ok(Some(found)) if found.kind != Found::Folder || found.owner != Owner::System => {
            tracing::warn!(
                ?found,
                "a Git tree's folder isn't SYSTEM's; it is made again"
            );
            if discard(fs, git, path) == Discarded::Left {
                return Err(Unavailable::FolderRefused);
            }
        }
        Ok(_) => {}
        Err(why) => {
            tracing::warn!(%why, "a Git tree's folder couldn't be read");
            return Err(Unavailable::FolderRefused);
        }
    }
    level(fs, path, sddl)
}

/// A folder of the trees: made or opened as itself, given `sddl`, read back.
pub(super) fn level(fs: &mut impl TreeFs, path: &Path, sddl: &str) -> Result<(), Unavailable> {
    fs.level(path, sddl).map_err(|refused| match refused {
        Refused::Reparse => Unavailable::ReparsePoint,
        Refused::Other(why) => {
            tracing::warn!(%why, "a Git tree folder failed its check");
            Unavailable::FolderRefused
        }
        Refused::Changed => Unavailable::FolderRefused,
    })
}
