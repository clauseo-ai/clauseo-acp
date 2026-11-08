//! The G8a cell (private issue #261, cell F3): in a matter holding a git repository, a commit
//! asked for inside the jail leaves `.git` unchanged, file for file. And no new `.git` is made
//! in the matter: one the agent made would be the lawyer's, run by the lawyer's git.

use std::path::{Path, PathBuf};

use crate::support::{World, git, git_outside, launcher, run_outside, sh, tree};

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

#[test]
fn no_git_can_be_made_or_renamed_onto_anywhere_in_the_matter() {
    let w = World::new();
    w.dir("m1/sub");
    w.file("m1/x", "x\n");
    w.dir("m1/folder");
    let m1 = w.at("m1");
    let policy = w.builder().runtime(git_runtime(&m1)).build().unwrap();
    let home = policy.state().to_string_lossy().into_owned();
    let (launcher, _) = launcher(policy);
    for script in [
        "mkdir .git",
        "mkdir sub/.git",
        "touch .git",
        "printf 'gitdir: ../elsewhere\\n' > sub/.git",
        "mv x .git",
        "mv folder sub/.git",
        "mkdir .GIT",
        "touch sub/.Git",
    ] {
        sh(&launcher, &m1, script, &[]).assert_refused();
    }
    let env = [("HOME", home.as_str())];
    let init = git(&launcher, &["init", "-q", "sub"], &m1, &env);
    assert_ne!(init.code, Some(0), "{init:#?}");
    assert!(init.stderr.contains("Operation not permitted"), "{init:#?}");
    let made: Vec<_> = std::fs::read_dir(&m1)
        .unwrap()
        .chain(std::fs::read_dir(m1.join("sub")).unwrap())
        .map(|e| e.unwrap().file_name())
        .filter(|n| n.eq_ignore_ascii_case(".git"))
        .collect();
    assert!(made.is_empty(), "{made:?}");
    // Names that only contain it stay the agent's to make.
    sh(
        &launcher,
        &m1,
        "touch .gitignore sub/.gitattributes a.git && mkdir .github && touch .github/ci.yml",
        &[],
    )
    .assert_ok();
}

/// The known gap (0038): the name rule sees only the rename's destination, so a folder that
/// already holds a `.git`, made outside the matter, can be moved in. The follow-up (a scan on the
/// lawyer's side) flips this test.
#[test]
fn a_folder_holding_a_git_moved_in_is_not_stopped_by_the_name_rule() {
    let w = World::new();
    w.dir("data/run/s1/tmp");
    let m1 = w.at("m1");
    let policy = w
        .builder()
        .runtime(git_runtime(&m1))
        .tmpdir(w.at("data/run/s1/tmp"))
        .build()
        .unwrap();
    let home = policy.state().to_string_lossy().into_owned();
    let foo = policy.tmpdir().join("foo").to_string_lossy().into_owned();
    let (launcher, _) = launcher(policy);
    git(
        &launcher,
        &["init", "-q", &foo],
        &m1,
        &[("HOME", home.as_str())],
    )
    .assert_ok();
    sh(&launcher, &m1, r#"mv "$1" foo"#, &[&foo]).assert_ok();
    assert!(m1.join("foo/.git/HEAD").is_file());
}
