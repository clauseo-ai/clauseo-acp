//! A launch repository's `commondir` target outside its read-only rules: the profile denies
//! the names git reads there, or the whole path when nothing is there yet.

use super::*;
use crate::policy::git_aside::CommonDir;

#[test]
fn commondir_targets_outside_the_read_only_rules_are_denied_there() {
    let w = World::new("commondir");
    // A linked worktree's git folder naming the matter itself: the names, not the matter.
    w.dir.file("matter/wt/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/wt/commondir", "..\n");
    // One naming a path that doesn't exist yet: the path, whole.
    w.dir.file("matter/lone/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/lone/commondir", "../missing/store\n");
    // One naming a folder of a read-only repository: covered already.
    w.dir.file("matter/old.git/HEAD", "ref: refs/heads/main\n");
    w.dir.dir("matter/old.git/objects");
    w.dir.dir("matter/old.git/refs");
    w.dir.file("matter/linked/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/linked/commondir", "../old.git\n");
    let policy = w.builder().build().unwrap();
    let matter = canon(&w.at("matter"));
    let mut got = policy.git.commondir.clone();
    got.sort_by_key(|target| format!("{target:?}"));
    assert_eq!(
        got,
        [
            CommonDir::Folder(matter.clone()),
            CommonDir::Missing(matter.join("missing/store")),
        ]
    );
    let profile = crate::SeatbeltProfile::new(&policy, "clauseo-t-0").unwrap();
    let text = profile.text();
    assert!(text.contains("(literal (param \"COMMONDIR_"), "{text}");
    let denied: Vec<&PathBuf> = profile
        .params()
        .iter()
        .filter(|(name, _)| name.starts_with("COMMONDIR_"))
        .map(|(_, path)| path)
        .collect();
    for name in ["objects", "refs", "config", "hooks"] {
        assert!(denied.contains(&&matter.join(name)), "{name}: {denied:?}");
    }
    assert!(denied.contains(&&matter.join("missing/store")));
    assert!(
        !denied.contains(&&matter),
        "the matter itself stays writable"
    );
}

/// A `commondir` that is a symbolic link, dangling or not, refuses the build: git reads
/// `config` and `hooks` through it, and where it leads can change once the agent runs.
#[cfg(unix)]
#[test]
fn a_commondir_that_is_a_link_refuses_the_build() {
    for (name, target) in [("dangling", "../nowhere/commondir"), ("inside", "../real")] {
        let w = World::new(&format!("commondir-link-{name}"));
        w.dir.file("matter/real", "..\n");
        w.dir.file("matter/wt/HEAD", "ref: refs/heads/main\n");
        w.dir.symlink("matter/wt/commondir", Path::new(target));
        let error = w.builder().build().unwrap_err().to_string();
        assert!(
            error.contains("`commondir` may not be a symbolic link"),
            "{name}: {error}"
        );
    }
}

#[test]
fn a_matter_on_the_data_dirs_volume_needs_no_volume_set_aside() {
    let w = World::new("same-volume");
    let policy = w
        .builder()
        .hidden(w.at("data"))
        .git_set_aside(w.at("data/sandbox/git-set-aside"))
        .build()
        .unwrap();
    assert_eq!(policy.volume_set_aside(), None);
}
