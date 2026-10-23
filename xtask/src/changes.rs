//! `--quick` change detection: which packages changed since `origin/main`, and who depends on them.

use crate::process::{self, Cmd, Program, Status};
use crate::workspace::{Member, package_for};
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::time::Duration;

/// Which packages `--quick` checks.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Selection {
    /// Every package, with the reason.
    All(String),
    /// These packages (sorted by name); empty means nothing changed.
    Packages(Vec<String>),
}

/// Repo-root files whose change selects every package: they change how everything builds or is
/// checked.
const ALL_PACKAGES_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    ".cargo/config.toml",
    ".config/nextest.toml",
    "clippy.toml",
    "deny.toml",
];

/// Directory whose any change selects every package: the xtask itself.
const ALL_PACKAGES_DIR: &str = "xtask/";

/// Docs that protocol's tests read, with the package that owns each. An edit to one of them
/// selects that package, so its nextest step runs in quick mode; no other doc selects a package.
const DOC_OWNERS: &[(&str, &str)] = &[
    ("docs/ui-states.md", "clauseo-protocol"),
    ("docs/decisions/0002-event-schema.md", "clauseo-protocol"),
    ("docs/decisions/0003-error-codes.md", "clauseo-protocol"),
];

/// Pure selection: `changed` are repo-relative `/`-separated paths.
pub(crate) fn select(changed: &[String], members: &[Member]) -> Selection {
    for file in changed {
        if ALL_PACKAGES_FILES.contains(&file.as_str()) || file.starts_with(ALL_PACKAGES_DIR) {
            return Selection::All(format!("{file} changed"));
        }
    }
    let mut selected: BTreeSet<String> = direct_members(changed, members).into_iter().collect();
    loop {
        let before = selected.len();
        for member in members {
            let depends_on_selected = member.path_deps.iter().any(|dep| {
                members
                    .iter()
                    .any(|other| other.dir == *dep && selected.contains(&other.name))
            });
            if depends_on_selected {
                selected.insert(member.name.clone());
            }
        }
        if selected.len() == before {
            break;
        }
    }
    Selection::Packages(selected.into_iter().collect())
}

/// Names of the members that own at least one changed file (before dependents are added). A
/// changed doc in `DOC_OWNERS` counts as its owner's file.
pub(crate) fn direct_members(changed: &[String], members: &[Member]) -> Vec<String> {
    let names: BTreeSet<String> = changed
        .iter()
        .filter_map(|file| {
            package_for(file, members)
                .map(|m| m.name.clone())
                .or_else(|| doc_owner(file, members))
        })
        .collect();
    names.into_iter().collect()
}

/// The member that owns the doc `file` in `DOC_OWNERS`, if the doc is one and the member exists.
fn doc_owner(file: &str, members: &[Member]) -> Option<String> {
    let (_, owner) = DOC_OWNERS.iter().find(|(doc, _)| *doc == file)?;
    members
        .iter()
        .find(|member| member.name == *owner)
        .map(|member| member.name.clone())
}

/// Tracked changes against the merge-base with `origin/main`, plus untracked files.
/// Errors when the merge-base cannot be found or git fails.
pub(crate) fn changed_files(root: &Path) -> Result<Vec<String>, String> {
    let base = merge_base(root)?;
    let tracked = git(
        root,
        &["diff", "-z", "--name-only", "--no-renames", base.as_str()],
    )?;
    let untracked = git(root, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    let files: BTreeSet<String> = split_nul(&tracked).chain(split_nul(&untracked)).collect();
    Ok(files.into_iter().collect())
}

/// Every file in the repo that git tracks or would track: tracked files still on disk, plus
/// untracked files that `.gitignore` does not exclude (`--others`), so a new file is seen before
/// anyone adds it, as `--quick` sees it. Repo-relative, `/`-separated, sorted. Errors when git
/// cannot start, or fails, e.g. outside a git repo.
pub(crate) fn repo_files(root: &Path) -> Result<Vec<String>, GitError> {
    Ok(only_files(root, &repo_entries(root)?))
}

/// Every entry git lists as `repo_files` does, before it keeps only the files on disk: a symlink
/// is one entry, whatever it points at, and a tracked file gone from disk stays. Sorted.
pub(crate) fn repo_entries(root: &Path) -> Result<Vec<String>, GitError> {
    let listed = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let entries: BTreeSet<String> = split_nul(&listed).collect();
    Ok(entries.into_iter().collect())
}

/// The entries among `entries` (repo-relative) that are files on disk under `root`, a symlink to
/// a file included.
pub(crate) fn only_files(root: &Path, entries: &[String]) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| root.join(entry).is_file())
        .cloned()
        .collect()
}

/// The commit this branch forked from: `git merge-base HEAD origin/main`.
pub(crate) fn merge_base(root: &Path) -> Result<String, String> {
    merge_base_commit(root).map_err(String::from)
}

/// `merge_base`, with git's error kept apart for a typed fix.
fn merge_base_commit(root: &Path) -> Result<String, GitError> {
    let args = ["merge-base", "HEAD", "origin/main"];
    let base = git(root, &args)?.trim().to_owned();
    if base.is_empty() {
        return Err(GitError::Failed {
            command: Cmd::new(Program::Git, args, GIT_TIMEOUT).to_string(),
            stderr: "it exited 0 but printed nothing".into(),
        });
    }
    Ok(base)
}

/// `git diff` of the working tree against the merge-base with `origin/main` (the base `--quick`
/// uses), for `cargo xtask mutants`. Untracked files are not in it. The options fix the format
/// whatever the user's git config says: no colour, no external diff tool, no rename pairs, and
/// the `a/` and `b/` prefixes cargo-mutants strips.
pub(crate) fn diff_since_merge_base(root: &Path) -> Result<String, GitError> {
    let base = merge_base_commit(root)?;
    git(
        root,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            base.as_str(),
        ],
    )
}

/// The commits gitleaks scans.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ScanRange {
    /// The commits after this one, the merge-base with `origin/main`.
    Since(String),
    /// Only the last commit: HEAD is the merge-base, as on `main` itself, outside CI.
    LastCommit,
    /// Everything, with the reason: no merge-base; HEAD is the merge-base in CI (a push to
    /// `main` may hold several commits); or HEAD is the merge-base and has no parent.
    AllHistory(String),
}

impl ScanRange {
    /// The `--log-opts` range, or `None` for all history.
    pub(crate) fn log_opts(&self) -> Option<String> {
        match self {
            ScanRange::Since(base) => Some(format!("{base}..HEAD")),
            ScanRange::LastCommit => Some("HEAD~1..HEAD".into()),
            ScanRange::AllHistory(_) => None,
        }
    }
}

/// What gitleaks scans in `root` (see `choose_range`).
pub(crate) fn scan_range(root: &Path, ci: bool) -> ScanRange {
    let base = match merge_base(root) {
        Ok(base) => base,
        Err(error) => {
            return ScanRange::AllHistory(format!("no merge-base with origin/main: {error}"));
        }
    };
    let head = git(root, &["rev-parse", "HEAD"]).map(|text| text.trim().to_owned());
    let has_parent = git(root, &["rev-parse", "--verify", "--quiet", "HEAD~1"]).is_ok();
    choose_range(base, head.ok().as_deref(), has_parent, ci)
}

/// The commits after `base`. When HEAD is `base` (on `main`), that range is empty. In CI that is a
/// push to `main`, which may bring several commits, so all history; locally the last commit, or
/// all history for a root commit.
pub(crate) fn choose_range(
    base: String,
    head: Option<&str>,
    has_parent: bool,
    ci: bool,
) -> ScanRange {
    if head != Some(base.as_str()) {
        ScanRange::Since(base)
    } else if ci {
        ScanRange::AllHistory(
            "CI on the merge-base with origin/main, as after a push to main".into(),
        )
    } else if has_parent {
        ScanRange::LastCommit
    } else {
        ScanRange::AllHistory("HEAD is the merge-base with origin/main and has no parent".into())
    }
}

fn split_nul(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Why a git command gave no output to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitError {
    /// git did not start, e.g. it is not installed; the OS error.
    NotStarted(String),
    /// git ran and failed, e.g. outside a git repo.
    Failed { command: String, stderr: String },
    /// git ran past `GIT_TIMEOUT` and was stopped; the door's message, which names the command.
    TimedOut(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::NotStarted(error) => write!(f, "could not run git: {error}"),
            GitError::Failed { command, stderr } => write!(f, "`{command}` failed: {stderr}"),
            GitError::TimedOut(message) => write!(f, "{message}"),
        }
    }
}

impl From<GitError> for String {
    fn from(error: GitError) -> String {
        error.to_string()
    }
}

/// How long one git command may run: these read the index, refs and one diff, which take well
/// under a second, so a minute means git hangs (on a lock or a credential prompt, say).
const GIT_TIMEOUT: Duration = Duration::from_mins(1);

/// The stdout of `git <args>` run in `root`.
fn git(root: &Path, args: &[&str]) -> Result<String, GitError> {
    let cmd = Cmd::new(Program::Git, args, GIT_TIMEOUT);
    let quiet =
        process::run_quiet(&cmd, root).map_err(|error| GitError::NotStarted(error.to_string()))?;
    if quiet.status == Status::TimedOut {
        return Err(GitError::TimedOut(process::timeout_message(&cmd)));
    }
    if !quiet.success() {
        return Err(GitError::Failed {
            command: cmd.to_string(),
            stderr: quiet.stderr.trim().to_owned(),
        });
    }
    Ok(quiet.stdout)
}

#[cfg(test)]
mod tests;
