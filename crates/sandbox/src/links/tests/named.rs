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
/// joined to its folder, is unknown before any look at it or resolve: the policy refuses it.
#[test]
fn a_network_path_is_unknown_before_any_file_call() {
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
        r"\\?\C:\m\N\..\x",
        r"\??\C:\m\N\..\x",
    ] {
        let got = named_with(matter, base, line, &mounts(), &read, never);
        assert!(matches!(got, Some(Named::Unknown(_))), "{line}: {got:?}");
    }
}

/// A path that reaches the network through a link in the matter (`share` links to a server) is
/// unknown: the walk classifies every link on the way before it resolves anything.
#[test]
fn a_path_through_a_link_to_the_network_is_unknown() {
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
        assert!(matches!(got, Some(Named::Unknown(_))), "{target}: {got:?}");
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
#[cfg(windows)]
#[test]
fn a_subst_drive_leads_where_it_maps() {
    let matter = Path::new(r"C:\m");
    let base = Path::new(r"C:\m\.git");
    let mounts = Mounts::new(Vec::new(), b"Z")
        .with_drive(b'C', network::Drive::Local)
        .with_drive(
            b'X',
            network::Drive::Subst(r"C:\data\run\s1\tmp".to_owned()),
        );
    let got = named_with(matter, base, r"X:\repo", &mounts, fake(&[]), ECHO);
    assert_eq!(
        got,
        Some(Named::Outside(PathBuf::from(r"C:\data\run\s1\tmp\repo")))
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
}

/// The walk lists a network `gitdir:` and network `commondir` files as unknown, unread, in a
/// `.git` or beside a `HEAD` outside one: the policy refuses them.
#[test]
fn the_walk_lists_what_a_network_path_names_as_unknown() {
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
    assert_eq!(
        scan.git_dirs_unknown.len(),
        3,
        "{:?}",
        scan.git_dirs_unknown
    );
}

/// A matter written through a link above it to a network mount refuses before it is resolved.
#[cfg(unix)]
#[test]
fn the_walk_refuses_a_matter_through_a_link_to_a_share() {
    let dir = TempDir::new("links-matter-link");
    dir.symlink("Shares", Path::new("/Volumes/share"));
    let mounts = Mounts::new(vec![(PathBuf::from("/Volumes/share"), true)], &[]);
    let refused = scan_matter_with(&dir.path().join("Shares/m"), mounts, GIT_DIRS_MAX);
    assert!(
        matches!(refused, Err(Error::MatterOnNetwork)),
        "{refused:?}"
    );
}

/// `gitdir:` and `commondir` are read as git reads them: a `.git` file must start with exactly
/// `gitdir: `, and only trailing CR and LF are taken off, so a space stays part of the path.
#[test]
fn git_files_are_parsed_as_git_parses_them() {
    let dir = TempDir::new("links-git-parse");
    for name in ["crlf", "nospace", "indented", "common", "spaced"] {
        dir.dir(&format!("matter/{name}"));
    }
    dir.file("matter/a/.git", "gitdir: ../crlf\r\n");
    dir.file("matter/b/.git", "gitdir:../nospace\n");
    dir.file("matter/c/.git", "x\ngitdir: ../indented\n");
    dir.file("matter/d/HEAD", "ref: refs/heads/main\n");
    dir.file("matter/d/commondir", "../common\r\n");
    dir.file("matter/e/HEAD", "ref: refs/heads/main\n");
    dir.file("matter/e/commondir", "../spaced \n");
    let matter = canon(&dir.path().join("matter"));
    let scan = scan_matter(&matter).unwrap();
    assert_eq!(
        scan.git_dirs(),
        [matter.join("common"), matter.join("crlf")]
    );
}

/// A matter holding a network mount, or on one, refuses before the walk lists anything; the
/// same matter with only local mounts is walked. Not dependent on any other launch check.
#[test]
fn the_walk_refuses_a_matter_on_or_holding_a_network_mount() {
    let dir = TempDir::new("links-inner-mount");
    dir.dir("matter/share/.git");
    dir.file("matter/share/CLAUDE.md", "c");
    dir.dir("matter/local/.git");
    let matter = canon(&dir.path().join("matter"));
    let inner = Mounts::new(vec![(matter.join("share"), true)], &[]);
    let refused = scan_matter_with(&matter, inner, GIT_DIRS_MAX);
    assert!(
        matches!(refused, Err(Error::MatterOnNetwork)),
        "{refused:?}"
    );
    // On Windows a drive letter, not a mount point, says where the matter is.
    #[cfg(unix)]
    {
        let holder = Mounts::new(vec![(canon(dir.path()), true)], &[]);
        let refused = scan_matter_with(&matter, holder, GIT_DIRS_MAX);
        assert!(
            matches!(refused, Err(Error::MatterOnNetwork)),
            "{refused:?}"
        );
    }
    let local = Mounts::new(vec![(matter.join("share"), false)], &[]);
    let scan = scan_matter_with(&matter, local, GIT_DIRS_MAX).unwrap();
    assert_eq!(
        scan.git(),
        [matter.join("local/.git"), matter.join("share/.git")]
    );
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
