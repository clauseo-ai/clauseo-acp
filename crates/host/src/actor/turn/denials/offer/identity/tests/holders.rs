//! Folders that hold a protected place, by identity (#764 round 3): macOS's data volume root and
//! `/System/Volumes` above it, `/Users`, a folder holding another matter and a mounted volume's
//! root are never offered whole, while a sibling that holds nothing is; Windows' `C:\Users` and
//! `C:\` (on Windows). Then an identity that can't be read refuses, the capture walks the host's
//! own paths (a matter root linking to a share is never opened), and a place made after the
//! capture makes it stale.

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::Arc;

use clauseo_protocol::DenialKind;

#[cfg(unix)]
use super::capture;
use super::{FakeIds, capture_on, card, rules};
#[cfg(unix)]
use super::{aliased, matter};
#[cfg(unix)]
use crate::actor::turn::denials::check::SessionCache;
#[cfg(unix)]
use crate::actor::turn::denials::offer::KnownMatter;
use crate::actor::turn::denials::offer::identity::FileId;
use crate::actor::turn::denials::offer::{Offer, OfferRules};
#[cfg(unix)]
use crate::actor::turn::denials::resolve::Disk;
#[cfg(unix)]
use crate::network_paths::resolve::tests::Fake;
use crate::network_paths::resolve::tests::FakeFs;
#[cfg(unix)]
use crate::network_paths::tests::mac_net;
use crate::network_paths::tests::plain_net;

/// A Mac: home on the data volume (2), firmlinked; matter `Smith` on a USB volume (3).
#[cfg(unix)]
fn mac_rules() -> OfferRules {
    OfferRules {
        home: Some("/Users/a".into()),
        data_dir: "/Users/a/Library/Application Support/Clauseo".into(),
        matter_root: "/Users/a/Matters/Acme".into(),
        matters: vec![
            KnownMatter {
                id: matter(1),
                name: "Acme".into(),
                root: "/Users/a/Matters/Acme".into(),
            },
            KnownMatter {
                id: matter(2),
                name: "Smith".into(),
                root: "/Volumes/Work/Clients/Smith".into(),
            },
        ],
        fold_case: true,
        ..rules()
    }
}

/// The Mac's identities: the system volume (1) holds `/`, `/System` and `/System/Volumes`; the
/// data volume (2) is mounted at `/System/Volumes/Data` and firmlinked at `/Users` and
/// `/Volumes`; USB volumes at `/Volumes/Work` (3) and `/Volumes/USB` (4).
fn mac_ids() -> FakeIds {
    let mut ids = FakeIds::with(&[
        &["/"],
        &["/System"],
        &["/System/Volumes"],
        &["/System/Volumes/Data"],
        &["/Users", "/System/Volumes/Data/Users"],
        &["/Users/a", "/System/Volumes/Data/Users/a"],
        &["/Users/a/Matters", "/System/Volumes/Data/Users/a/Matters"],
        &["/Users/a/Matters/Acme"],
        &["/Users/a/Deals"],
        &["/Volumes", "/System/Volumes/Data/Volumes"],
        &["/Volumes/Work", "/System/Volumes/Data/Volumes/Work"],
        &["/Volumes/Work/Clients"],
        &["/Volumes/Work/Clients/Smith"],
        &["/Volumes/Work/Other"],
        &["/Volumes/USB", "/System/Volumes/Data/Volumes/USB"],
    ]);
    for id in ids.ids.values_mut() {
        id.volume = match id.file {
            1..=3 => 1,
            4..=10 => 2,
            11..=14 => 3,
            _ => 4,
        };
    }
    ids
}

fn whole(path: &str, offered: bool) -> Offer {
    Offer {
        kind: DenialKind::Folder {
            path: path.into(),
            other_matter: None,
        },
        offered,
    }
}

// Unix fixture paths aren't absolute on Windows, where the walk refuses them; Windows' walk
// rules are tested in network_paths' Windows tests.
#[cfg(unix)]
#[test]
fn a_folder_holding_a_protected_place_or_a_volume_root_is_never_offered_whole() {
    let ids = mac_ids();
    let captured = capture_on(mac_rules(), &ids, &FakeFs::default(), &mac_net(&[]));
    for path in [
        "/System/Volumes/Data",
        "/System/Volumes",
        "/Users",
        "/Volumes/Work/Clients",
        "/Volumes/Work",
        // A mounted volume's root that holds nothing protected.
        "/Volumes/USB",
    ] {
        assert!(
            captured.places.holds_protected(Path::new(path), &ids),
            "{path}"
        );
        assert_eq!(
            card(&captured, &ids, path, false),
            whole(path, false),
            "{path}"
        );
    }
    // The bug: a missing file just under the data volume's root got that whole volume's card.
    // Now its folder is refused and the file alone is offered.
    assert_eq!(
        card(&captured, &ids, "/System/Volumes/Data/x", true),
        Offer {
            kind: DenialKind::File {
                path: "/System/Volumes/Data/x".into(),
                other_matter: None,
            },
            offered: true,
        }
    );
    // Siblings that hold nothing protected are offered whole.
    for path in ["/Volumes/Work/Other", "/Users/a/Deals"] {
        assert!(
            !captured.places.holds_protected(Path::new(path), &ids),
            "{path}"
        );
        assert_eq!(
            card(&captured, &ids, path, false),
            whole(path, true),
            "{path}"
        );
    }
    // A folder that does not exist holds nothing.
    assert!(
        !captured
            .places
            .holds_protected(Path::new("/Users/a/Deals/new"), &ids)
    );
}

#[cfg(windows)]
#[test]
fn windows_users_and_a_drive_root_are_never_offered_whole() {
    let ids = FakeIds::with(&[
        &[r"C:\"],
        &[r"C:\Users"],
        &[r"C:\Users\a"],
        &[r"C:\Users\a\Matters"],
        &[r"C:\Users\a\Matters\Acme"],
        &[r"C:\Work"],
    ]);
    let rules = OfferRules {
        home: Some(r"C:\Users\a".into()),
        data_dir: r"C:\Users\a\AppData\Roaming\Clauseo".into(),
        matter_root: r"C:\Users\a\Matters\Acme".into(),
        matters: Vec::new(),
        fold_case: true,
        ..rules()
    };
    let captured = capture_on(rules, &ids, &FakeFs::default(), &plain_net(true));
    for path in [r"C:\Users", r"C:\"] {
        assert!(
            captured.places.holds_protected(Path::new(path), &ids),
            "{path}"
        );
        assert_eq!(
            card(&captured, &ids, path, false),
            whole(path, false),
            "{path}"
        );
    }
    assert_eq!(
        card(&captured, &ids, r"C:\Work", false),
        whole(r"C:\Work", true)
    );
}

// Unix fixture paths aren't absolute on Windows, where the walk refuses them; Windows' walk
// rules are tested in network_paths' Windows tests.
#[cfg(unix)]
#[test]
fn an_identity_that_cant_be_read_refuses() {
    let mut ids = aliased();
    let captured = capture(rules(), &ids);
    ids.unreadable.push("/home/a/Deals".into());
    assert!(
        captured
            .places
            .pin(Path::new("/home/a/Deals/spa.docx"), &ids)
            .is_err()
    );
    assert!(
        captured
            .places
            .holds_protected(Path::new("/home/a/Deals"), &ids)
    );
}

// Unix fixture paths aren't absolute on Windows, where the walk refuses them; its Windows twin
// is in `windows_places.rs`.
#[cfg(unix)]
#[test]
fn a_matter_root_linking_to_a_share_is_captured_by_its_text_never_opened() {
    for target in [r"\\server\share", "/net/server/share"] {
        let fs = FakeFs::with(&[("/home/a/Matters/Smith", Fake::Link(target.into()))]);
        let ids = aliased();
        let captured = capture_on(rules(), &ids, &fs, &plain_net(false));
        let smith = Path::new("/home/a/Matters/Smith");
        let roots: Vec<&Path> = captured.rules.matters.iter().map(|k| &*k.root).collect();
        assert!(roots.contains(&smith), "{target}: {roots:?}");
        // Its text still judges: a path in it is Smith's.
        let inside = captured.rules.other_matter(&smith.join("x.docx"));
        assert_eq!(inside.map(|known| known.id), Some(matter(2)), "{target}");
        // The walk read the link itself and stopped: nothing past it, nothing on the share.
        let touched = fs.touched();
        assert!(
            touched.iter().all(|p| !p.starts_with(smith) || p == smith),
            "{target}: {touched:?}"
        );
        assert!(
            touched
                .iter()
                .all(|p| !p.starts_with("/net") && !p.to_string_lossy().starts_with(r"\\")),
            "{target}: {touched:?}"
        );
        // No identity asked of it or of anything under it.
        let asked = ids.asked();
        assert!(
            asked.iter().all(|p| !p.starts_with(smith)),
            "{target}: {asked:?}"
        );
    }
}

// Unix fixture paths aren't absolute on Windows, where the walk refuses them; Windows' walk
// rules are tested in network_paths' Windows tests.
#[cfg(unix)]
#[test]
fn a_place_made_after_the_capture_makes_it_stale() {
    let late = aliased();
    let mut early = aliased();
    early.ids.remove(Path::new("/home/a/.ssh"));
    early.ids.remove(Path::new("/home/a/SSH~1"));
    let before = capture(rules(), &early);
    assert!(!before.places.stale(&early));
    assert!(before.places.stale(&late));
    // Captured before `.ssh` was made, its 8.3 name would be a folder of its own.
    let ssh = "/home/a/SSH~1/id_rsa";
    assert!(card(&before, &late, ssh, true).offered);
    // The session's cache takes it again once `.ssh` is there.
    let (fs, net) = (FakeFs::default(), plain_net(false));
    let real = |_: &Path| -> Option<PathBuf> { None };
    let disk = Disk {
        fs: &fs,
        real: &real,
        net: &net,
    };
    let cache = SessionCache::default();
    let first = cache.captured(1, rules(), &disk, &early);
    assert!(Arc::ptr_eq(
        &first,
        &cache.captured(1, rules(), &disk, &early)
    ));
    let again = cache.captured(1, rules(), &disk, &late);
    assert!(!Arc::ptr_eq(&first, &again));
    let got = card(&again, &late, ssh, true);
    assert_eq!(got, whole("/home/a/.ssh", false));
}

/// The fake's identities are what the cases say they are.
#[test]
fn the_macs_volumes_are_as_drawn() {
    let ids = mac_ids();
    let volume = |path: &str| ids.ids.get(Path::new(path)).map(|id: &FileId| id.volume);
    assert_eq!(volume("/System/Volumes"), Some(1));
    assert_eq!(volume("/System/Volumes/Data"), Some(2));
    assert_eq!(volume("/Users/a/Deals"), Some(2));
    assert_eq!(volume("/Volumes/Work"), Some(3));
    assert_eq!(volume("/Volumes/Work/Other"), Some(3));
    assert_eq!(volume("/Volumes/USB"), Some(4));
}
