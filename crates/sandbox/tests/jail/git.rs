//! The G8a cell (private issue #261, cell F3): in a matter holding a git repository, a commit
//! asked for inside the jail leaves `.git` unchanged, file for file. And no new `.git` is made
//! in the matter: one the agent made would be the lawyer's, run by the lawyer's git.

use std::path::{Path, PathBuf};

use crate::support::{World, git, git_outside, launcher, run_outside, sh, tree};

/// git's runtime: what `xcode-select -p` names (the folder `/usr/bin/git` runs git from), or
/// for Xcode the whole app, whose `Info.plist` and shared frameworks the shim reads. The
/// Command Line Tools are in the base profile already.
pub(crate) fn git_runtime(cwd: &std::path::Path) -> PathBuf {
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
        "ln x .git",
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

/// A bare repository needs no `.git` name: `HEAD`, `objects/` and `refs/` make one. No `HEAD`
/// is made, and a folder holding one at launch is read-only with its whole tree, as is the
/// folder a `commondir` beside it names.
#[test]
fn no_head_is_made_and_an_existing_bare_repository_is_read_only() {
    let w = World::new();
    w.dir("m1/docs");
    w.file("m1/old.git/HEAD", "ref: refs/heads/main\n");
    w.file("m1/old.git/config", "[core]\n\tbare = true\n");
    w.dir("m1/old.git/objects");
    w.dir("m1/old.git/refs/heads");
    // A lone HEAD the agent could complete, and a worktree-style git dir's commondir target.
    w.file("m1/lone/HEAD", "ref: refs/heads/main\n");
    w.file("m1/wt/HEAD", "ref: refs/heads/main\n");
    w.file("m1/wt/commondir", "../common\n");
    w.dir("m1/common");
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    let before = tree(&m1.join("old.git"));
    for script in [
        "touch HEAD",
        "touch docs/HEAD",
        "touch docs/head",
        "printf '[core]\\n\\tfsmonitor = x\\n' >> old.git/config",
        "touch old.git/objects/x",
        "touch old.git/refs/heads/main",
        "mv old.git old2.git",
        "printf '../common\\n' > lone/commondir",
        "mkdir lone/objects",
        "printf '[core]\\n\\tfsmonitor = x\\n' > common/config",
    ] {
        sh(&launcher, &m1, script, &[]).assert_refused();
    }
    assert_eq!(tree(&m1.join("old.git")), before);
    assert!(!m1.join("HEAD").exists() && !m1.join("docs/HEAD").exists());
    sh(&launcher, &m1, "touch HEADER.md ahead docs/HEAD.txt", &[]).assert_ok();
}

/// Names case-insensitive HFS+ opens as `.git` or `HEAD` (it ignores U+200C, U+200D, ...)
/// are refused on every volume; Hindi and Persian names with ZWJ or ZWNJ are not.
#[test]
fn names_hfs_reads_as_git_or_head_are_refused_and_zwj_names_are_not() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    for (cmd, name) in [("mkdir", ".g\u{200C}it"), ("touch", "HE\u{200D}AD")] {
        sh(&launcher, &m1, &format!("{cmd} \"$1\""), &[name]).assert_refused();
        assert!(!m1.join(name).exists(), "{name:?} was made");
    }
    // क्‍ष (with ZWJ) and می‌خواهم (with ZWNJ).
    let names = [
        "\u{915}\u{94D}\u{200D}\u{937}.txt",
        "\u{645}\u{6CC}\u{200C}\u{62E}\u{648}\u{627}\u{647}\u{645}.txt",
    ];
    sh(
        &launcher,
        &m1,
        r#"for f in "$@"; do printf x > "$f" || exit 1; done"#,
        &names,
    )
    .assert_ok();
    for name in names {
        assert!(m1.join(name).is_file(), "{name:?}");
    }
}
