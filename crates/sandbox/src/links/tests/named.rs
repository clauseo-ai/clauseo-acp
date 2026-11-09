//! Where a `gitdir:` or `commondir` line leads: classified as written, then name by name and
//! link by link, and resolved only once every step is local.

use super::*;

/// A resolver that answers by text alone.
const ECHO: fn(&Path) -> io::Result<PathBuf> = |path| Ok(network::lexical(path));

fn mounts() -> Mounts {
    Mounts::new(vec![(PathBuf::from("/Volumes/share"), true)], b"Z")
        .with_drive(b'V', network::Drive::Unknown)
        .with_drive(b'X', network::Drive::Subst("C:/data/run/s1/tmp".to_owned()))
}

/// A server, device or network-drive path as written, or one that reaches a network mount once
/// joined to its folder, is skipped before any look at it or resolve.
#[test]
fn a_network_path_is_skipped_before_any_file_call() {
    let matter = Path::new("/m");
    let base = Path::new("/m/.git/worktrees/wt");
    let read = fake(&[]);
    for line in [
        r"\\host\share\repo",
        "//host/share/repo",
        r"\\?\UNC\host\share",
        r"\??\UNC\host\share",
        r"Z:\repo\.git",
        r"\??\Z:\repo",
        "/Volumes/share/repo",
        "../../../../Volumes/share/repo",
    ] {
        assert_eq!(
            named_with(matter, base, line, &mounts(), &read, never),
            None,
            "{line}"
        );
    }
}

/// A path that reaches the network through a link in the matter (`share` links to a server) is
/// skipped: the walk classifies every link on the way before it resolves anything.
#[test]
fn a_path_through_a_link_to_the_network_is_skipped() {
    let matter = Path::new("/m");
    let base = Path::new("/m/.git");
    for target in [
        r"\\host\share",
        "//host/share",
        r"\??\UNC\host\share",
        "/Volumes/share",
    ] {
        let read = fake(&[("/m/share", target)]);
        let got = named_with(matter, base, "../share/x", &mounts(), &read, never);
        assert_eq!(got, None, "{target}");
    }
}

/// A path on a device the launch doesn't know, as written or through a link, is unknown, and
/// never resolved: the policy refuses the launch.
#[test]
fn a_path_on_an_unknown_device_is_unknown() {
    let matter = Path::new("/m");
    let base = Path::new("/m/.git");
    let read = fake(&[("/m/vera", r"V:\repo")]);
    for (line, want) in [
        (r"V:\repo", r"V:\repo"),
        (r"\??\V:\repo", r"\??\V:\repo"),
        (r"\\.\pipe\x", r"\\.\pipe\x"),
        ("../vera/x", r"V:\repo"),
    ] {
        let got = named_with(matter, base, line, &mounts(), &read, never);
        assert_eq!(got, Some(Named::Unknown(PathBuf::from(want))), "{line}");
    }
}

/// A local `subst` drive leads where it maps to: here into the session's temp folder, which
/// the policy then refuses as agent-writable (`policy::tests::git_dirs`).
#[test]
fn a_subst_drive_leads_where_it_maps() {
    let matter = Path::new("/m");
    let base = Path::new("/m/.git");
    let got = named_with(matter, base, "X:/repo", &mounts(), fake(&[]), ECHO);
    assert_eq!(
        got,
        Some(Named::Outside(PathBuf::from("C:/data/run/s1/tmp/repo")))
    );
}

/// A local path goes to the resolver, as does one on the matter's own network mount (the walk
/// reads that volume anyway).
#[test]
fn a_local_path_is_resolved() {
    let matter = Path::new("/m");
    let base = Path::new("/m/.git/worktrees/wt");
    let none = Mounts::default();
    assert_eq!(
        named_with(matter, base, "../../../shared", &none, fake(&[]), ECHO),
        Some(Named::Inside(PathBuf::from("/m/shared")))
    );
    assert_eq!(
        named_with(
            matter,
            base,
            "../../../../elsewhere",
            &none,
            fake(&[]),
            ECHO
        ),
        Some(Named::Outside(PathBuf::from("/elsewhere")))
    );
    let on_share = Mounts::new(vec![(PathBuf::from("/m"), true)], &[]).trusting(matter);
    assert_eq!(
        named_with(matter, base, "../../../shared", &on_share, fake(&[]), ECHO),
        Some(Named::Inside(PathBuf::from("/m/shared")))
    );
}

/// The walk skips a network `gitdir:` and network `commondir` files unread, in a `.git` or
/// beside a `HEAD` outside one.
#[test]
fn the_walk_lists_nothing_a_network_path_names() {
    let dir = TempDir::new("links-commondir-unc");
    dir.file("matter/a/.git", "gitdir: //host/share/repo\n");
    dir.file("matter/.git/HEAD", "ref: refs/heads/main\n");
    dir.file("matter/.git/commondir", "\\\\host\\share\n");
    dir.file("matter/b/HEAD", "ref: refs/heads/main\n");
    dir.file("matter/b/commondir", "\\\\?\\UNC\\host\\share\n");
    let scan = scan_matter(&dir.path().join("matter")).unwrap();
    assert!(scan.git_dirs().is_empty(), "{:?}", scan.git_dirs());
    assert!(
        scan.git_dirs_outside.is_empty(),
        "{:?}",
        scan.git_dirs_outside
    );
    assert!(
        scan.git_dirs_unknown.is_empty(),
        "{:?}",
        scan.git_dirs_unknown
    );
}

/// A network mount inside the matter is never entered: nothing under it is listed.
#[test]
fn the_walk_never_enters_a_network_mount_in_the_matter() {
    let dir = TempDir::new("links-inner-mount");
    dir.dir("matter/share/.git");
    dir.file("matter/share/CLAUDE.md", "c");
    dir.dir("matter/local/.git");
    let matter = canon(&dir.path().join("matter"));
    let mounts = Mounts::new(vec![(matter.join("share"), true)], &[]);
    let scan = scan_matter_with(&matter, mounts, GIT_DIRS_MAX).unwrap();
    assert_eq!(scan.git(), [matter.join("local/.git")]);
    assert!(scan.nested().is_empty(), "{:?}", scan.nested());
}

/// Past the cap of git dirs, the walk refuses the launch.
#[test]
fn the_walk_refuses_past_the_git_dir_cap() {
    let dir = TempDir::new("links-cap");
    for repo in ["a", "b", "c"] {
        dir.file(
            &format!("matter/{repo}/.git/HEAD"),
            "ref: refs/heads/main\n",
        );
    }
    let matter = dir.path().join("matter");
    assert!(scan_matter_with(&matter, Mounts::default(), 3).is_ok());
    let refused = scan_matter_with(&matter, Mounts::default(), 2);
    assert!(
        matches!(&refused, Err(Error::Policy(m)) if m.contains("more than 2 git folders")),
        "{refused:?}"
    );
}
