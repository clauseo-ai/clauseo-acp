//! The G8a cell (private issue #261, cell F3): in a matter holding a git repository, a commit
//! asked for inside the jail leaves `.git` unchanged, file for file.

use std::path::{Path, PathBuf};

use crate::support::{World, git, git_outside, launcher, run_outside, tree};

/// git's runtime: what `xcode-select -p` names (the folder `/usr/bin/git` runs git from), or
/// for Xcode the whole app, whose `Info.plist` and shared frameworks the shim reads. The
/// Command Line Tools are in the base profile already.
fn git_runtime(cwd: &std::path::Path) -> PathBuf {
    let out = run_outside("/usr/bin/xcode-select", &["-p"], cwd);
    out.assert_ok();
    let developer = PathBuf::from(out.stdout.trim());
    if developer.ends_with("Contents/Developer") {
        developer
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_owned()
    } else {
        developer
    }
}

#[test]
fn a_commit_in_the_jail_leaves_git_unchanged() {
    let w = World::new();
    let m1 = w.at("m1");
    let git_set_up = |args: &[&str]| git_outside(args, &m1);
    git_set_up(&["init", "-q", "-b", "main"]).assert_ok();
    w.file("m1/brief.txt", "Clause 4 draft\n");
    git_set_up(&["add", "brief.txt"]).assert_ok();
    git_set_up(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=t@example.com",
        "commit",
        "-qm",
        "first",
    ])
    .assert_ok();
    w.file("m1/brief.txt", "Clause 4 final\n");

    let policy = w.builder().runtime(git_runtime(&m1)).build().unwrap();
    let state = policy.state().to_owned();
    let (launcher, _) = launcher(policy);
    let before = tree(&m1.join(".git"));
    assert!(before.len() > 10, "{}", before.len());
    let home = state.to_string_lossy().into_owned();
    let env = [("HOME", home.as_str()), ("GIT_CONFIG_NOSYSTEM", "1")];
    let commit = [
        "-c",
        "user.name=Agent",
        "-c",
        "user.email=agent@example.com",
        "commit",
        "-am",
        "the agent's commit",
    ];
    let out = git(&launcher, &commit, &m1, &env);
    assert_ne!(out.code, Some(0), "{out:#?}");
    assert!(out.stderr.contains("Operation not permitted"), "{out:#?}");
    assert_eq!(
        tree(&m1.join(".git")),
        before,
        "the jailed commit changed .git"
    );
    // The file the agent edits stays the agent's to change.
    let status = git(&launcher, &["status", "--porcelain"], &m1, &env);
    assert_eq!(status.assert_ok().stdout, " M brief.txt\n");
}
