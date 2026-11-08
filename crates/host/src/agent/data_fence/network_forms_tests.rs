//! Every network form is refused with nothing looked at, as a path an agent names and as a
//! link's target: Windows' device and UNC spellings, mixed slashes, a remote drive, macOS's
//! other texts for a mount and its firmlinks, Linux's automounts and FUSE mounts, a wildcard on
//! any of them. Then the folds a deny applies (`ı`, `İ`, `ſ`, the Kelvin sign) and the data
//! folder found by its identity.

use std::path::{Path, PathBuf};

use super::network_tests::{fence, look, refused, windows_net};
use crate::network_paths::NetEnv;
use crate::network_paths::resolve::tests::{Fake, FakeFs};
use crate::network_paths::tests::{mac_net, plain_net};

/// Linux's rules with the mounts of a table that has an autofs and a gvfs mount.
fn linux_net() -> NetEnv {
    let table = b"/dev/sda1 / ext4 rw 0 0\n\
systemd-1 /auto autofs rw 0 0\n\
gvfsd-fuse /run/user/1000/gvfs fuse.gvfsd-fuse rw 0 0\n\
rclone: /mnt/drive fuse.rclone rw 0 0\n";
    NetEnv {
        remote_mounts: crate::network_paths::tests::linux_mounts(table),
        ..plain_net(false)
    }
}

/// Each network form with the rules it is read under.
fn forms() -> Vec<(NetEnv, &'static str)> {
    let mut forms: Vec<(NetEnv, &str)> = Vec::new();
    for path in [
        r"\\server\share\x.docx",
        r"\\?\UNC\server\share\x",
        r"\??\UNC\server\share\x",
        r"\\.\UNC\server\share\x",
        r"\\?\GLOBALROOT\Device\Mup\server\share\x",
        r"\\localhost\share\x",
        r"\\localhost\c$\x",
        r"\\127.0.0.1\share\x",
        r"\\127.0.0.1\c$\x",
        r"//server\share/x",
        r"\\server/share\x",
        "//server/share/x",
        r"Z:\x.docx",
        r"z:/x",
        r"\\?\Z:\x",
        r"\\server\share\*.docx",
        r"Z:\*.docx",
    ] {
        forms.push((windows_net(), path));
    }
    for path in [
        "/Volumes/smb/x",
        "/Volumes/smb/*",
        "/.nofollow/Volumes/smb/x",
        "/.NoFollow/x",
        "/.re\u{17F}olve/1/2/x",
        "/.resolve/1/2",
        "/.vol/16777220/2",
        "/System/Volumes/Data/Volumes/smb/x",
        "/system/volumes/data/volumes/SMB",
        "/System/Volumes/Data/home/key/x",
        "/.nofollow/System/Volumes/Data/home/key",
        "/home/key/x",
        "/net/server/x",
        "/Network/Servers/x",
        "/Netwo\u{72}\u{212A}/Servers/x",
    ] {
        forms.push((mac_net(&["/Volumes/smb"]), path));
    }
    for path in [
        "/auto/home/x",
        "/run/user/1000/gvfs/smb-share:server=s,share=x/y",
        "/mnt/drive/x",
    ] {
        forms.push((linux_net(), path));
    }
    forms
}

/// The only paths a check from the matter `/m` with the data folder `/d` may look at.
fn only_own(fs: &FakeFs) -> Vec<PathBuf> {
    fs.touched()
        .into_iter()
        .filter(|path| {
            !["/", "/d", "/d/profile", "/m"]
                .iter()
                .any(|own| path == Path::new(own))
        })
        .collect()
}

#[test]
fn every_network_form_is_refused_with_nothing_looked_at() {
    for (net, path) in forms() {
        let fs = FakeFs::default();
        assert!(refused(&fence("/m", &[path]), &net, &fs), "{path}");
        assert!(!look(Path::new("/m"), Path::new(path), &net, &fs), "{path}");
        assert_eq!(only_own(&fs), Vec::<PathBuf>::new(), "{path}");
    }
}

/// A link whose target is any network form is read, never followed: under each form's own
/// rules, and under Windows' (a junction needs no privilege there, and its target is joined and
/// its `..` dropped by text).
#[cfg(unix)]
#[test]
fn a_link_to_any_network_form_stops_at_the_link() {
    let windows = forms().into_iter().map(|(net, target)| {
        (
            NetEnv {
                windows: true,
                fold_case: true,
                ..net
            },
            target,
        )
    });
    for (net, target) in forms().into_iter().chain(windows) {
        let fs = FakeFs::with(&[("/m/link", Fake::Link(target.into()))]);
        assert!(
            refused(&fence("/m", &["/m/link/doc.docx"]), &net, &fs),
            "{target}"
        );
        let past: Vec<PathBuf> = only_own(&fs)
            .into_iter()
            .filter(|path| path != Path::new("/m/link"))
            .collect();
        assert!(past.is_empty(), "{target}: {past:?}");
    }
}

/// Every loopback or self form is UNC, refused as an agent's text and as a link's target: the
/// SMB client would go over loopback. No localhost case is special, so the machine's own name and
/// its FQDN need no lookup: every `\\host` is refused.
#[test]
fn every_loopback_and_self_form_is_refused_as_unc() {
    let net = windows_net();
    let forms = [
        r"\\localhost\c$\Users\a",
        r"\\LOCALHOST\share\x",
        r"\\127.0.0.1\c$\x",
        r"\\127.10.20.30\c$\x",
        r"\\::1\c$\x",
        r"\\[::1]\c$\x",
        r"\\0.0.0.0\c$\x",
        r"\\MYPC\c$\x",
        r"\\mypc.corp.example.com\c$\x",
        r"\\?\UNC\localhost\c$\x",
        r"\\.\UNC\127.0.0.1\c$\x",
        r"\\0--1.ipv6-literal.net\c$\x",
        "//localhost/c$/x",
    ];
    for path in forms {
        let fs = FakeFs::default();
        assert!(refused(&fence("/m", &[path]), &net, &fs), "{path}");
        assert!(!look(Path::new("/m"), Path::new(path), &net, &fs), "{path}");
        assert_eq!(only_own(&fs), Vec::<PathBuf>::new(), "{path}");
    }
    #[cfg(unix)]
    for target in forms {
        let fs = FakeFs::with(&[("/m/link", Fake::Link(target.into()))]);
        assert!(refused(&fence("/m", &["/m/link/x"]), &net, &fs), "{target}");
        let past: Vec<PathBuf> = only_own(&fs)
            .into_iter()
            .filter(|path| path != Path::new("/m/link"))
            .collect();
        assert!(past.is_empty(), "{target}: {past:?}");
    }
}

/// An 8.3 short name is walked: on a local drive it is put in its long form by the walk's
/// canonical step, and a name with `~` and a digit is an ordinary name; an alias of the data
/// folder, or of a folder in it, is still caught (by the long form, or by identity).
#[test]
fn a_short_name_is_walked_and_an_alias_of_the_data_folder_still_caught() {
    let net = windows_net();
    let fs = FakeFs::default();
    for path in [
        r"C:\Users\JONATH~1\AppData\Local\Temp\x",
        r"docs\Draft~2.docx",
    ] {
        assert!(!refused(&fence("/m", &[path]), &net, &fs), "{path}");
    }
    // The short name `/u/CLAUSE~1` of the data folder `/d`, and `/d/MATTER~1` of a folder in
    // it: the canonical step gives their long forms.
    let fs = FakeFs::with(&[("/u/CLAUSE~1/x", Fake::File), ("/d/MATTER~1/x", Fake::File)]);
    let real = |path: &Path| match path.to_str() {
        Some("/u/CLAUSE~1") => Some(PathBuf::from("/d")),
        Some("/d/MATTER~1") => Some(PathBuf::from("/d/matters")),
        _ => None,
    };
    let unix = plain_net(false);
    let disk = super::resolve::Disk {
        fs: &fs,
        real: &real,
        net: &unix,
    };
    for path in ["/u/CLAUSE~1/x", "/d/MATTER~1/x"] {
        assert!(fence("/m", &[path]).touches_data_on(false, &disk), "{path}");
    }
    // With no canonical step, identity still finds the alias.
    let mut fs = FakeFs::with(&[("/u/CLAUSE~1/x", Fake::File)]);
    fs.ids.insert(PathBuf::from("/d"), (3, 9));
    fs.ids.insert(PathBuf::from("/u/CLAUSE~1"), (3, 9));
    assert!(refused(&fence("/m", &["/u/CLAUSE~1/x"]), &unix, &fs));
}

/// The firmlinks put a data volume path in its root form; when they can't be read, every data
/// volume path is refused.
#[test]
fn a_data_volume_path_takes_its_firmlinked_form() {
    let fs = FakeFs::default();
    let known = mac_net(&["/Volumes/smb"]);
    assert!(!refused(
        &fence("/m", &["/System/Volumes/Data/Users/a/x"]),
        &known,
        &fs
    ));
    let unknown = NetEnv {
        firmlinks: None,
        ..mac_net(&[])
    };
    assert!(refused(
        &fence("/m", &["/System/Volumes/Data/Users/a/x"]),
        &unknown,
        &fs
    ));
    assert!(!refused(&fence("/m", &["/Users/a/x"]), &unknown, &fs));
}

/// A deny folds `ı`, `İ`, `ſ` and the Kelvin sign as NTFS and APFS do; the share matter's
/// exception, an allow, never does.
#[test]
fn a_deny_folds_dotless_and_dotted_i_and_an_allow_does_not() {
    let fs = FakeFs::default();
    let net = plain_net(false);
    let real = |_: &Path| None;
    let disk = super::resolve::Disk {
        fs: &fs,
        real: &real,
        net: &net,
    };
    let check = |data: &str, path: &str| {
        let mut check = fence("/m", &[path]);
        check.data = PathBuf::from(data);
        check.touches_data_on(true, &disk)
    };
    assert!(check("/u/.git", "/u/.g\u{131}t/config"));
    assert!(check("/u/.git", "/u/.G\u{130}t/config"));
    assert!(check("/u/.clauseo", "/u/.clau\u{17F}eo/x"));
    assert!(check("/u/.kdata", "/u/.\u{212A}data/x"));
    let share = windows_net();
    for (matter, path) in [
        (r"\\srv\share\.git", r"\\srv\share\.g\u{131}t\x"),
        (r"\\srv\share\.git", r"\\srv\share\.G\u{130}t\x"),
        (r"\\srv\share\.clauseo", r"\\srv\share\.clau\u{17F}eo\x"),
        (r"\\srv\share\.kdata", r"\\srv\share\.\u{212A}data\x"),
    ] {
        assert!(refused(&fence(matter, &[path]), &share, &fs), "{path}");
    }
}

/// A local path is in the data folder when it or a folder above it is the data folder by
/// identity, whatever its text.
#[test]
fn a_path_into_the_data_folder_by_another_spelling_is_refused_by_identity() {
    let mut fs = FakeFs::with(&[("/alias/x", Fake::File), ("/other/x", Fake::File)]);
    fs.ids.insert(PathBuf::from("/d"), (1, 7));
    fs.ids.insert(PathBuf::from("/alias"), (1, 7));
    fs.ids.insert(PathBuf::from("/other"), (1, 8));
    let net = plain_net(false);
    assert!(refused(&fence("/m", &["/alias/x"]), &net, &fs));
    assert!(!refused(&fence("/m", &["/other/x"]), &net, &fs));
    let mut search = fence("/m", &[]);
    search.roots = vec![PathBuf::from("/alias")];
    assert!(refused(&search, &net, &fs));
}

/// A name Windows reads as another place (a trailing dot, a stream, an 8.3 name, a device) is
/// refused from the text; a wildcard is not.
#[test]
fn a_windows_name_read_as_another_place_is_refused_untouched() {
    let net = windows_net();
    for path in [r"docs\x.docx.", r"docs\x:y", r"docs\CON.txt"] {
        let fs = FakeFs::default();
        assert!(refused(&fence("/m", &[path]), &net, &fs), "{path}");
        assert!(!look(Path::new("/m"), Path::new(path), &net, &fs), "{path}");
        assert_eq!(only_own(&fs), Vec::<PathBuf>::new(), "{path}");
    }
}
