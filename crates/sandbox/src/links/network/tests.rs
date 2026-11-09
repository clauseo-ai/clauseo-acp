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
        r"\\.\UNC\host\share",
        "//./unc/host/share",
    ] {
        assert_eq!(place(path, &none), Place::Off(Off::Network), "{path}");
    }
    for path in [
        r"\\.\pipe\x",
        r"\\?\Volume{0}\x",
        r"\??\Volume{0}\x",
        r"\\.\PhysicalDrive0",
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
    let mounts = Mounts::new(Vec::new(), b"z");
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
    for path in [r"C:\repo", r"\\?\C:\repo"] {
        assert_eq!(
            place(path, &mounts),
            Place::Local(PathBuf::from(path)),
            "{path}"
        );
    }
}

/// A `\\.\` device path on a drive letter is that drive's path, as Win32 reads it: local on a
/// local drive, network on a network one.
#[test]
fn a_device_path_on_a_drive_letter_is_its_drive() {
    let mounts = Mounts::new(Vec::new(), b"Z");
    assert_eq!(
        place(r"\\.\C:\repo\..\x", &mounts),
        Place::Local(PathBuf::from(r"C:\repo\..\x"))
    );
    assert_eq!(
        place("//./c:/x", &mounts),
        Place::Local(PathBuf::from("c:/x"))
    );
    assert_eq!(place(r"\\.\Z:\x", &mounts), Place::Off(Off::Network));
}

/// A local `subst` drive is its mapping, one level at a time: onto a local disk it is
/// local (the mapped path), onto a network drive it is network, and a chain past the cap (or a
/// loop) is unknown.
#[test]
fn a_subst_drive_is_what_it_maps_to() {
    let subst = |to: &str| Drive::Subst(to.to_owned());
    let mounts = Mounts::default()
        .with_drive(b'X', subst(r"C:\work\state\tmp"))
        .with_drive(b'Y', subst(r"X:\deeper"))
        .with_drive(b'N', subst(r"Z:\x"))
        .with_drive(b'Z', Drive::Network)
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

fn refused(matter: &str, mounts: &Mounts) -> bool {
    match check_matter_local(Path::new(matter), mounts) {
        Ok(()) => false,
        Err(Error::MatterOnNetwork) => true,
        Err(other) => panic!("{other:?}"),
    }
}

/// A matter on a Windows network drive refuses: a mapped drive, a server path, a verbatim server
/// path, and a `subst` onto a share, directly or through another `subst`. A local one doesn't.
#[test]
fn a_matter_on_a_windows_network_drive_refuses() {
    let mounts = Mounts::new(Vec::new(), b"Z")
        .with_drive(b'C', Drive::Local)
        .with_drive(b'Y', Drive::Network)
        .with_drive(b'X', Drive::Subst(r"Y:\work".to_owned()))
        .with_drive(b'W', Drive::Subst(r"C:\work".to_owned()));
    for matter in [
        r"Z:\matters\m",
        r"z:\m",
        r"\\server\share\m",
        "//server/share/m",
        r"\\?\UNC\server\share\m",
        r"\??\UNC\server\share\m",
        r"Y:\m",
        r"X:\m",
        r"\\?\Z:\m",
    ] {
        assert!(refused(matter, &mounts), "{matter}");
    }
    for matter in [r"C:\matters\m", r"W:\m", r"D:\m", r"\\?\C:\m"] {
        assert!(!refused(matter, &mounts), "{matter}");
    }
}

/// A matter on a macOS network mount refuses, by the local-fs allowlist; so does a matter
/// holding one. A local matter beside them doesn't.
#[test]
fn a_matter_on_or_holding_a_macos_network_mount_refuses() {
    for kind in ["smbfs", "nfs", "afpfs", "webdav"] {
        let mounts = Mounts::of_kinds(&[("/", "apfs"), ("/Volumes/share", kind)]);
        assert!(refused("/Volumes/share/m", &mounts), "{kind}");
        assert!(refused("/Volumes/share", &mounts), "{kind}");
        assert!(!refused("/Users/a/m", &mounts), "{kind}");
        let inner = Mounts::of_kinds(&[("/", "apfs"), ("/Users/a/m/inner", kind)]);
        assert!(refused("/Users/a/m", &inner), "{kind}");
        assert!(!refused("/Users/a/other", &inner), "{kind}");
    }
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
    for kind in ["lifs", "ntfs", "udf", "cd9660"] {
        assert!(LOCAL_FS.contains(&kind), "{kind}");
    }
    for kind in [
        "smbfs", "nfs", "afpfs", "webdav", "autofs", "macfuse", "osxfuse", "nullfs", "bindfs", "",
    ] {
        assert!(Mounts::is_network_fs(kind), "{kind}");
    }
}

/// Local media mounted inside a matter (a USB stick, an NTFS disk, a DVD) don't refuse it.
#[test]
fn local_media_inside_a_matter_do_not_refuse_it() {
    for kind in [
        "apfs", "hfs", "msdos", "exfat", "lifs", "ntfs", "udf", "cd9660",
    ] {
        let mounts = Mounts::of_kinds(&[("/", "apfs"), ("/Users/a/m/media", kind)]);
        assert!(!refused("/Users/a/m", &mounts), "{kind}");
        assert!(!net("/Users/a/m/media/.git", &mounts), "{kind}");
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

/// A fake `read`: `links` (link, target as written), `errors` (path, kind); any other path is
/// not a link.
fn reader<'a>(
    links: &'a [(&str, &str)],
    errors: &'a [(&str, io::ErrorKind)],
) -> impl Fn(&Path) -> io::Result<Option<PathBuf>> + 'a {
    move |path: &Path| {
        let text = path.to_string_lossy();
        assert!(
            !text.starts_with("/Volumes/share") && !text.starts_with("//"),
            "a network path was looked at: {text}"
        );
        if let Some((_, kind)) = errors.iter().find(|(at, _)| Path::new(at) == path) {
            return Err(io::Error::from(*kind));
        }
        Ok(links
            .iter()
            .find(|(link, _)| Path::new(link) == path)
            .map(|(_, target)| PathBuf::from(target)))
    }
}

fn share() -> Mounts {
    Mounts::new(vec![(PathBuf::from("/Volumes/share"), true)], &[])
}

/// Windows takes `..` off by text before it opens a path, so `J/../N/x` opens `N/x` whatever
/// `J` links to: the check does the same, and finds `N`'s link to the share. The kernel's way
/// (elsewhere) steps back from `J`'s target and finds no `N` there.
#[test]
fn on_windows_dots_go_by_text_before_any_link_is_followed() {
    let read = reader(&[("/m/J", "/far/deep"), ("/m/N", "//host/share")], &[]);
    let path = Path::new("/m/J/../N/x");
    let by_text = link_to_network_as(path, &share(), &read, true);
    assert_eq!(by_text, Some((PathBuf::from("//host/share"), Off::Network)));
    assert_eq!(link_to_network_as(path, &share(), &read, false), None);
}

/// A relative link target is joined to the link's folder and its `..` taken off by text too.
#[test]
fn on_windows_a_relative_target_drops_its_dots_by_text() {
    let read = reader(
        &[("/m/a", "sub/../../Volumes/share/x"), ("/m/sub", "/far")],
        &[],
    );
    let got = link_to_network_as(Path::new("/m/a/y"), &share(), &read, true);
    assert_eq!(
        got,
        Some((PathBuf::from("/Volumes/share/x/y"), Off::Network))
    );
}

/// The same on Windows paths, on Windows.
#[cfg(windows)]
#[test]
fn a_junction_then_dots_on_windows_paths_is_caught() {
    let mounts = Mounts::new(Vec::new(), &[]).with_drive(b'C', Drive::Local);
    let read = reader(
        &[(r"C:\m\J", r"C:\far\deep"), (r"C:\m\N", r"\\host\share")],
        &[],
    );
    let got = link_to_network(Path::new(r"C:\m\J\..\N\x"), &mounts, &read);
    assert_eq!(got, Some((PathBuf::from(r"\\host\share"), Off::Network)));
}

/// Only a missing name ends a chain as local; any other failed look is unknown, never local.
#[test]
fn only_a_missing_name_ends_a_chain() {
    let errors = [
        ("/m/gone", io::ErrorKind::NotFound),
        ("/m/denied", io::ErrorKind::PermissionDenied),
        ("/m/odd", io::ErrorKind::Other),
    ];
    let read = reader(&[], &errors);
    for dots_by_text in [false, true] {
        let gone = link_to_network_as(Path::new("/m/gone/x"), &share(), &read, dots_by_text);
        assert_eq!(gone, None);
        for name in ["denied", "odd"] {
            let path = PathBuf::from("/m").join(name);
            let got = link_to_network_as(&path.join("x"), &share(), &read, dots_by_text);
            assert_eq!(got, Some((path, Off::Unknown)), "{name}");
        }
    }
}

/// A chain longer than the kernel follows is unknown, never local: one more hop could reach a
/// share. Windows follows 63 reparse points, so its cap is 63.
#[test]
fn a_chain_past_the_hop_cap_is_unknown() {
    let names: Vec<(String, String)> = (0..=HOPS_MAX)
        .map(|at| (format!("/m/l{at}"), format!("/m/l{}", at + 1)))
        .collect();
    let links: Vec<(&str, &str)> = names
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let read = reader(&links, &[]);
    for dots_by_text in [false, true] {
        let got = link_to_network_as(Path::new("/m/l0"), &share(), &read, dots_by_text);
        assert!(matches!(got, Some((_, Off::Unknown))), "{got:?}");
    }
    #[cfg(windows)]
    assert_eq!(HOPS_MAX, 63);
}

#[test]
fn lexical_keeps_leading_dots_of_a_relative_path() {
    assert_eq!(lexical(Path::new("../a/../../b")), Path::new("../../b"));
    assert_eq!(lexical(Path::new("a/../..")), Path::new(".."));
}

/// Win32 hands `\\?\` and `\??\` paths to the kernel with `..` as written, after any link
/// before it, so a `..` there is unknown, never taken off by text. A plain path's is.
#[test]
fn dots_in_a_verbatim_path_are_unknown() {
    let mounts = Mounts::default().with_drive(b'C', Drive::Local);
    for path in [
        r"\\?\C:\m\N\..\x",
        r"\??\C:\m\N\..\x",
        r"\\?\C:/m/N/../x",
        r"\??\UNC\h\..\s",
    ] {
        assert_eq!(place(path, &mounts), Place::Off(Off::Unknown), "{path}");
        let read = reader(&[], &[]);
        for dots_by_text in [false, true] {
            let got = link_to_network_as(Path::new(path), &mounts, &read, dots_by_text);
            assert_eq!(got, Some((PathBuf::from(path), Off::Unknown)), "{path}");
        }
    }
    assert!(matches!(place(r"\\?\C:\m\x", &mounts), Place::Local(_)));
}
