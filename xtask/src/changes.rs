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

/// Names of the members that own at least one changed file (before dependents are added).
pub(crate) fn direct_members(changed: &[String], members: &[Member]) -> Vec<String> {
    let names: BTreeSet<String> = changed
        .iter()
        .filter_map(|file| package_for(file, members))
        .map(|m| m.name.clone())
        .collect();
    names.into_iter().collect()
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
mod tests {
    use super::*;

    fn member(name: &str, dir: &str, path_deps: &[&str]) -> Member {
        Member {
            name: name.into(),
            dir: dir.into(),
            path_deps: path_deps.iter().map(ToString::to_string).collect(),
            targets: Vec::new(),
            dependencies: crate::workspace::PackageDependencies::default(),
        }
    }

    fn files(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    fn workspace() -> Vec<Member> {
        vec![
            member("a", "crates/a", &[]),
            member("b", "crates/b", &["crates/a"]),
            member("c", "crates/c", &[]),
            member("xtask", "xtask", &[]),
        ]
    }

    #[test]
    fn root_manifest_selects_all() {
        let sel = select(&files(&["Cargo.toml"]), &workspace());
        assert!(matches!(sel, Selection::All(ref r) if r.contains("Cargo.toml")));
    }

    #[test]
    fn lockfile_and_toolchain_select_all() {
        for f in ["Cargo.lock", "rust-toolchain.toml"] {
            let sel = select(&files(&[f]), &workspace());
            assert!(matches!(sel, Selection::All(_)), "{f} should select all");
        }
    }

    #[test]
    fn xtask_change_selects_all() {
        let sel = select(&files(&["xtask/src/x.rs"]), &workspace());
        assert!(matches!(sel, Selection::All(_)));
    }

    #[test]
    fn config_and_lint_files_select_all() {
        for f in [
            ".cargo/config.toml",
            ".config/nextest.toml",
            "clippy.toml",
            "deny.toml",
        ] {
            let sel = select(&files(&[f]), &workspace());
            assert!(matches!(sel, Selection::All(_)), "{f} should select all");
        }
    }

    #[test]
    fn change_selects_package_and_its_dependents() {
        let sel = select(&files(&["crates/a/src/lib.rs"]), &workspace());
        assert_eq!(sel, Selection::Packages(vec!["a".into(), "b".into()]));
    }

    #[test]
    fn nested_manifest_selects_package_and_dependents_not_all() {
        let sel = select(&files(&["crates/a/Cargo.toml"]), &workspace());
        assert_eq!(sel, Selection::Packages(vec!["a".into(), "b".into()]));
    }

    #[test]
    fn dependents_are_transitive() {
        let mut members = workspace();
        members.push(member("d", "crates/d", &["crates/b"]));
        let sel = select(&files(&["crates/a/src/lib.rs"]), &members);
        assert_eq!(
            sel,
            Selection::Packages(vec!["a".into(), "b".into(), "d".into()])
        );
    }

    #[test]
    fn file_outside_every_member_selects_nothing() {
        let sel = select(
            &files(&["README.md", ".github/workflows/cla.yml"]),
            &workspace(),
        );
        assert_eq!(sel, Selection::Packages(vec![]));
    }

    #[test]
    fn split_nul_drops_empty_entries() {
        let parts: Vec<String> = split_nul("a.rs\0crates/b/x.rs\0\0").collect();
        assert_eq!(parts, vec!["a.rs".to_owned(), "crates/b/x.rs".to_owned()]);
    }

    #[test]
    fn a_branch_scans_its_own_commits() {
        let range = choose_range("abc".into(), Some("def"), true, true);
        assert_eq!(range, ScanRange::Since("abc".into()));
        assert_eq!(range.log_opts().as_deref(), Some("abc..HEAD"));
    }

    #[test]
    fn on_main_the_merge_base_is_head_so_the_last_commit_is_scanned() {
        let range = choose_range("abc".into(), Some("abc"), true, false);
        assert_eq!(range, ScanRange::LastCommit);
        assert_eq!(range.log_opts().as_deref(), Some("HEAD~1..HEAD"));
    }

    #[test]
    fn in_ci_on_main_a_push_may_hold_several_commits_so_all_history_is_scanned() {
        let range = choose_range("abc".into(), Some("abc"), true, true);
        assert!(matches!(range, ScanRange::AllHistory(_)));
        assert_eq!(range.log_opts(), None);
    }

    #[test]
    fn a_root_commit_on_main_scans_all_history() {
        let range = choose_range("abc".into(), Some("abc"), false, false);
        assert!(matches!(range, ScanRange::AllHistory(_)));
        assert_eq!(range.log_opts(), None);
    }

    #[test]
    fn repo_files_are_tracked_and_unignored_files_on_disk() {
        let dir = crate::scratch::Scratch::new("changes-repo-files");
        git(&dir, &["init", "-q"]).expect("git init");
        crate::scratch::Scratch::write(dir.join(".gitignore"), "ignored.rs\n")
            .expect("write .gitignore");
        crate::scratch::Scratch::create_dir_all(dir.join("src")).expect("create src");
        for file in ["src/kept.rs", "src/gone.rs", "new.ts", "ignored.rs"] {
            crate::scratch::Scratch::write(dir.join(file), "x\n").expect("write a file");
        }
        git(&dir, &["add", "src/kept.rs", "src/gone.rs"]).expect("git add");
        crate::scratch::Scratch::remove_file(dir.join("src/gone.rs"))
            .expect("remove a tracked file");
        assert_eq!(
            repo_files(&dir),
            Ok(files(&[".gitignore", "new.ts", "src/kept.rs"]))
        );
    }

    #[test]
    fn the_merge_base_with_origin_main_is_the_commit_the_branch_forked_from() {
        let dir = crate::scratch::Scratch::new("changes-merge-base");
        let commit = [
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@x",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qam",
        ];
        git(&dir, &["init", "-q"]).expect("git init");
        crate::scratch::Scratch::write(dir.join("a.rs"), "fn a() {}\n").expect("write a.rs");
        git(&dir, &["add", "a.rs"]).expect("git add");
        git(&dir, &[&commit[..], &["base"]].concat()).expect("commit the base");
        let base = git(&dir, &["rev-parse", "HEAD"]).expect("rev-parse");
        git(&dir, &["update-ref", "refs/remotes/origin/main", "HEAD"]).expect("set origin/main");
        crate::scratch::Scratch::write(dir.join("a.rs"), "fn b() {}\n").expect("change a.rs");
        git(&dir, &[&commit[..], &["change"]].concat()).expect("commit the change");
        let base = base.trim().to_owned();
        assert_eq!(merge_base(&dir), Ok(base.clone()));
        assert_eq!(scan_range(&dir, false), ScanRange::Since(base));
    }

    #[test]
    fn repo_files_fail_outside_a_git_repo_with_git_s_command_and_error() {
        let dir = crate::scratch::Scratch::new("changes-repo-files-none");
        let Err(GitError::Failed { command, stderr }) = repo_files(&dir) else {
            panic!("expected git to fail outside a repo");
        };
        assert_eq!(
            command,
            "git ls-files -z --cached --others --exclude-standard"
        );
        assert!(stderr.contains("not a git repository"), "{stderr}");
    }
}
