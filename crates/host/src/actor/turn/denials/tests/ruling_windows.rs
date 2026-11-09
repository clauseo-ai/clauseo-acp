//! Windows' twin of `ruling.rs` (tool calls are Windows' only denial source), through
//! `outside_jail`, `decide` and `ruled` on a fake disk with the `C:\` places of
//! `identity::tests::windows_places` and their 8.3 names: a path in the matter or the data
//! folder, by any spelling, is no denial and one outside is; a tool call's claim on another
//! matter gives no grant, and the same file confirmed by an OS report gives the read-only card;
//! a respelled place is judged as the place it names, a trailing dot refused unresolved; a
//! network path, or a link in the matter to one, is refused untouched. Windows only: a `C:\`
//! path is absolute there alone.

use std::path::{Path, PathBuf};

use clauseo_protocol::{DenialId, DenialKind, OtherMatter, ThreadId, TurnId};
use clauseo_sandbox::PathClass;
use uuid::Uuid;

use super::super::check::{Decided, Unchecked, decide, outside_jail};
use super::super::count::{Denials, Record, Recorded};
use super::super::offer::Offer;
use super::super::offer::identity::tests::windows_places::{
    ACME, SMITH, mapped_net, win_ids, win_rules,
};
use super::super::offer::identity::tests::{FakeIds, matter};
use super::super::offer::identity::{Captured, FileId};
use super::super::ruled;
use super::disk;
use crate::network_paths::resolve::tests::{Fake, FakeFs};

const BRIEF: &str = r"C:\Users\a\Matters\Smith\brief.docx";
const SHORT_BRIEF: &str = r"C:\Users\a\Matters\SMITH~1\brief.docx";
const SPA: &str = r"C:\Users\a\Deals\spa.docx";

fn fake() -> FakeFs {
    FakeFs::with(&[
        (BRIEF, Fake::File),
        (SHORT_BRIEF, Fake::File),
        (r"C:\Users\a\Matters\Smith\CLAUSE~1\notes.md", Fake::File),
        (r"C:\Users\a\SSH~1\id_rsa", Fake::File),
        (SPA, Fake::File),
        (r"C:\Users\a\Matters\Acme\brief.docx", Fake::File),
        (
            r"C:\Users\a\Matters\Acme\share",
            Fake::Link(r"\\server\share".into()),
        ),
    ])
}

fn ids() -> FakeIds {
    let mut ids = win_ids();
    for path in [BRIEF, SHORT_BRIEF] {
        let brief = FileId {
            volume: 1,
            file: 999,
        };
        ids.ids.insert(PathBuf::from(path), brief);
    }
    ids
}

/// The session's capture on `fs`, Windows' rules with `Z:` mapped to a share.
fn captured(fs: &FakeFs, ids: &FakeIds) -> Captured {
    Captured::new(win_rules(), &disk(fs, &mapped_net()), ids)
}

/// The check of `path` (refused `class`), and the card a tool call gets for it.
fn claim(path: &str, class: PathClass, confirmed: bool) -> (Decided, Offer) {
    let (fs, ids, net) = (fake(), ids(), mapped_net());
    let captured = captured(&fs, &ids);
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
fn a_tool_calls_path_is_a_denial_only_outside_the_jail() {
    let (fs, ids, net) = (fake(), ids(), mapped_net());
    let captured = captured(&fs, &ids);
    let jail = |named: &[&str]| {
        let named: Vec<String> = named.iter().map(|path| (*path).to_owned()).collect();
        outside_jail(Path::new(ACME), &captured, &named, &disk(&fs, &net), &ids)
    };
    // The matter, relative, absolute, in another case and by its 8.3 name, and the data folder
    // by its 8.3 name: nothing.
    let inside = [
        "brief.docx",
        r"C:\Users\a\Matters\Acme\brief.docx",
        r"c:\users\a\matters\acme\brief.docx",
        r"C:\Users\a\Matters\ACME~1\brief.docx",
        r"C:\Users\a\AppData\Roaming\CLAUSE~1\run\x",
    ];
    assert_eq!(jail(&inside), None);
    // Outside: the first path that is, another matter's too.
    let spa = Some((PathBuf::from(SPA), PathClass::Outside));
    assert_eq!(jail(&["brief.docx", SPA]), spa);
    assert_eq!(
        jail(&[BRIEF]),
        Some((PathBuf::from(BRIEF), PathClass::Outside))
    );
    // A `..` out of the matter is outside, not a link.
    assert_eq!(jail(&[r"..\..\Deals\spa.docx"]), spa);
}

#[test]
fn a_network_path_from_a_tool_call_is_refused_untouched() {
    let (fs, ids, net) = (fake(), ids(), mapped_net());
    let captured = captured(&fs, &ids);
    let before = fs.touched().len();
    for path in [
        r"\\server\share\brief.docx",
        "//server/share/brief.docx",
        r"\\?\UNC\server\share\brief.docx",
        r"\\.\UNC\server\share\brief.docx",
        r"Z:\Matters\brief.docx",
    ] {
        let named = [path.to_owned()];
        let got = outside_jail(Path::new(ACME), &captured, &named, &disk(&fs, &net), &ids);
        assert_eq!(
            got,
            Some((PathBuf::from(path), PathClass::Outside)),
            "{path}"
        );
        let decided = decide(
            &captured,
            Path::new(path),
            PathClass::Outside,
            &disk(&fs, &net),
            &ids,
        );
        assert_eq!(decided.unchecked, Some(Unchecked::Network), "{path}");
        let (offer, _) = ruled(&decided, false, |_| true);
        assert!(!decided.offer.offered && !offer.offered, "{path}");
    }
    assert_eq!(fs.touched().len(), before, "{:?}", fs.touched());

    // Through a link in the matter: stopped at the link, the share never looked at.
    let named = [r"share\x.docx".to_owned()];
    let got = outside_jail(Path::new(ACME), &captured, &named, &disk(&fs, &net), &ids);
    let through = PathBuf::from(r"C:\Users\a\Matters\Acme\share\x.docx");
    assert_eq!(got, Some((through.clone(), PathClass::LinkTarget)));
    let touched = fs.touched();
    assert!(
        touched
            .iter()
            .all(|p| !p.starts_with(&through) && !p.to_string_lossy().starts_with(r"\\")),
        "{touched:?}"
    );
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
    let (fs, ids, net) = (fake(), ids(), mapped_net());
    let captured = captured(&fs, &ids);
    let (thread, turn) = (ThreadId::new(v7(1)).unwrap(), TurnId::new(v7(2)).unwrap());
    let mut denials = Denials::default();
    // The OS reports the file by its 8.3 folder name; the tool call by its own.
    let os = decide(
        &captured,
        Path::new(SHORT_BRIEF),
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
    // Another matter's root by a trailing dot: a name Windows reads as another place, refused
    // unresolved before anything is touched; never offered, not even confirmed.
    let (decided, root) = claim(r"C:\Users\a\Matters\Smith.", PathClass::Outside, true);
    assert_eq!(decided.unchecked, Some(Unchecked::Unresolved));
    assert!(!root.offered);
    // By its 8.3 name: its root, never offered whole, not even confirmed.
    let (_, root) = claim(r"C:\Users\a\Matters\SMITH~1", PathClass::Outside, true);
    let kind = DenialKind::Folder {
        path: SMITH.into(),
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
        r"C:\Users\a\Matters\Smith\CLAUSE~1\notes.md",
        r"C:\Users\a\SSH~1\id_rsa",
    ] {
        let (decided, offer) = claim(alias, PathClass::Outside, true);
        assert!(!decided.offer.offered, "{alias}");
        assert!(!offer.offered, "{alias}");
        assert!(decided.facts.is_some_and(|facts| facts.never), "{alias}");
    }
    // A file the launch would deny, outside every place: one file, never its folder.
    let (decided, offer) = claim(SPA, PathClass::Outside, false);
    assert!(matches!(decided.offer.kind, DenialKind::Folder { .. }));
    let kind = DenialKind::File {
        path: SPA.into(),
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
