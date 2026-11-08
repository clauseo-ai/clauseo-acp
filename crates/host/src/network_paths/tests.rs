//! Each network form, from its text alone: Windows' UNC and device forms, a remote drive by an
//! injected drive-type answer, macOS's `/net` and `/Network`, a fake remote mount list, URLs,
//! and the Linux mount table's parse.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::linux::remote_mounts as linux_remote_mounts;
use super::macos_rules::parse_firmlinks;
use super::{NetEnv, fold_name, is_network_path};

/// No mounts, no remote drive: only the text decides.
pub(crate) fn plain_net(windows: bool) -> NetEnv {
    NetEnv {
        windows,
        fold_case: windows,
        remote_mounts: Vec::new(),
        remote_drive: Box::new(|_| false),
        macos: false,
        firmlinks: Some(Vec::new()),
    }
}

/// The remote mount points of a Linux mount table (`linux.rs`).
pub(crate) fn linux_mounts(table: &[u8]) -> Vec<PathBuf> {
    linux_remote_mounts(table).unwrap()
}

/// macOS's rules: folded names, the given remote mounts, and the usual firmlinks.
pub(crate) fn mac_net(remote_mounts: &[&str]) -> NetEnv {
    NetEnv {
        fold_case: true,
        remote_mounts: remote_mounts.iter().map(PathBuf::from).collect(),
        macos: true,
        firmlinks: Some(parse_firmlinks(
            "/Users\tUsers\n/Volumes\tVolumes\n/Applications\tApplications\n/usr/local\tusr/local\n",
        )),
        ..plain_net(false)
    }
}

/// Windows' rules with drive `Z:` mapped to a share; the letters asked are kept.
fn windows_net() -> (NetEnv, Arc<Mutex<Vec<char>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&asked);
    let net = NetEnv {
        windows: true,
        fold_case: true,
        remote_drive: Box::new(move |letter| {
            seen.lock().unwrap().push(letter);
            letter == 'Z'
        }),
        ..plain_net(true)
    };
    (net, asked)
}

fn network(text: &str, net: &NetEnv) -> bool {
    is_network_path(Path::new(text), net)
}

#[test]
fn every_windows_network_and_device_form_is_network() {
    let (net, _) = windows_net();
    for text in [
        r"\\server\share\brief.docx",
        r"\\server",
        "//server/share/brief.docx",
        r"\\?\UNC\server\share\x",
        r"\\.\UNC\server\share\x",
        r"\??\UNC\server\share\x",
        r"\Device\Mup\server\share\x",
        r"\device\mup\server",
        r"\Device\HarddiskVolume3\x",
        r"\GLOBAL??\C:\x",
        r"\\.\pipe\x",
        r"\\.\C:\x",
        r"\\?\GLOBALROOT\Device\Mup\server\x",
        r"\\?\Volume{0b3c1f2e-0000-0000-0000-100000000000}\x",
        r"\\?\C:x",
        r"\\?\Z:\x",
        r"Z:\x",
        r"z:\x",
        "Z:",
        "smb://server/share",
        "afp://server/share",
        "file://server/share/x",
    ] {
        assert!(network(text, &net), "{text}");
    }
    for text in [
        r"C:\Users\a\brief.docx",
        "C:/Users/a/brief.docx",
        r"\\?\C:\Users\a",
        r"\\?\c:\x",
        r"\Users\a",
        "C://x",
    ] {
        assert!(!network(text, &net), "{text}");
    }
}

#[test]
fn a_drive_is_asked_by_its_letter_alone() {
    let (net, asked) = windows_net();
    assert!(network(r"Z:\Deals\spa.docx", &net));
    assert!(!network(r"C:\Deals\spa.docx", &net));
    assert!(network(r"\\?\Z:\Deals", &net));
    // A UNC path is network before any drive is asked.
    assert!(network(r"\\server\share", &net));
    assert_eq!(*asked.lock().unwrap(), vec!['Z', 'C', 'Z']);
}

#[test]
fn backslash_forms_are_network_on_every_system_slash_forms_only_on_windows() {
    let unix = plain_net(false);
    assert!(network(r"\\server\share\x", &unix));
    assert!(network(r"\\?\UNC\server\x", &unix));
    assert!(network(r"\Device\Mup\server", &unix));
    // On Unix `//x` is `/x`, and a drive letter is a file name.
    assert!(!network("//server/share", &unix));
    assert!(!network(r"Z:\x", &unix));
}

#[test]
fn macos_automount_folders_are_network() {
    let mac = NetEnv {
        fold_case: true,
        ..plain_net(false)
    };
    for text in [
        "/net/host/share",
        "/net",
        "/Network/Servers/host/x",
        "/NET/host",
    ] {
        assert!(network(text, &mac), "{text}");
    }
    for text in ["/netflix/x", "/Users/a/net/x", "/home/a/Network"] {
        assert!(!network(text, &mac), "{text}");
    }
    // A network check folds on every system: over-matching is its safe side.
    assert!(network("/NET/host", &plain_net(false)));
}

#[test]
fn a_path_under_a_remote_mount_is_network() {
    let net = NetEnv {
        remote_mounts: vec!["/Volumes/share".into(), "/mnt/nfs".into()],
        ..plain_net(false)
    };
    assert!(network("/Volumes/share", &net));
    assert!(network("/Volumes/share/Deals/spa.docx", &net));
    assert!(network("/mnt/nfs/x", &net));
    assert!(!network("/Volumes/sharex/a", &net));
    assert!(!network("/Volumes/Macintosh HD/a", &net));
}

#[test]
fn the_linux_mount_table_gives_every_mount_not_of_a_local_type() {
    let table = "\
/dev/nvme0n1p2 / ext4 rw,relatime 0 0
server:/export /mnt/nfs nfs4 rw,relatime 0 0
//server/share /mnt/my\\040share cifs rw 0 0
user@host:/x /home/a/remote fuse.sshfs rw 0 0
tmpfs /tmp tmpfs rw 0 0
//server/s3 /mnt/s3 smb3 rw 0 0
proc /proc proc rw 0 0
drvfs /mnt/c 9p rw 0 0
";
    let mounts = linux_remote_mounts(table.as_bytes()).unwrap();
    let want: Vec<PathBuf> = [
        "/mnt/nfs",
        "/mnt/my share",
        "/home/a/remote",
        "/mnt/s3",
        "/mnt/c",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    assert_eq!(mounts, want);
}

#[test]
fn a_verbatim_drive_path_is_put_plain() {
    let (net, _) = windows_net();
    assert_eq!(net.plain(Path::new(r"\\?\C:\x")), PathBuf::from(r"C:\x"));
    assert_eq!(net.plain(Path::new(r"C:\x")), PathBuf::from(r"C:\x"));
    let unix = plain_net(false);
    assert_eq!(
        unix.plain(Path::new(r"\\?\C:\x")),
        PathBuf::from(r"\\?\C:\x")
    );
}

#[test]
fn a_deny_name_folds_the_long_s_and_the_kelvin_sign() {
    assert_eq!(fold_name(".Clau\u{17F}eo"), ".clauseo");
    assert_eq!(fold_name("\u{212A}DATA"), "kdata");
    assert_eq!(fold_name(".GIT"), ".git");
}

/// A spelling APFS or NTFS takes as the same name reaches the mount and the automount folders:
/// the long s, the Kelvin sign, and NFC against NFD.
#[test]
fn a_folded_or_normalised_spelling_of_a_mount_is_network() {
    let net = NetEnv {
        remote_mounts: vec!["/Volumes/Share".into(), "/Volumes/Caf\u{e9}".into()],
        ..plain_net(false)
    };
    for text in [
        "/Volume\u{17F}/Share/x",
        "/Volumes/\u{17F}hare/x",
        "/Netwo\u{72}\u{212A}/Servers/evil",
        "/Volumes/Cafe\u{301}/x",
    ] {
        assert!(network(text, &net), "{text}");
    }
    let nfd_mount = NetEnv {
        remote_mounts: vec!["/Volumes/Cafe\u{301}".into()],
        ..plain_net(false)
    };
    assert!(network("/Volumes/Caf\u{e9}/x", &nfd_mount));
    assert!(!network("/Volumes/Cafe/x", &nfd_mount));
}

#[test]
fn a_linux_mount_table_is_read_as_bytes_and_an_unparsable_one_is_unknown() {
    let mut table = b"/dev/sda1 / ext4 rw 0 0\nhost:/x /mnt/caf\xe9 nfs rw 0 0\n".to_vec();
    table.extend_from_slice(b"systemd-1 /proc/sys/fs/binfmt_misc autofs rw 0 0\n");
    table.extend_from_slice(b"gvfsd-fuse /run/user/1000/gvfs fuse.gvfsd-fuse rw 0 0\n");
    let mounts = linux_remote_mounts(&table).unwrap();
    assert_eq!(mounts.len(), 3);
    assert_eq!(
        mounts.get(1),
        Some(&PathBuf::from("/proc/sys/fs/binfmt_misc"))
    );
    assert_eq!(linux_remote_mounts(b"/dev/sda1 /\n"), None);
    assert_eq!(linux_remote_mounts(b""), Some(Vec::new()));
}
