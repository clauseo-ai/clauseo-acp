use super::*;

fn net(path: &str, mounts: &Mounts) -> bool {
    is_network_path(Path::new(path), mounts)
}

fn place(path: &str, mounts: &Mounts) -> Place {
    classify(Path::new(path), mounts)
}

#[test]
fn server_and_device_paths_are_not_touched_by_their_text() {
    let none = Mounts::default();
    for path in [
        r"\\host\share",
        r"\\host\share\repo\.git",
        "//host/share/repo",
        r"\\?\UNC\host\share",
        r"\??\UNC\host\share",
        r"\??\unc\host\share",
        r"/\host\share",
    ] {
        assert_eq!(place(path, &none), Place::Off(Off::Network), "{path}");
    }
    for path in [
        r"\\.\pipe\x",
        r"\\?\Volume{0}\x",
        r"\??\Volume{0}\x",
        r"\\.\C:\x",
    ] {
        assert_eq!(place(path, &none), Place::Off(Off::Unknown), "{path}");
    }
    for path in [
        r"\\?\C:\repo",
        r"\??\C:\repo",
        "/Users/a/repo",
        "repo/.git",
        r"C:\repo",
        "/",
    ] {
        assert!(!net(path, &none), "{path}");
    }
}

#[test]
fn a_drive_letter_is_what_its_device_is_in_any_case_and_form() {
    let mounts = Mounts::new(Vec::new(), b"z").with_drive(b'V', Drive::Unknown);
    for path in [
        r"Z:\repo",
        r"z:\repo",
        "Z:/repo",
        r"\\?\Z:\repo",
        r"\??\Z:\repo",
        "Z:repo",
    ] {
        assert_eq!(place(path, &mounts), Place::Off(Off::Network), "{path}");
    }
    for path in [r"V:\x", r"\??\v:\x"] {
        assert_eq!(place(path, &mounts), Place::Off(Off::Unknown), "{path}");
    }
    for path in [r"C:\repo", r"\\?\C:\repo"] {
        assert_eq!(
            place(path, &mounts),
            Place::Local(PathBuf::from(path)),
            "{path}"
        );
    }
}

/// A local `subst` drive is its mapping, one level at a time: onto a local disk it is
/// local (the mapped path), onto a network or unknown drive it is that, and a chain past the
/// cap (or a loop) is unknown.
#[test]
fn a_subst_drive_is_what_it_maps_to() {
    let subst = |to: &str| Drive::Subst(to.to_owned());
    let mounts = Mounts::default()
        .with_drive(b'X', subst(r"C:\work\state\tmp"))
        .with_drive(b'Y', subst(r"X:\deeper"))
        .with_drive(b'N', subst(r"Z:\x"))
        .with_drive(b'Z', Drive::Network)
        .with_drive(b'U', subst(r"V:\x"))
        .with_drive(b'V', Drive::Unknown)
        .with_drive(b'L', subst(r"L:\loop"))
        .with_drive(b'S', subst(r"C:\"));
    let local = |path: &str| Place::Local(PathBuf::from(path));
    assert_eq!(place(r"X:\repo", &mounts), local(r"C:\work\state\tmp\repo"));
    assert_eq!(place("X:repo", &mounts), local(r"C:\work\state\tmp\repo"));
    assert_eq!(
        place(r"\??\X:\repo", &mounts),
        local(r"C:\work\state\tmp\repo")
    );
    assert_eq!(
        place(r"Y:\repo", &mounts),
        local(r"C:\work\state\tmp\deeper\repo")
    );
    assert_eq!(place(r"S:\repo", &mounts), local(r"C:\repo"));
    assert_eq!(place(r"N:\repo", &mounts), Place::Off(Off::Network));
    assert_eq!(place(r"U:\repo", &mounts), Place::Off(Off::Unknown));
    assert_eq!(place(r"L:\repo", &mounts), Place::Off(Off::Unknown));
}

#[test]
fn the_longest_mount_point_prefix_decides() {
    let mounts = Mounts::new(
        vec![
            (PathBuf::from("/"), false),
            (PathBuf::from("/Volumes/share"), true),
            (PathBuf::from("/Volumes/share/local"), false),
            (PathBuf::from("/net"), true),
        ],
        &[],
    );
    assert!(net("/Volumes/share", &mounts));
    assert!(net("/Volumes/share/repo/.git", &mounts));
    assert!(net("/net/host/x", &mounts));
    assert!(!net("/Volumes/share/local/x", &mounts));
    assert!(!net("/Volumes/shared/x", &mounts));
    assert!(!net("/Users/a", &mounts));
    assert!(!net("/Users/a", &Mounts::default()));
}

/// The mount holding the matter counts local (the walk reads it anyway); one inside the matter
/// keeps its kind, and is listed as a folder the walk never enters.
#[test]
fn the_matters_own_mount_is_trusted_and_mounts_inside_it_are_not() {
    let mounts = Mounts::new(
        vec![
            (PathBuf::from("/"), false),
            (PathBuf::from("/Volumes/share"), true),
            (PathBuf::from("/Volumes/share/m/inner"), true),
            (PathBuf::from("/Volumes/other"), true),
        ],
        &[],
    )
    .trusting(Path::new("/Volumes/share/m"));
    assert!(!net("/Volumes/share/m/.git", &mounts));
    assert!(net("/Volumes/share/m/inner/x", &mounts));
    assert!(net("/Volumes/other/x", &mounts));
    assert_eq!(
        mounts.network_inside(Path::new("/Volumes/share/m")),
        [PathBuf::from("/Volumes/share/m/inner")]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn on_macos_a_mount_point_matches_in_any_letter_case() {
    let mounts = Mounts::new(vec![(PathBuf::from("/Volumes/Share"), true)], &[]);
    assert!(net("/volumes/share/x", &mounts));
}

/// Only the known local file systems are local; macFUSE, nullfs, bindfs and any other count as
/// network.
#[test]
fn only_known_local_file_systems_are_local() {
    for kind in LOCAL_FS {
        assert!(!Mounts::is_network_fs(kind), "{kind}");
    }
    for kind in [
        "smbfs", "nfs", "afpfs", "webdav", "autofs", "macfuse", "osxfuse", "nullfs", "bindfs", "",
    ] {
        assert!(Mounts::is_network_fs(kind), "{kind}");
    }
}

#[test]
fn lexical_drops_dots_and_never_climbs_past_the_root() {
    assert_eq!(lexical(Path::new("/a/b/../c/./d")), Path::new("/a/c/d"));
    assert_eq!(lexical(Path::new("/a/../../..")), Path::new("/"));
    assert_eq!(
        lexical(Path::new("/m/.git/worktrees/w/../../../../Volumes/s")),
        Path::new("/Volumes/s")
    );
}
