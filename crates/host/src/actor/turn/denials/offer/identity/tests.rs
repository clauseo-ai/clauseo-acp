//! The identity layer: every spelling of a known place (a trailing dot, an 8.3 name, macOS's
//! `/System/Volumes/Data` firmlink form) pinned to the place by a fake identity source, and the
//! offer rules then judging the place, not the spelling; a network place never asked about; the
//! targets' sameness.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use clauseo_protocol::{DenialKind, MatterId, OtherMatter};
use clauseo_sandbox::PathClass;
use uuid::Uuid;

use super::{Captured, FileId, Identity, Places, Target};
use crate::actor::turn::denials::offer::{KnownMatter, Offer, OfferRules, Refused, offer};
use crate::actor::turn::denials::resolve::Disk;
use crate::network_paths::NetEnv;
use crate::network_paths::resolve::tests::FakeFs;
use crate::network_paths::tests::plain_net;

mod holders;

/// Identities in memory: each path's, aliases sharing one; every path asked is kept. Any other
/// path is missing (`NotFound`), one in `unreadable` can't be read.
#[derive(Debug, Default)]
pub(crate) struct FakeIds {
    pub(crate) ids: BTreeMap<PathBuf, FileId>,
    pub(crate) asked: Mutex<Vec<PathBuf>>,
    pub(crate) unreadable: Vec<PathBuf>,
}

impl FakeIds {
    /// `names`, each path in a group sharing the group's identity.
    pub(crate) fn with(groups: &[&[&str]]) -> Self {
        let mut ids = BTreeMap::new();
        for (n, group) in (1_u128..).zip(groups) {
            for path in *group {
                ids.insert(PathBuf::from(path), FileId { volume: 1, file: n });
            }
        }
        Self {
            ids,
            asked: Mutex::new(Vec::new()),
            unreadable: Vec::new(),
        }
    }

    pub(crate) fn asked(&self) -> Vec<PathBuf> {
        self.asked.lock().unwrap().clone()
    }
}

impl Identity for FakeIds {
    fn id(&self, path: &Path) -> io::Result<FileId> {
        self.asked.lock().unwrap().push(path.to_path_buf());
        if self.unreadable.iter().any(|unreadable| unreadable == path) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.ids
            .get(path)
            .copied()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }
}

/// `rules` captured on `fs` under `net` (names as written), identities from `ids`.
pub(crate) fn capture_on(rules: OfferRules, ids: &FakeIds, fs: &FakeFs, net: &NetEnv) -> Captured {
    let real = |_: &Path| None;
    let disk = Disk {
        fs,
        real: &real,
        net,
    };
    Captured::new(rules, &disk, ids)
}

/// `rules` captured on an empty disk (every path walks to itself), identities from `ids`.
pub(crate) fn capture(rules: OfferRules, ids: &FakeIds) -> Captured {
    capture_on(rules, ids, &FakeFs::default(), &plain_net(false))
}

pub(crate) fn matter(n: u128) -> MatterId {
    MatterId::new(Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0000 | n,
    ))
    .unwrap()
}

/// `/home/a`, its data folder, this matter `Acme` and another, `Smith`, under `Matters`.
pub(crate) fn rules() -> OfferRules {
    let known = |n, name: &str| KnownMatter {
        id: matter(n),
        name: name.into(),
        root: format!("/home/a/Matters/{name}").into(),
    };
    OfferRules {
        home: Some("/home/a".into()),
        data_dir: "/home/a/.local/share/clauseo".into(),
        matter: matter(1),
        matter_root: "/home/a/Matters/Acme".into(),
        refused: Vec::new(),
        matters: vec![known(1, "Acme"), known(2, "Smith")],
        fold_case: false,
    }
}

/// The places of [`rules`] and their aliases: Windows' trailing dot and 8.3 names, macOS's
/// firmlink form, each sharing its place's identity.
pub(crate) fn aliased() -> FakeIds {
    FakeIds::with(&[
        &["/"],
        &["/home", "/System/Volumes/Data/home"],
        &[
            "/home/a",
            "/home/a.",
            "/home/a ",
            "/System/Volumes/Data/home/a",
        ],
        &["/home/a/Matters", "/home/a/MATTER~1"],
        &["/home/a/Matters/Acme"],
        &[
            "/home/a/Matters/Smith",
            "/home/a/Matters/Smith.",
            "/home/a/Matters/SMITH~1",
        ],
        &[
            "/home/a/Matters/Smith/.clauseo",
            "/home/a/Matters/Smith/CLAUSE~1",
        ],
        &["/home/a/Matters/Smith/.git", "/home/a/Matters/Smith/GIT~1"],
        &["/home/a/.ssh", "/home/a/SSH~1"],
        &["/home/a/Deals", "/System/Volumes/Data/home/a/Deals"],
        &["/home/a/.local"],
        &["/home/a/.local/share"],
        &[
            "/home/a/.local/share/clauseo",
            "/home/a/.local/share/CLAUSE~1",
        ],
    ])
}

fn smith() -> OtherMatter {
    OtherMatter {
        matter: matter(2),
        name: "Smith".into(),
    }
}

/// The card for `path` as `check::decide_walked` makes it: pinned, its whole folder asked by
/// identity.
pub(crate) fn card(captured: &Captured, ids: &FakeIds, path: &str, is_file: bool) -> Offer {
    let path = Path::new(path);
    let pinned = captured.places.pin(path, ids).unwrap();
    let folder = if is_file {
        path.parent().unwrap()
    } else {
        path
    };
    offer(
        &captured.rules,
        Refused::Path {
            path: &pinned,
            is_file,
            class: PathClass::Outside,
            holds_protected: captured.places.holds_protected(folder, ids),
        },
    )
}

fn folder(path: &str, other_matter: Option<OtherMatter>) -> DenialKind {
    DenialKind::Folder {
        path: path.into(),
        other_matter,
    }
}

#[test]
fn every_spelling_of_a_known_place_is_judged_as_that_place() {
    let ids = aliased();
    let captured = capture(rules(), &ids);
    let not_offered = |path: &str, is_file: bool, kind: DenialKind| {
        let got = card(&captured, &ids, path, is_file);
        assert_eq!(
            got,
            Offer {
                kind,
                offered: false
            },
            "{path}"
        );
    };
    // Home, by a trailing dot, a trailing space, and macOS's firmlink form.
    for alias in ["/home/a.", "/home/a ", "/System/Volumes/Data/home/a"] {
        not_offered(alias, false, folder("/home/a", None));
    }
    // A folder holding home, by its firmlink form.
    not_offered("/System/Volumes/Data/home", false, folder("/home", None));
    // Another matter's root, by a trailing dot or its 8.3 name: never offered whole.
    for alias in ["/home/a/Matters/Smith.", "/home/a/Matters/SMITH~1"] {
        not_offered(alias, false, folder("/home/a/Matters/Smith", Some(smith())));
    }
    // Its `.clauseo` and `.git` by their 8.3 names, and the folder of the matters.
    let clauseo = "/home/a/Matters/Smith/.clauseo/notes.md";
    let git = "/home/a/Matters/Smith/.git/config";
    for (alias, shown) in [
        ("/home/a/Matters/Smith/CLAUSE~1/notes.md", clauseo),
        ("/home/a/Matters/Smith/GIT~1/config", git),
    ] {
        let kind = DenialKind::File {
            path: shown.into(),
            other_matter: Some(smith()),
        };
        not_offered(alias, true, kind);
    }
    not_offered("/home/a/MATTER~1", false, folder("/home/a/Matters", None));
    // `.ssh` by its 8.3 name, and the data folder by its.
    let ssh = "/home/a/SSH~1/id_rsa";
    not_offered(ssh, true, folder("/home/a/.ssh", None));
    not_offered("/home/a/.local/share/CLAUSE~1", false, {
        folder("/home/a/.local/share/clauseo", None)
    });
    // A folder that is no known place keeps its own rules: offered, spelled as the place it
    // lies in.
    let deals = card(
        &captured,
        &ids,
        "/System/Volumes/Data/home/a/Deals/spa.docx",
        true,
    );
    assert_eq!(
        deals,
        Offer {
            kind: folder("/home/a/Deals", None),
            offered: true
        }
    );
}

#[test]
fn by_its_text_alone_a_spelling_would_have_been_offered() {
    // The bug the identity layer closes: no places, so the text rules see `/home/a.` as a
    // folder of its own.
    let captured = Captured {
        rules: rules(),
        places: Places::default(),
    };
    let got = card(&captured, &aliased(), "/home/a.", false);
    assert!(got.offered, "{got:?}");
}

#[test]
fn a_network_place_is_never_asked_about() {
    let mut rules = rules();
    rules.matters.push(KnownMatter {
        id: matter(3),
        name: "Share".into(),
        root: "/net/server/Share".into(),
    });
    let ids = aliased();
    let captured = capture(rules, &ids);
    assert!(
        ids.asked().iter().all(|p| !p.starts_with("/net")),
        "{:?}",
        ids.asked()
    );
    assert!(captured.places.len() > 5, "{}", captured.places.len());
}

#[test]
fn targets_are_the_same_by_identity_else_by_text() {
    let id = |n| Some(FileId { volume: 1, file: n });
    let at = |text: &str, id| Target {
        id,
        text: text.into(),
    };
    assert!(at("/a", id(1)).same(&at("/b", id(1))));
    assert!(!at("/a", id(1)).same(&at("/a", id(2))));
    assert!(at("/a", None).same(&at("/a", id(2))));
    assert!(at("/a", id(1)).same_file(&at("/b", id(1))));
    assert!(!at("/a", None).same_file(&at("/a", None)));
    assert_eq!(
        Target::text("/Users/A", true),
        Target::text("/users/a", true)
    );
    assert_ne!(
        Target::text("/Users/A", false),
        Target::text("/users/a", false)
    );
}
