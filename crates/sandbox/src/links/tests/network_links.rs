//! Links to network paths: found as leading outside, never resolved or followed.

use super::*;

fn found(link: &str, target: &str) -> LinkFinding {
    LinkFinding {
        path: PathBuf::from(link),
        kind: LinkKind::Symlink,
        target: Some(PathBuf::from(target)),
    }
}

/// A link to a server or device path, a remote drive or a network mount, directly or through
/// another link (to it or to a folder on the way), leads outside, to that path, with no file
/// call on it and no canonicalize.
#[test]
fn a_link_to_a_network_path_leads_outside_unresolved() {
    let matter = Path::new("/m");
    let mounts = Mounts::new(vec![(PathBuf::from("/Volumes/share"), true)], b"Z");
    let read = fake(&[
        ("/m/unc", r"\\host\share"),
        ("/m/slashes", "//host/share"),
        ("/m/verbatim", r"\\?\UNC\host\share"),
        ("/m/device", r"\\.\pipe\x"),
        ("/m/drive", r"Z:\repo"),
        ("/m/mount", "/Volumes/share/repo"),
        ("/m/hop", "slashes"),
        ("/m/up", "../m/sub/x"),
        ("/m/sub", "/Volumes/share"),
        ("/m/nt", r"\??\UNC\host\share"),
        ("/m/vol", r"\\?\Volume{0}\x"),
        ("/m/junction", r"\??\C:\m\N\..\x"),
    ]);
    for (link, target) in [
        ("/m/unc", r"\\host\share"),
        ("/m/slashes", "//host/share"),
        ("/m/verbatim", r"\\?\UNC\host\share"),
        ("/m/device", r"\\.\pipe\x"),
        ("/m/drive", r"Z:\repo"),
        ("/m/mount", "/Volumes/share/repo"),
        ("/m/hop", "//host/share"),
        ("/m/up", "/Volumes/share"),
        ("/m/nt", r"\??\UNC\host\share"),
        ("/m/vol", r"\\?\Volume{0}\x"),
        ("/m/junction", r"\??\C:\m\N\..\x"),
    ] {
        let got = symlink_finding_with(
            matter,
            Path::new(link),
            LinkKind::Symlink,
            &mounts,
            &read,
            never,
        );
        assert_eq!(got, Some(found(link, target)), "{link}");
    }
}

/// A link to a local path is resolved as before: inside the matter it is no finding.
#[test]
fn a_link_to_a_local_path_is_resolved() {
    let matter = Path::new("/m");
    let read = fake(&[("/m/in", "real")]);
    let resolve = |_: &Path| Ok(PathBuf::from("/m/real"));
    let got = symlink_finding_with(
        matter,
        Path::new("/m/in"),
        LinkKind::Symlink,
        &Mounts::default(),
        &read,
        resolve,
    );
    assert_eq!(got, None);
}

/// The walk finds a link to `//host/share` as leading there.
#[cfg(unix)]
#[test]
fn the_walk_finds_a_link_to_a_share_unresolved() {
    let dir = TempDir::new("links-unc-link");
    dir.dir("matter");
    dir.symlink("matter/share", Path::new("//host/share"));
    let matter = canon(&dir.path().join("matter"));
    let scan = scan_matter(&matter).unwrap();
    assert_eq!(
        scan.links(),
        [LinkFinding {
            path: matter.join("share"),
            kind: LinkKind::Symlink,
            target: Some(PathBuf::from("//host/share")),
        }]
    );
}
