//! The ruling through `decide` and `ruled`, on a fake disk with a fake identity source that maps
//! Windows' and macOS's other spellings to their place: a tool call's claim on another matter
//! gives no grant; the same file confirmed by an OS report gives the read-only card; a respelled
//! claim (`Smith.`, `SMITH~1`, `CLAUSE~1`, `SSH~1`) is judged as the place it names.

use std::path::{Path, PathBuf};

use clauseo_protocol::{DenialId, DenialKind, OtherMatter, ThreadId, TurnId};
use clauseo_sandbox::PathClass;
use uuid::Uuid;

use super::super::check::{Decided, decide};
use super::super::count::{Denials, Record, Recorded};
use super::super::offer::Offer;
use super::super::offer::identity::tests::{FakeIds, aliased, matter, rules};
use super::super::offer::identity::{Captured, FileId};
use super::super::ruled;
use super::disk;
use crate::network_paths::resolve::tests::{Fake, FakeFs};
use crate::network_paths::tests::plain_net;

const BRIEF: &str = "/home/a/Matters/Smith/brief.docx";

fn fake() -> FakeFs {
    FakeFs::with(&[
        (BRIEF, Fake::File),
        ("/home/a/Matters/SMITH~1/brief.docx", Fake::File),
        ("/home/a/Matters/Smith.", Fake::Dir),
        ("/home/a/Matters/Smith/CLAUSE~1/notes.md", Fake::File),
        ("/home/a/SSH~1/id_rsa", Fake::File),
        ("/home/a/Deals/spa.docx", Fake::File),
    ])
}

fn ids() -> FakeIds {
    let mut ids = aliased();
    for path in [BRIEF, "/home/a/Matters/SMITH~1/brief.docx"] {
        let brief = FileId {
            volume: 1,
            file: 999,
        };
        ids.ids.insert(PathBuf::from(path), brief);
    }
    ids
}

/// The check of `path` (refused `class`), and the card a tool call gets for it.
fn claim(path: &str, class: PathClass, confirmed: bool) -> (Decided, Offer) {
    let (fs, ids) = (fake(), ids());
    let net = plain_net(false);
    let captured = Captured::new(rules(), &disk(&fs, &net), &ids);
    let decided = decide(&captured, Path::new(path), class, &disk(&fs, &net), &ids);
    let (offer, _) = ruled(&decided, false, |_| confirmed);
    (decided, offer)
}

fn smith() -> OtherMatter {
    OtherMatter {
        matter: matter(2),
        name: "Smith".into(),
    }
}

fn v7(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 | n)
}

#[test]
fn a_tool_calls_claim_on_another_matter_gives_no_grant() {
    let (decided, offer) = claim(BRIEF, PathClass::Hidden, false);
    // An OS report of it would be offered, read-only; the claim alone is a plain line.
    assert!(decided.offer.offered);
    let kind = DenialKind::File {
        path: BRIEF.into(),
        other_matter: Some(smith()),
    };
    assert_eq!(
        offer,
        Offer {
            kind,
            offered: false
        }
    );
}

#[test]
fn the_same_file_confirmed_by_an_os_report_gives_the_read_only_card() {
    let (fs, ids) = (fake(), ids());
    let net = plain_net(false);
    let captured = Captured::new(rules(), &disk(&fs, &net), &ids);
    let (thread, turn) = (ThreadId::new(v7(1)).unwrap(), TurnId::new(v7(2)).unwrap());
    let mut denials = Denials::default();
    // The OS reports the file by its 8.3 folder name; the tool call by its own.
    let os = decide(
        &captured,
        Path::new("/home/a/Matters/SMITH~1/brief.docx"),
        PathClass::Hidden,
        &disk(&fs, &net),
        &ids,
    );
    let (os_offer, os_card) = ruled(&os, true, |_| false);
    let record = Record {
        kind: &os_offer.kind,
        offered: os_offer.offered,
        card: &os_card,
        asked: os.asked.as_ref(),
        os: true,
    };
    let mint = || Ok(DenialId::new(v7(3)).unwrap());
    let (recorded, _) = denials
        .record(thread, Some(turn), Some(turn), record, mint)
        .unwrap();
    assert!(matches!(recorded, Recorded::Log { count: 1, .. }));
    let tool = decide(
        &captured,
        Path::new(BRIEF),
        PathClass::Hidden,
        &disk(&fs, &net),
        &ids,
    );
    let (offer, card) = ruled(&tool, false, |asked| {
        denials.confirmed(thread, Some(turn), asked)
    });
    let kind = DenialKind::File {
        path: BRIEF.into(),
        other_matter: Some(smith()),
    };
    assert_eq!(
        offer,
        Offer {
            kind,
            offered: true
        }
    );
    assert!(card.same_file(&os_card), "{card:?} {os_card:?}");
}

#[test]
fn a_respelled_claim_is_judged_as_the_place_it_names() {
    // Another matter's root by a trailing dot: its root, never offered whole, not even
    // confirmed.
    let (_, root) = claim("/home/a/Matters/Smith.", PathClass::Outside, true);
    let kind = DenialKind::Folder {
        path: "/home/a/Matters/Smith".into(),
        other_matter: Some(smith()),
    };
    assert_eq!(
        root,
        Offer {
            kind,
            offered: false
        }
    );
    // Its `.clauseo` by its 8.3 name, and `.ssh` by its: never offered, even confirmed.
    for alias in [
        "/home/a/Matters/Smith/CLAUSE~1/notes.md",
        "/home/a/SSH~1/id_rsa",
    ] {
        let (decided, offer) = claim(alias, PathClass::Outside, true);
        assert!(!decided.offer.offered, "{alias}");
        assert!(!offer.offered, "{alias}");
        assert!(decided.facts.is_some_and(|facts| facts.never), "{alias}");
    }
    // A file the launch would deny, outside every place: one file, never its folder.
    let (decided, offer) = claim("/home/a/Deals/spa.docx", PathClass::Outside, false);
    assert!(matches!(decided.offer.kind, DenialKind::Folder { .. }));
    let kind = DenialKind::File {
        path: "/home/a/Deals/spa.docx".into(),
        other_matter: None,
    };
    assert_eq!(
        offer,
        Offer {
            kind,
            offered: true
        }
    );
}
