//! Windows' places by identity on a `C:\` fixture (tool calls are Windows' only denial source):
//! each 8.3 spelling pinned to its place, an identity that can't be read refused, a place made
//! after the capture making it stale, a matter root linking to a share judged by its text and
//! never opened, and no network place (UNC, `//server`, the `\\?\UNC\` and `\\.\UNC\` forms, a
//! mapped drive) touched or asked about. Windows only: a `C:\` path is absolute there alone.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::{DenialKind, OtherMatter};

use super::{FakeIds, capture_on, card, matter};
use crate::actor::turn::denials::check::SessionCache;
use crate::actor::turn::denials::offer::identity::Captured;
use crate::actor::turn::denials::offer::{KnownMatter, Offer, OfferRules};
use crate::actor::turn::denials::resolve::Disk;
use crate::network_paths::NetEnv;
use crate::network_paths::resolve::tests::{Fake, FakeFs};
use crate::network_paths::tests::plain_net;

pub(crate) const HOME: &str = r"C:\Users\a";
pub(crate) const ACME: &str = r"C:\Users\a\Matters\Acme";
pub(crate) const SMITH: &str = r"C:\Users\a\Matters\Smith";
pub(crate) const DATA: &str = r"C:\Users\a\AppData\Roaming\Clauseo";

/// `C:\Users\a`, its data folder, this matter `Acme` and another, `Smith`, under `Matters`;
/// names compare without case, as on NTFS.
pub(crate) fn win_rules() -> OfferRules {
    let known = |n, name: &str, root: &str| KnownMatter {
        id: matter(n),
        name: name.into(),
        root: root.into(),
    };
    OfferRules {
        home: Some(HOME.into()),
        data_dir: DATA.into(),
        matter: matter(1),
        matter_root: ACME.into(),
        refused: Vec::new(),
        matters: vec![known(1, "Acme", ACME), known(2, "Smith", SMITH)],
        fold_case: true,
    }
}

/// The places of [`win_rules`] and their 8.3 names, each sharing its place's identity.
pub(crate) fn win_ids() -> FakeIds {
    FakeIds::with(&[
        &[r"C:\"],
        &[r"C:\Users"],
        &[HOME],
        &[r"C:\Users\a\Matters", r"C:\Users\a\MATTER~1"],
        &[ACME, r"C:\Users\a\Matters\ACME~1"],
        &[SMITH, r"C:\Users\a\Matters\SMITH~1"],
        &[
            r"C:\Users\a\Matters\Smith\.clauseo",
            r"C:\Users\a\Matters\Smith\CLAUSE~1",
        ],
        &[
            r"C:\Users\a\Matters\Smith\.git",
            r"C:\Users\a\Matters\Smith\GIT~1",
        ],
        &[r"C:\Users\a\.ssh", r"C:\Users\a\SSH~1"],
        &[r"C:\Users\a\Deals"],
        &[DATA, r"C:\Users\a\AppData\Roaming\CLAUSE~1"],
    ])
}

/// Windows' rules with drive `Z:` mapped to a share (as `network_paths::tests::windows_net`).
pub(crate) fn mapped_net() -> NetEnv {
    NetEnv {
        remote_drive: Box::new(|letter| letter == 'Z'),
        ..plain_net(true)
    }
}

/// [`win_rules`] captured on an empty disk (every path walks to itself) under [`mapped_net`].
pub(crate) fn win_capture(ids: &FakeIds) -> Captured {
    capture_on(win_rules(), ids, &FakeFs::default(), &mapped_net())
}

/// Whether `path` is on the share or the mapped drive of these tests.
fn on_the_network(path: &Path) -> bool {
    let text = path.to_string_lossy().to_ascii_lowercase();
    text.starts_with(r"\\")
        || text.starts_with("//")
        || text.starts_with("z:")
        || text.contains("server")
}

fn smith() -> OtherMatter {
    OtherMatter {
        matter: matter(2),
        name: "Smith".into(),
    }
}

fn folder(path: &str, other_matter: Option<OtherMatter>, offered: bool) -> Offer {
    Offer {
        kind: DenialKind::Folder {
            path: path.into(),
            other_matter,
        },
        offered,
    }
}

#[test]
fn every_8_3_spelling_of_a_known_place_is_pinned_to_it() {
    let ids = win_ids();
    let captured = win_capture(&ids);
    let pin = |path: &str| captured.places.pin(Path::new(path), &ids).unwrap();
    for (alias, place) in [
        (
            r"C:\Users\a\Matters\SMITH~1\x.docx",
            r"C:\Users\a\Matters\Smith\x.docx",
        ),
        (
            r"C:\Users\a\Matters\Smith\CLAUSE~1\notes.md",
            r"C:\Users\a\Matters\Smith\.clauseo\notes.md",
        ),
        (r"C:\Users\a\SSH~1\id_rsa", r"C:\Users\a\.ssh\id_rsa"),
        (
            r"C:\Users\a\AppData\Roaming\CLAUSE~1\run",
            r"C:\Users\a\AppData\Roaming\Clauseo\run",
        ),
    ] {
        assert_eq!(pin(alias), PathBuf::from(place), "{alias}");
    }
    // The cards: another matter's root, the folder of the matters and `.ssh` never offered
    // whole; another matter's `.clauseo` never; a folder that is no known place offered.
    let got = |path: &str, is_file: bool| card(&captured, &ids, path, is_file);
    assert_eq!(
        got(r"C:\Users\a\Matters\SMITH~1", false),
        folder(SMITH, Some(smith()), false)
    );
    assert_eq!(
        got(r"C:\Users\a\MATTER~1", false),
        folder(r"C:\Users\a\Matters", None, false)
    );
    assert_eq!(
        got(r"C:\Users\a\SSH~1\id_rsa", true),
        folder(r"C:\Users\a\.ssh", None, false)
    );
    let clauseo = Offer {
        kind: DenialKind::File {
            path: r"C:\Users\a\Matters\Smith\.clauseo\notes.md".into(),
            other_matter: Some(smith()),
        },
        offered: false,
    };
    assert_eq!(
        got(r"C:\Users\a\Matters\Smith\CLAUSE~1\notes.md", true),
        clauseo
    );
    assert_eq!(
        got(r"C:\Users\a\Deals\spa.docx", true),
        folder(r"C:\Users\a\Deals", None, true)
    );
}

#[test]
fn an_identity_that_cant_be_read_refuses() {
    let mut ids = win_ids();
    let captured = win_capture(&ids);
    ids.unreadable.push(r"C:\Users\a\Deals".into());
    let places = &captured.places;
    assert!(
        places
            .pin(Path::new(r"C:\Users\a\Deals\spa.docx"), &ids)
            .is_err()
    );
    assert!(places.holds_protected(Path::new(r"C:\Users\a\Deals"), &ids));
}

#[test]
fn a_place_made_after_the_capture_makes_it_stale() {
    let late = win_ids();
    let mut early = win_ids();
    early.ids.remove(Path::new(r"C:\Users\a\.ssh"));
    early.ids.remove(Path::new(r"C:\Users\a\SSH~1"));
    let before = win_capture(&early);
    assert!(!before.places.stale(&early));
    assert!(before.places.stale(&late));
    // Captured before `.ssh` was made, its 8.3 name would be a folder of its own.
    let ssh = r"C:\Users\a\SSH~1\id_rsa";
    assert!(card(&before, &late, ssh, true).offered);
    // The session's cache takes it again once `.ssh` is there.
    let (fs, net) = (FakeFs::default(), mapped_net());
    let real = |_: &Path| -> Option<PathBuf> { None };
    let disk = Disk {
        fs: &fs,
        real: &real,
        net: &net,
    };
    let cache = SessionCache::default();
    let first = cache.captured(1, win_rules(), &disk, &early);
    assert!(Arc::ptr_eq(
        &first,
        &cache.captured(1, win_rules(), &disk, &early)
    ));
    let again = cache.captured(1, win_rules(), &disk, &late);
    assert!(!Arc::ptr_eq(&first, &again));
    assert_eq!(
        card(&again, &late, ssh, true),
        folder(r"C:\Users\a\.ssh", None, false)
    );
}

#[test]
fn a_matter_root_linking_to_a_share_is_captured_by_its_text_never_opened() {
    let targets = [
        r"\\server\share",
        "//server/share",
        r"\\?\UNC\server\share",
        r"Z:\Matters",
    ];
    for target in targets {
        let fs = FakeFs::with(&[(SMITH, Fake::Link(target.into()))]);
        let ids = win_ids();
        let captured = capture_on(win_rules(), &ids, &fs, &mapped_net());
        let smith = Path::new(SMITH);
        let roots: Vec<&Path> = captured.rules.matters.iter().map(|k| &*k.root).collect();
        assert!(roots.contains(&smith), "{target}: {roots:?}");
        // Its text still judges: a path in it is Smith's.
        let inside = captured.rules.other_matter(&smith.join("x.docx"));
        assert_eq!(inside.map(|known| known.id), Some(matter(2)), "{target}");
        // The walk read the link itself and stopped: nothing past it, nothing on the share.
        let touched = fs.touched();
        assert!(touched.iter().any(|p| p == smith), "{target}: {touched:?}");
        assert!(
            touched
                .iter()
                .all(|p| (!p.starts_with(smith) || p == smith) && !on_the_network(p)),
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

#[test]
fn a_network_place_is_never_touched_nor_asked_about() {
    let mut rules = win_rules();
    let shares = [
        ("Unc", r"\\server\share\x"),
        ("Slash", "//server/share/w"),
        ("Verbatim", r"\\?\UNC\server\share\y"),
        ("Device", r"\\.\UNC\server\share\v"),
        ("Mapped", r"Z:\Matters\Mapped"),
    ];
    for (n, (name, root)) in (3_u128..).zip(shares) {
        rules.matters.push(KnownMatter {
            id: matter(n),
            name: name.into(),
            root: root.into(),
        });
    }
    let fs = FakeFs::default();
    let ids = win_ids();
    let captured = capture_on(rules, &ids, &fs, &mapped_net());
    let (touched, asked) = (fs.touched(), ids.asked());
    assert!(touched.iter().all(|p| !on_the_network(p)), "{touched:?}");
    assert!(asked.iter().all(|p| !on_the_network(p)), "{asked:?}");
    // The local places were looked at and asked about.
    assert!(touched.iter().any(|p| p == Path::new(ACME)), "{touched:?}");
    assert!(asked.iter().any(|p| p == Path::new(ACME)), "{asked:?}");
    assert_ne!(captured.places.len(), 0);
}
