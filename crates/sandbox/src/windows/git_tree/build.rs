//! The order of the Git trees' work ([`ensure`], [`remove`]), as `git_tree.rs` says, over
//! [`TreeFs`]. Pure: tested on every OS with the fake.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::discover::{discover, identity};
use super::{
    Account, BUILDING, Entry, GIT_DIR, GIT_EXE, MSYS_DLL, Recorded, Refused, SOURCE_DIR, Source,
    TreeFs, Unavailable, account_sddl, folder_name_ok, git_dir_sddl, joined, source_sddl,
};
use crate::windows::admin_copy::{self, Found};
use crate::windows::sids::Sid;

/// Builds or keeps every account's tree, records the outcome, then deletes the trees no run
/// uses, as the module says. Returns what it recorded, one entry per account, in order. Never
/// fails: a failure is logged and recorded as `Git Bash unavailable: <code>`.
pub fn ensure(fs: &mut impl TreeFs, accounts: &[Account], group: &Sid) -> Vec<(Sid, Recorded)> {
    if accounts.is_empty() {
        // No pool (an in-app removal kept the admin copy and its refresh): nothing to serve.
        if let Err(why) = remove(fs) {
            tracing::warn!(%why, "the Git trees of a removed pool couldn't be deleted yet");
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
    let values: Vec<(Sid, String)> = outcomes
        .iter()
        .map(|(sid, recorded)| (sid.clone(), recorded.text()))
        .collect();
    if let Err(why) = fs.write_record(&values) {
        // The record may still name an older tree: nothing is retired.
        tracing::warn!(%why, "the Git trees' record couldn't be written");
        fs.log(true, "the sandbox accounts' Git trees couldn't be recorded");
        return outcomes;
    }
    retire_all(fs, plan.as_ref(), accounts, &outcomes);
    outcomes
}

/// Deletes every tree and source copy (the whole `%ProgramFiles%\Clauseo\Sandbox\git\`) and
/// the record's subkey, as `--remove` does; either missing is `Ok`. The folder's path comes
/// from `%ProgramFiles%` alone: no admin copy need be there. The first failure is returned once
/// both were tried.
pub fn remove(fs: &mut impl TreeFs) -> Result<(), String> {
    let folder = fs.program_files().and_then(|program_files| {
        fs.remove_tree(&admin_copy::admin_dir(&program_files).join(GIT_DIR))
    });
    let record = fs.delete_record();
    folder.and(record)
}

/// The Event Log's line for an account left without a tree: fixed words and the code.
pub(super) fn unavailable_line(reason: Unavailable) -> String {
    format!(
        "a sandbox account's Git tree isn't available (Git Bash unavailable: {})",
        reason.code()
    )
}

/// What the accounts' builds share: the folders, the source, and, made when first needed, the
/// install's listing and its copy in `git\_source\<id>\`.
struct Plan {
    /// `%ProgramFiles%\Clauseo\Sandbox\git`.
    git: PathBuf,
    /// The install.
    source: Source,
    /// The pool's group, which the source copy grants.
    group: Sid,
    /// Whether the install is on another volume, so every file comes from the copy.
    copy_all: bool,
    /// The install's entries, once listed.
    entries: Option<Vec<Entry>>,
    /// Whether the copy is there for this tree id.
    copied: bool,
}

/// Steps 1 to 3: the admin folder's `git\` (made and secured) and the source.
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
    level(fs, &git, &git_dir_sddl(group))?;
    let copy_all = match (fs.volume(&source.root), fs.volume(&admin)) {
        (Ok(from), Ok(to)) => from != to,
        (from, to) => {
            let why = from.err().or(to.err()).unwrap_or_default();
            tracing::warn!(%why, "a volume couldn't be read for the Git trees");
            return Err(Unavailable::SourceUnreadable);
        }
    };
    Ok(Plan {
        git,
        source,
        group: group.clone(),
        copy_all,
        entries: None,
        copied: false,
    })
}

impl Plan {
    /// The copy's folder: `git\_source\<id>`.
    fn copy_dir(&self) -> PathBuf {
        self.git.join(SOURCE_DIR).join(&self.source.id)
    }

    /// The install's entries, listed once.
    fn entries(&mut self, fs: &mut impl TreeFs) -> Result<Vec<Entry>, Unavailable> {
        if let Some(entries) = &self.entries {
            return Ok(entries.clone());
        }
        let entries = fs
            .walk(&self.source.root)
            .map_err(|refused| match refused {
                Refused::Reparse => Unavailable::ReparsePoint,
                Refused::Other(why) => {
                    tracing::warn!(%why, "the Git install couldn't be listed");
                    Unavailable::SourceUnreadable
                }
            })?;
        self.entries = Some(entries.clone());
        Ok(entries)
    }

    /// Which of `entries` come from the copy: every file on another volume, else each file
    /// that has another name in the install.
    fn copied_files(&self, entries: &[Entry]) -> Vec<bool> {
        let mut names: BTreeMap<u128, usize> = BTreeMap::new();
        for entry in entries.iter().filter(|entry| !entry.folder) {
            *names.entry(entry.id).or_default() += 1;
        }
        entries
            .iter()
            .map(|entry| {
                !entry.folder
                    && (self.copy_all || names.get(&entry.id).is_some_and(|count| *count > 1))
            })
            .collect()
    }

    /// Step 5's copy, made when some file needs it and it isn't there yet.
    fn ensure_copy(
        &mut self,
        fs: &mut impl TreeFs,
        entries: &[Entry],
        from_copy: &[bool],
    ) -> Result<(), Unavailable> {
        if self.copied || !from_copy.contains(&true) {
            return Ok(());
        }
        let sddl = source_sddl(&self.group);
        level(fs, &self.git.join(SOURCE_DIR), &sddl)?;
        let copy = self.copy_dir();
        let root = self.source.root.clone();
        let files = [self.source.files[0], self.source.files[1]];
        fill(
            fs,
            &copy,
            &sddl,
            &files,
            &root,
            |entry_at| {
                from_copy
                    .get(entry_at)
                    .copied()
                    .map(|copied| (copied, Put::Copy))
            },
            entries,
        )?;
        self.copied = true;
        Ok(())
    }
}

/// Step 4 for one account: its folder, its current tree, kept or built.
fn account_tree(fs: &mut impl TreeFs, plan: &mut Plan, account: &Account) -> Recorded {
    if !folder_name_ok(&account.name) {
        fs.log(true, &unavailable_line(Unavailable::BadAccount));
        return Recorded::Unavailable(Unavailable::BadAccount);
    }
    let folder = plan.git.join(&account.name);
    let sddl = account_sddl(&account.sid);
    let tree = folder.join(&plan.source.id);
    match level(fs, &folder, &sddl).and_then(|()| build_tree(fs, plan, &tree, &sddl)) {
        Ok(()) => Recorded::Tree(tree),
        Err(reason) => {
            fs.log(true, &unavailable_line(reason));
            Recorded::Unavailable(reason)
        }
    }
}

/// The tree `tree`: kept when already a folder (secured again), else built from the install
/// and the copy.
fn build_tree(
    fs: &mut impl TreeFs,
    plan: &mut Plan,
    tree: &Path,
    sddl: &str,
) -> Result<(), Unavailable> {
    if fs.found(tree).map_err(|_why| Unavailable::FolderRefused)? == Found::Folder {
        return level(fs, tree, sddl);
    }
    let entries = plan.entries(fs)?;
    let from_copy = plan.copied_files(&entries);
    plan.ensure_copy(fs, &entries, &from_copy)?;
    let (root, copy) = (plan.source.root.clone(), plan.copy_dir());
    let files = [plan.source.files[0], plan.source.files[1]];
    fill(
        fs,
        tree,
        sddl,
        &files,
        &root,
        |entry_at| {
            from_copy.get(entry_at).map(|copied| {
                let from = if *copied { copy.clone() } else { root.clone() };
                (true, Put::Link(from))
            })
        },
        &entries,
    )
}

/// How [`fill`] puts a file in place.
#[derive(Debug, Clone)]
enum Put {
    /// A hard link of the file at the same path under this root.
    Link(PathBuf),
    /// A copy of the install's file, which inherits its folder's DACL.
    Copy,
}

/// `to` as a tree of `entries` from the install at `root`: missing or replaced, built under
/// `<to>.tmp`, each folder made, each file `how` takes (by its place in `entries`) put there,
/// the install's two named files checked against `files`, then renamed into place. A failed
/// build deletes its temporary tree.
fn fill(
    fs: &mut impl TreeFs,
    to: &Path,
    sddl: &str,
    files: &[super::FileId; 2],
    root: &Path,
    how: impl Fn(usize) -> Option<(bool, Put)>,
    entries: &[Entry],
) -> Result<(), Unavailable> {
    match fs.found(to) {
        Ok(Found::Folder) => return level(fs, to, sddl),
        Ok(Found::Missing) => {}
        Ok(Found::File | Found::Link) => {
            fs.remove_tree(to)
                .map_err(|_why| Unavailable::BuildFailed)?;
        }
        Err(_why) => return Err(Unavailable::FolderRefused),
    }
    let temp = building(to);
    fs.remove_tree(&temp)
        .map_err(|_why| Unavailable::BuildFailed)?;
    level(fs, &temp, sddl)?;
    let filled = put_all(fs, root, &temp, entries, &how)
        .and_then(|()| unchanged(fs, root, files))
        .and_then(|()| {
            fs.rename(&temp, to)
                .map_err(|why| (Unavailable::BuildFailed, why))
        });
    match filled {
        Ok(()) => Ok(()),
        Err((reason, why)) => {
            tracing::warn!(%why, "a Git tree couldn't be built");
            let _left = fs.remove_tree(&temp);
            Err(reason)
        }
    }
}

/// Every entry, in order, into `temp`: each folder made, each file `how` keeps put there.
fn put_all(
    fs: &mut impl TreeFs,
    root: &Path,
    temp: &Path,
    entries: &[Entry],
    how: &impl Fn(usize) -> Option<(bool, Put)>,
) -> Result<(), (Unavailable, String)> {
    let failed = |why| (Unavailable::BuildFailed, why);
    for (at, entry) in entries.iter().enumerate() {
        let target = temp.join(&entry.rel);
        if entry.folder {
            fs.make_dir(&target).map_err(failed)?;
            continue;
        }
        match how(at) {
            Some((true, Put::Link(from))) => fs.hard_link(&from.join(&entry.rel), &target),
            Some((true, Put::Copy)) => fs.copy_file(&root.join(&entry.rel), &target),
            Some((false, _)) | None => Ok(()),
        }
        .map_err(failed)?;
    }
    Ok(())
}

/// Whether the install's `msys-2.0.dll` and `git.exe` are still the files the tree id names:
/// an update that ran during the build fails it.
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
        let why = "Git changed while its tree was built".to_owned();
        Err((Unavailable::BuildFailed, why))
    }
}

/// The temporary name `to` is built under.
fn building(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(BUILDING);
    to.with_file_name(name)
}

/// A folder of the trees: made or opened as itself, given `sddl`, read back.
fn level(fs: &mut impl TreeFs, path: &Path, sddl: &str) -> Result<(), Unavailable> {
    fs.level(path, sddl).map_err(|refused| match refused {
        Refused::Reparse => Unavailable::ReparsePoint,
        Refused::Other(why) => {
            tracing::warn!(%why, "a Git tree folder failed its check");
            Unavailable::FolderRefused
        }
    })
}

/// Step 7, once the record names only current trees: each account's old trees, the folders of
/// accounts no longer in the pool, and the older source copies.
fn retire_all(
    fs: &mut impl TreeFs,
    plan: Option<&Plan>,
    accounts: &[Account],
    outcomes: &[(Sid, Recorded)],
) {
    let git = match plan {
        Some(plan) => plan.git.clone(),
        None => match fs.admin_dir() {
            Ok(admin) => admin.join(GIT_DIR),
            Err(_why) => return,
        },
    };
    let id = plan.map(|plan| plan.source.id.as_str());
    for (account, (_, recorded)) in accounts.iter().zip(outcomes) {
        if folder_name_ok(&account.name) {
            let current = matches!(recorded, Recorded::Tree(_))
                .then_some(id)
                .flatten();
            retire(fs, &git.join(&account.name), &account.sid, current);
        }
    }
    for name in fs.entries(&git).unwrap_or_default() {
        let pool = accounts
            .iter()
            .any(|account| account.name.eq_ignore_ascii_case(&name));
        if pool || name.eq_ignore_ascii_case(SOURCE_DIR) {
            continue;
        }
        // A dropped SID's account is gone, and a rolled-back user was deleted: no run of
        // theirs goes on.
        if let Err(why) = fs.remove_tree(&git.join(&name)) {
            tracing::warn!(%why, "a former account's Git trees couldn't be deleted yet");
        }
    }
    // The trees' links keep the files of an older copy, so it goes at once.
    let sources = git.join(SOURCE_DIR);
    for name in fs.entries(&sources).unwrap_or_default() {
        if id.is_none_or(|id| !name.eq_ignore_ascii_case(id)) {
            let _kept = fs.remove_tree(&sources.join(&name));
        }
    }
}

/// In `folder`, `account`'s: every temporary tree deleted; every tree but `current` deleted
/// once no process runs as the account (one that can't be told counts as running).
fn retire(fs: &mut impl TreeFs, folder: &Path, account: &Sid, current: Option<&str>) {
    let Ok(names) = fs.entries(folder) else {
        return;
    };
    let mut active = None;
    for name in names {
        if current.is_some_and(|id| name.eq_ignore_ascii_case(id)) {
            continue;
        }
        if !name.ends_with(BUILDING) {
            let running = *active.get_or_insert_with(|| fs.active(account).unwrap_or(true));
            if running {
                continue;
            }
        }
        if let Err(why) = fs.remove_tree(&folder.join(&name)) {
            tracing::warn!(%why, "an old Git tree couldn't be deleted yet");
        }
    }
}
