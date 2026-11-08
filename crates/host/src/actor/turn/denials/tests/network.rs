//! Zero touches through the intake's check (`decide`, and the tool-call source's `outside_jail`)
//! for every network form: Windows' UNC and device forms, `//server` and a remote drive, `/net`
//! and `/Network`, URLs, a path under a remote mount (NFS, CIFS, autofs, GNOME's gvfs, rclone,
//! 9p, sshfs), every path when the mount table can't be read, macOS's `/.nofollow`, `/.resolve`,
//! `/.vol`, `/home` and the `/System/Volumes/Data` form of a share, and a local link to each.
//! Then macOS's firmlinked home is home, and a `..` after a missing name reaches `.ssh`, which
//! is not offered.

use std::path::{Path, PathBuf};

use clauseo_protocol::DenialKind;
use clauseo_sandbox::PathClass;

use super::super::check::{Unchecked, decide, outside_jail};
use super::super::network::NetEnv;
use super::super::offer::Offer;
use super::super::offer::identity::tests::FakeIds;
use super::super::offer::identity::{Captured, Places};
use super::{bare, disk, rules};
use crate::network_paths::resolve::tests::{Fake, FakeFs};
use crate::network_paths::tests::{linux_mounts, mac_net, plain_net};

/// A Linux mount table with one of each remote kind, and local ones.
const MOUNTS: &[u8] = b"/dev/sda1 / ext4 rw 0 0
nas:/export /mnt/nas nfs4 rw 0 0
//server/share /mnt/cifs cifs rw 0 0
systemd-1 /mnt/auto autofs rw 0 0
gvfsd-fuse /run/user/1000/gvfs fuse.gvfsd-fuse rw 0 0
remote: /home/a/Drive fuse.rclone rw 0 0
host0 /mnt/host 9p rw 0 0
a@h:/ /mnt/ssh fuse.sshfs rw 0 0
tmpfs /tmp tmpfs rw 0 0
";

/// Every form, each with the rules it is a network path under.
fn forms() -> Vec<(&'static str, NetEnv)> {
    let unix = || plain_net(false);
    let windows = || NetEnv {
        remote_drive: Box::new(|letter| letter == 'Z'),
        ..plain_net(true)
    };
    let linux = || NetEnv {
        remote_mounts: linux_mounts(MOUNTS),
        ..plain_net(false)
    };
    let mac = || mac_net(&["/Volumes/share"]);
    vec![
        (r"\\server\share\x.docx", unix()),
        (r"\\?\UNC\server\share\x.docx", unix()),
        (r"\??\UNC\server\share\x.docx", unix()),
        (r"\Device\Mup\server\share\x.docx", unix()),
        (r"\GLOBAL??\UNC\server\x.docx", unix()),
        (r"\\.\pipe\x", unix()),
        (r"\\?\GLOBALROOT\Device\Mup\x", unix()),
        (r"\\?\Volume{0}\x.docx", unix()),
        ("//server/share/x.docx", windows()),
        (r"Z:\x.docx", windows()),
        (r"\\?\Z:\x.docx", windows()),
        (r"\\localhost\Z$\x.docx", windows()),
        ("/net/host/x.docx", unix()),
        ("/Network/Servers/host/x.docx", unix()),
        ("smb://host/share/x.docx", unix()),
        ("file:///etc/x", unix()),
        ("/mnt/nas/x.docx", linux()),
        ("/mnt/cifs/x.docx", linux()),
        ("/mnt/auto/x.docx", linux()),
        (
            "/run/user/1000/gvfs/smb-share:server=h,share=s/x.docx",
            linux(),
        ),
        ("/home/a/Drive/x.docx", linux()),
        ("/mnt/host/x.docx", linux()),
        ("/mnt/ssh/x.docx", linux()),
        // An unreadable mount table: every path counts as remote.
        (
            "/home/a/Deals/x.docx",
            NetEnv {
                remote_mounts: vec![PathBuf::from("/")],
                ..plain_net(false)
            },
        ),
        ("/.nofollow/Volumes/share/x.docx", mac()),
        ("/.resolve/1/2/x.docx", mac()),
        ("/.vol/16777220/2/x.docx", mac()),
        ("/System/Volumes/Data/Volumes/share/x.docx", mac()),
        ("/Volumes/share/x.docx", mac()),
        ("/home/a/x.docx", mac()),
    ]
}

#[test]
fn every_network_form_is_refused_through_the_intake_untouched() {
    let fs = FakeFs::default();
    let ids = FakeIds::default();
    let captured = bare(rules());
    for (path, net) in forms() {
        let got = decide(
            &captured,
            Path::new(path),
            PathClass::Outside,
            &disk(&fs, &net),
            &ids,
        );
        assert_eq!(got.unchecked, Some(Unchecked::Network), "{path}");
        let kind = DenialKind::File {
            path: path.into(),
            other_matter: None,
        };
        let not_offered = Offer {
            kind,
            offered: false,
        };
        assert_eq!(got.offer, not_offered, "{path}");
        let named = [path.to_owned()];
        let jail = outside_jail(
            Path::new("/srv/m"),
            &captured,
            &named,
            &disk(&fs, &net),
            &ids,
        );
        assert_eq!(
            jail,
            Some((PathBuf::from(path), PathClass::Outside)),
            "{path}"
        );
        assert_eq!(fs.touched(), Vec::<PathBuf>::new(), "{path}");
        assert_eq!(ids.asked(), Vec::<PathBuf>::new(), "{path}");
    }
}

#[test]
fn a_local_link_to_each_network_form_stops_at_the_link() {
    let captured = bare(rules());
    let ids = FakeIds::default();
    for (target, net) in forms() {
        let fs = FakeFs::with(&[("/srv/m/link", Fake::Link(target.into()))]);
        let got = decide(
            &captured,
            Path::new("/srv/m/link/y.docx"),
            PathClass::Outside,
            &disk(&fs, &net),
            &ids,
        );
        assert!(!got.offer.offered, "{target}");
        assert!(got.unchecked.is_some(), "{target}");
        let touched = fs.touched();
        assert!(
            touched.iter().all(|p| !p.starts_with("/srv/m/link/y.docx")),
            "{target}: {touched:?}"
        );
        assert!(
            touched
                .iter()
                .all(|p| p.starts_with("/srv") || p == Path::new("/")),
            "{target}: {touched:?}"
        );
        assert_eq!(ids.asked(), Vec::<PathBuf>::new(), "{target}");
    }
}

#[test]
fn macos_firmlinked_home_is_home() {
    let rules = super::super::offer::OfferRules {
        home: Some("/Users/a".into()),
        ..rules()
    };
    let ids = FakeIds::with(&[
        &["/Users/a", "/System/Volumes/Data/Users/a"],
        &["/Users", "/System/Volumes/Data/Users"],
    ]);
    let places = Places::capture(&rules, &ids, &|_| false);
    let captured = Captured { rules, places };
    let fs = FakeFs::with(&[("/System/Volumes/Data/Users/a/Deals/x.docx", Fake::File)]);
    let net = mac_net(&[]);
    let at = |path: &str, is_file| {
        let got = decide(
            &captured,
            Path::new(path),
            PathClass::Outside,
            &disk(&fs, &net),
            &ids,
        );
        (got.offer, is_file)
    };
    let (home, _) = at("/System/Volumes/Data/Users/a", false);
    assert_eq!(
        home,
        Offer {
            kind: DenialKind::Folder {
                path: "/Users/a".into(),
                other_matter: None
            },
            offered: false
        }
    );
    let (deals, _) = at("/System/Volumes/Data/Users/a/Deals/x.docx", true);
    assert_eq!(
        deals,
        Offer {
            kind: DenialKind::Folder {
                path: "/Users/a/Deals".into(),
                other_matter: None
            },
            offered: true
        }
    );
}

#[test]
fn a_dot_dot_after_a_missing_name_still_reaches_the_link_and_its_ssh() {
    let fs = FakeFs::with(&[
        ("/tmp/l", Fake::Link("/home/a/.ssh".into())),
        ("/home/a/.ssh/id_rsa", Fake::File),
    ]);
    let net = plain_net(false);
    let ids = FakeIds::default();
    let got = decide(
        &bare(rules()),
        Path::new("/tmp/missing/../l/id_rsa"),
        PathClass::Outside,
        &disk(&fs, &net),
        &ids,
    );
    assert!(fs.touched().contains(&PathBuf::from("/tmp/l")));
    assert_eq!(got.unchecked, None);
    assert!(!got.offer.offered, "{got:?}");
    assert!(got.facts.is_some_and(|facts| facts.never));
}
