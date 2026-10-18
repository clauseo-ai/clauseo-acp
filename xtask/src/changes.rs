//! `--quick` change detection: which packages changed since `origin/main`, and who depends on them.

use crate::process::{self, Cmd, Program};
use crate::workspace::{Member, package_for};
use std::collections::BTreeSet;
use std::path::Path;

/// Which packages `--quick` checks.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Selection {
    /// Every package, with the reason.
    All(String),
    /// These packages (sorted by name); empty means nothing changed.
    Packages(Vec<String>),
}

/// Repo-root files whose change selects every package: they change how everything builds or is checked.
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
    let base = git(root, &["merge-base", "HEAD", "origin/main"])?
        .trim()
        .to_owned();
    if base.is_empty() {
        return Err("`git merge-base HEAD origin/main` printed nothing".into());
    }
    let tracked = git(
        root,
        &["diff", "-z", "--name-only", "--no-renames", base.as_str()],
    )?;
    let untracked = git(root, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    let files: BTreeSet<String> = split_nul(&tracked).chain(split_nul(&untracked)).collect();
    Ok(files.into_iter().collect())
}

fn split_nul(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let cmd = Cmd::new(Program::Named("git"), args);
    let quiet = process::run_quiet(&cmd, root).map_err(|e| format!("could not run git: {e}"))?;
    if !quiet.success {
        return Err(format!("`{cmd}` failed: {}", quiet.stderr.trim()));
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
}
