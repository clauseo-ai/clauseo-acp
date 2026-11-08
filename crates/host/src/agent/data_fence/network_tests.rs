//! The fence never touches a network location an agent names: a UNC or device path, a remote
//! drive, a remote mount, or a local link to any of them counts as inside the data folder, so it
//! is refused, and the fake disk shows nothing at or past it was looked at. A path that lies, by
//! its text, in a matter folder the lawyer keeps on a share is outside and untouched; the text
//! rule is strict (separators, `.`, `..`, ASCII case only, one form).

use std::path::{Path, PathBuf};

use super::resolve::Disk;
use super::{FenceCheck, may_look};
use crate::network_paths::NetEnv;
#[cfg(unix)]
use crate::network_paths::resolve::tests::Fake;
use crate::network_paths::resolve::tests::FakeFs;
use crate::network_paths::tests::plain_net;

/// A request from the matter `cwd` naming `paths`, the data folder at `/d`.
fn fence(cwd: &str, paths: &[&str]) -> FenceCheck {
    FenceCheck {
        data: PathBuf::from("/d"),
        allowed: Vec::new(),
        cwd: PathBuf::from(cwd),
        home: None,
        paths: paths.iter().map(PathBuf::from).collect(),
        roots: Vec::new(),
        words: Vec::new(),
    }
}

/// Whether the fence refuses `check` on `fs` under `net`.
fn refused(check: &FenceCheck, net: &NetEnv, fs: &FakeFs) -> bool {
    let real = |_: &Path| None;
    let disk = Disk {
        fs,
        real: &real,
        net,
    };
    check.touches_data_on(false, &disk)
}

/// The paths `fs` was asked about that are on a server, a remote mount or a remote drive.
fn network_touched(fs: &FakeFs) -> Vec<PathBuf> {
    fs.touched()
        .into_iter()
        .filter(|path| {
            let text = path.to_string_lossy();
            ["srv", "evil", "server", "nfs", "Z:", "\\\\"]
                .iter()
                .any(|mark| text.contains(mark))
        })
        .collect()
}

/// Windows' rules with drive `Z:` a mapped share.
fn windows_net() -> NetEnv {
    NetEnv {
        remote_drive: Box::new(|letter| letter == 'Z'),
        ..plain_net(true)
    }
}

#[test]
fn a_unc_or_device_path_in_a_local_matter_is_refused_untouched() {
    let fs = FakeFs::default();
    for net in [plain_net(false), windows_net()] {
        for path in [
            r"\\server\share\brief.docx",
            r"\\?\UNC\server\share\brief.docx",
            r"\Device\Mup\server\share",
            r"\\.\UNC\server\share",
            r"\??\UNC\server\share",
            r"\\?\GLOBALROOT\Device\Mup\server",
        ] {
            assert!(refused(&fence("/m", &[path]), &net, &fs), "{path}");
            let held = may_look(Path::new("/m"), Path::new(path), &net, &fs);
            assert!(!held, "{path}");
        }
    }
    assert_eq!(network_touched(&fs), Vec::<PathBuf>::new());
}

/// Stage E's rule (7b8c6f12): a drive letter that `GetDriveTypeW` calls `DRIVE_REMOTE` on its
/// root is network, every path on it too, `\\?\Z:\` included; a local drive is walked.
#[test]
fn a_path_on_a_mapped_network_drive_is_refused_untouched() {
    let fs = FakeFs::default();
    let net = windows_net();
    for path in [r"Z:\Deals\spa.docx", r"z:\x", r"\\?\Z:\Deals"] {
        assert!(refused(&fence("/m", &[path]), &net, &fs), "{path}");
    }
    assert_eq!(network_touched(&fs), Vec::<PathBuf>::new());
}

#[test]
fn a_path_under_a_remote_mount_or_an_automount_folder_is_refused_untouched() {
    let fs = FakeFs::default();
    let net = NetEnv {
        remote_mounts: vec!["/mnt/nfs".into(), "/Volumes/share".into()],
        ..plain_net(false)
    };
    for path in [
        "/mnt/nfs/Deals/spa.docx",
        "/Volumes/share",
        "/net/server/x",
        "/Network/Servers/x",
    ] {
        assert!(refused(&fence("/m", &[path]), &net, &fs), "{path}");
        assert!(!may_look(Path::new("/m"), Path::new(path), &net, &fs));
    }
    assert_eq!(network_touched(&fs), Vec::<PathBuf>::new());
    let touched = fs.touched();
    assert!(
        !touched
            .iter()
            .any(|p| p.starts_with("/net") || p.starts_with("/Volumes/share"))
    );
}

/// A local link whose target is a UNC path (or a remote mount) is read, never followed: the
/// walk stops at it, and the path counts as inside.
#[cfg(unix)]
#[test]
fn a_link_to_a_network_target_is_refused_before_it_is_followed() {
    let net = NetEnv {
        remote_mounts: vec!["/mnt/nfs".into()],
        ..plain_net(false)
    };
    for target in [
        r"\\server\share",
        r"\\?\UNC\server\share",
        "/mnt/nfs/x",
        "/net/server",
    ] {
        let fs = FakeFs::with(&[("/m/link", Fake::Link(target.into()))]);
        assert!(
            refused(&fence("/m", &["/m/link/doc.docx"]), &net, &fs),
            "{target}"
        );
        let past: Vec<PathBuf> = fs
            .touched()
            .into_iter()
            .filter(|p| {
                (p.starts_with("/m/link") && p != Path::new("/m/link"))
                    || p.starts_with("/mnt")
                    || p.starts_with("/net")
            })
            .collect();
        assert!(past.is_empty(), "{target}: {past:?}");
        assert!(!may_look(
            Path::new("/m"),
            Path::new("link/doc.docx"),
            &net,
            &fs
        ));
    }
}

/// A local path is walked as before: not refused when outside the data folder, refused in it.
#[cfg(unix)]
#[test]
fn a_local_path_is_walked_as_before() {
    let fs = FakeFs::with(&[
        ("/m/brief.docx", Fake::File),
        ("/m/in", Fake::Link("/d".into())),
    ]);
    let net = plain_net(false);
    assert!(!refused(
        &fence("/m", &["/m/brief.docx", "notes.txt"]),
        &net,
        &fs
    ));
    assert!(refused(&fence("/m", &["/m/in/settings.json"]), &net, &fs));
    assert!(refused(&fence("/m", &["/m/x/../in/y"]), &net, &fs));
    assert!(may_look(
        Path::new("/m"),
        Path::new("brief.docx"),
        &net,
        &fs
    ));
}

/// In a matter on a share, a path by its text under the matter is outside and never looked at;
/// another server, a `..` or a `.` loses that and is refused.
#[test]
fn a_path_in_a_share_matter_is_outside_untouched_and_anything_else_refused() {
    let fs = FakeFs::default();
    let net = windows_net();
    let matter = r"\\srv\share\m";
    for path in [
        r"\\srv\share\m\brief.docx",
        r"\\srv\share\m",
        r"\\SRV\Share\M\Brief.docx",
        "//srv/share/m/brief.docx",
        r"//srv\share/m\sub/brief.docx",
    ] {
        assert!(!refused(&fence(matter, &[path]), &net, &fs), "{path}");
        assert!(
            !may_look(Path::new(matter), Path::new(path), &net, &fs),
            "{path}"
        );
    }
    assert_eq!(network_touched(&fs), Vec::<PathBuf>::new());
    for path in [
        r"\\evil\share\m\brief.docx",
        r"\\srv\share\m\..\other\x",
        r"\\srv\share\m\sub\..\x",
        r"\\srv\share\m\.\x",
        r"\\srv\share\m\x.\y",
        r"\\srv\share\m2\x",
        r"\\srv\share\m\\x",
        r"\\?\UNC\srv\share\m\x",
        r"Z:\m\x",
    ] {
        assert!(refused(&fence(matter, &[path]), &net, &fs), "{path}");
    }
    assert_eq!(network_touched(&fs), Vec::<PathBuf>::new());
}

/// Names compare with ASCII case folding alone: the Kelvin sign (U+212A) is not `k`.
#[test]
fn the_kelvin_sign_is_not_k() {
    let fs = FakeFs::default();
    let net = windows_net();
    let matter = r"\\srv\share\kases";
    assert!(!refused(
        &fence(matter, &[r"\\srv\share\Kases\x"]),
        &net,
        &fs
    ));
    assert!(refused(
        &fence(matter, &[r"\\srv\share\\u{212A}ases\x"]),
        &net,
        &fs
    ));
    assert_eq!(network_touched(&fs), Vec::<PathBuf>::new());
}

/// A matter on a mapped drive matches only paths on that drive, never the same share by its
/// UNC name; a UNC matter never matches its `\\?\UNC\` spelling.
#[test]
fn only_the_matter_folders_own_form_matches() {
    let fs = FakeFs::default();
    let net = windows_net();
    assert!(!refused(
        &fence(r"Z:\m", &[r"Z:\m\x.docx", "z:/m/x.docx"]),
        &net,
        &fs
    ));
    assert!(refused(&fence(r"Z:\m", &[r"\\srv\share\m\x"]), &net, &fs));
    assert!(refused(&fence(r"Z:\m", &[r"\\?\Z:\m\x"]), &net, &fs));
    assert!(refused(&fence(r"\\srv\share\m", &[r"Z:\m\x"]), &net, &fs));
    assert!(refused(
        &fence(r"\\?\UNC\srv\share\m", &[r"\\srv\share\m\x"]),
        &net,
        &fs
    ));
    assert!(refused(
        &fence(r"\\srv\share\m", &[r"\\?\UNC\srv\share\m\x"]),
        &net,
        &fs
    ));
    assert_eq!(network_touched(&fs), Vec::<PathBuf>::new());
}

/// A deny folds at least as broadly as APFS and NTFS: the long s and the Kelvin sign reach the
/// data folder, in a path and inside a command's word. The same Kelvin-sign spelling never gets
/// the share matter's exception, which compares strictly (`the_kelvin_sign_is_not_k`).
#[test]
fn a_long_s_or_kelvin_sign_spelling_of_the_data_folder_is_refused() {
    let fs = FakeFs::default();
    let net = plain_net(false);
    let real = |_: &Path| None;
    let disk = Disk {
        fs: &fs,
        real: &real,
        net: &net,
    };
    let check = |data: &str, paths: &[&str], words: &[&str]| {
        let mut check = fence("/m", paths);
        check.data = PathBuf::from(data);
        check.words = words.iter().map(|word| (*word).to_owned()).collect();
        check.touches_data_on(true, &disk)
    };
    assert!(check(
        "/u/.clauseo",
        &["/u/.clau\u{17F}eo/settings.json"],
        &[]
    ));
    assert!(check("/u/kdata", &["/u/\u{212A}data/x"], &[]));
    assert!(check("/u/kdata", &[], &["/u/\u{212A}DATA/x"]));
    assert!(check(
        "/u/.clauseo",
        &[],
        &["x=/u/.CLAU\u{17F}EO/settings.json"]
    ));
    assert!(!check("/u/kdata", &["/u/kdatb/x"], &["/u/kdatb/x"]));
    let share = windows_net();
    let matter = r"\\srv\share\kdata";
    let kelvin = r"\\srv\share\\u{212A}data\x";
    assert!(refused(&fence(matter, &[kelvin]), &share, &fs));
}
