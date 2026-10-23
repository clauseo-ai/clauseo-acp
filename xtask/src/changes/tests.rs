//! Tests for `changes`: the selection, the git reads and the scan range.

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
fn a_doc_protocol_tests_read_selects_protocol_and_its_dependents() {
    let mut members = workspace();
    members.push(member("clauseo-protocol", "crates/protocol", &[]));
    members.push(member("host", "crates/host", &["crates/protocol"]));
    for doc in [
        "docs/ui-states.md",
        "docs/decisions/0002-event-schema.md",
        "docs/decisions/0003-error-codes.md",
    ] {
        let sel = select(&files(&[doc]), &members);
        assert_eq!(
            sel,
            Selection::Packages(vec!["clauseo-protocol".into(), "host".into()]),
            "{doc}"
        );
        assert_eq!(
            direct_members(&files(&[doc]), &members),
            ["clauseo-protocol"],
            "{doc}"
        );
    }
    // Only a doc in the map has an owner: any other file, here one at the root, selects nothing
    // even with the owner in the workspace.
    let sel = select(&files(&["README.md"]), &members);
    assert_eq!(sel, Selection::Packages(vec![]));
}

#[test]
fn other_docs_select_nothing_and_a_doc_owner_missing_from_the_workspace_selects_nothing() {
    let sel = select(
        &files(&[
            "docs/decisions/0001-decision-process.md",
            "docs/glossary.md",
        ]),
        &workspace(),
    );
    assert_eq!(sel, Selection::Packages(vec![]));
    let sel = select(&files(&["docs/ui-states.md"]), &workspace());
    assert_eq!(sel, Selection::Packages(vec![]));
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
    crate::scratch::Scratch::remove_file(dir.join("src/gone.rs")).expect("remove a tracked file");
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
fn the_diff_is_against_the_merge_base_with_origin_main_with_a_and_b_prefixes() {
    let dir = crate::scratch::Scratch::new("changes-diff-merge-base");
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
    assert_eq!(merge_base(&dir), Ok(base.trim().to_owned()));
    let diff = diff_since_merge_base(&dir).expect("the diff");
    assert!(diff.contains("--- a/a.rs\n+++ b/a.rs\n"), "{diff}");
    assert!(diff.contains("-fn a() {}\n+fn b() {}\n"), "{diff}");
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
