//! The folders `commondir` files and `gitdir:` lines name: read-only in the matter, refused in
//! a place the agent can write, and a `commondir` link refused.

use super::*;

const HEAD: &str = "ref: refs/heads/main\n";

/// A worktree's `commondir` under the `.git` names the folder it points at (inside the matter)
/// as read-only; the repository's own `.git`, which `../..` names, is denied once.
#[test]
fn a_worktree_commondir_makes_its_folder_read_only_when_inside_the_matter() {
    let w = World::new("wt-commondir");
    for (wt, line) in [("wt1", "../..\n"), ("wt2", "../../../shared\n")] {
        w.dir
            .file(&format!("matter/.git/worktrees/{wt}/HEAD"), HEAD);
        w.dir
            .file(&format!("matter/.git/worktrees/{wt}/commondir"), line);
    }
    w.dir.dir("matter/shared");
    let policy = w.builder().build().unwrap();
    let matter = canon(&w.at("matter"));
    let rules = policy.read_only_in_matter();
    assert!(rules.contains(&PathRule::Subpath(matter.join("shared"))));
    assert!(rules.contains(&PathRule::Subpath(matter.join(".git"))));
    let git = rules
        .iter()
        .filter(|r| r.path() == matter.join(".git"))
        .count();
    assert_eq!(git, 1, "{rules:?}");
}

/// The world with `.git/commondir` naming `target` (relative to the world) by its absolute
/// path, which exists.
fn naming(test: &str, target: &str) -> (World, PathBuf) {
    let w = World::new(test);
    w.dir.dir(target);
    let target = canon(&w.at(target));
    w.dir.file("matter/.git/HEAD", HEAD);
    w.dir
        .file("matter/.git/commondir", &format!("{}\n", target.display()));
    (w, target)
}

/// A `commondir` naming a folder in the session's temp folder, the notes folder or an allowed
/// folder refuses the launch: the agent could write the repository's `config` there.
#[test]
fn a_commondir_naming_a_folder_the_agent_can_write_is_refused() {
    let rule = "may not name a folder the agent can write";
    let (w, target) = naming("commondir-tmp", "data/run/s1/tmp/common");
    let built = w.builder().tmpdir(w.at("data/run/s1/tmp")).build();
    refused(built, rule, &target);

    let (w, target) = naming("commondir-notes", "data/agent-state/m1/notes/common");
    let built = w.builder().notes(w.at("data/agent-state/m1/notes")).build();
    refused(built, rule, &target);

    let (w, target) = naming("commondir-allowed", "other/common");
    let built = w.builder().allowed_folder(w.at("other")).build();
    refused(built, rule, &target);

    let (w, target) = naming("commondir-state", "state/common");
    refused(w.builder().build(), rule, &target);
}

/// A `commondir` naming a folder outside the matter that the agent can't write needs no rule:
/// the jail denies it anyway.
#[test]
fn a_commondir_naming_an_unwritable_outside_folder_is_ignored() {
    let (w, target) = naming("commondir-other", "other/common");
    let policy = w.builder().build().unwrap();
    let rules = policy.read_only_in_matter();
    assert!(rules.iter().all(|rule| rule.path() != target), "{rules:?}");
}

/// A `commondir` beside a `HEAD` that is a link refuses the launch: git follows it, the walk
/// does not.
#[cfg(unix)]
#[test]
fn a_commondir_link_is_refused() {
    let w = World::new("commondir-link");
    w.dir.dir("matter/shared");
    w.dir.file("matter/real", "../shared\n");
    w.dir.file("matter/.git/HEAD", HEAD);
    w.dir.symlink("matter/.git/commondir", Path::new("../real"));
    let link = canon(&w.at("matter")).join(".git/commondir");
    refused(w.builder().build(), "commondir may not be a link", &link);
}
