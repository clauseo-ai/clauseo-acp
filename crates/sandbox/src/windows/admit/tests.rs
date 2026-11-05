use std::io;
use std::path::{Path, PathBuf};

use super::*;
use crate::windows::acl_plan::plan;
use crate::windows::ledger::{Ledger, ledger_path};
use crate::windows::test_world::{FakeGroups, World, matter_one, matter_two, sids};

/// A folder check that answers `answer` for `folder` and `Ok` for every other path.
fn answering(
    folder: PathBuf,
    answer: fn() -> io::Result<FolderSupport>,
) -> impl FnMut(&Path) -> io::Result<FolderSupport> {
    move |path| {
        if path == folder {
            answer()
        } else {
            Ok(FolderSupport::Ok)
        }
    }
}

/// The steps a launch takes up to the first permission entry: the gate, the plan, the ledger's
/// pending entries. A refusal returns at the gate.
fn prepare(
    policy: &Policy,
    others: &[PathBuf],
    check: impl FnMut(&Path) -> io::Result<FolderSupport>,
    groups: &FakeGroups,
    ledger: &mut Ledger,
) -> Result<usize, Error> {
    let admitted = admit_with(policy, others, check, groups)?;
    let grants = plan(&admitted, &sids())?;
    let ids = ledger
        .record_pending(matter_one(), &grants)
        .map_err(Error::Probe)?;
    Ok(ids.len())
}

/// A ledger with one of another matter's entries already in it, and its bytes.
fn ledger_with_an_entry(w: &World) -> (Ledger, Vec<u8>) {
    let path = ledger_path(&w.at("data"));
    let mut ledger = Ledger::open(&path).unwrap();
    let other = w.builder().build().unwrap();
    let admitted = admit_with(
        &other,
        &[],
        |_| Ok(FolderSupport::Ok),
        &FakeGroups::default(),
    );
    let grants = plan(&admitted.unwrap(), &sids()).unwrap();
    ledger.record_pending(matter_two(), &grants[..2]).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    (ledger, bytes)
}

#[test]
fn every_grant_folder_is_checked() {
    let w = World::new("admit-checks-all");
    let policy = w.full_policy();
    let mut asked = Vec::new();
    let check = |path: &Path| {
        asked.push(path.to_owned());
        Ok(FolderSupport::Ok)
    };
    admit_with(&policy, &[], check, &FakeGroups::default()).unwrap();
    let want = [
        w.canonical("matter"),
        w.canonical("data/agent-state/m1/claude"),
        w.canonical("data/agent-state/m1/notes"),
        w.canonical("data/claude-home"),
    ];
    assert_eq!(asked, want);
}

#[test]
fn each_refused_folder_names_its_kind_and_not_its_path() {
    let w = World::new("admit-kinds");
    let policy = w.full_policy();
    let root = dunce::canonicalize(w.dir.path()).unwrap();
    let cases = [
        ("matter", PathKind::Matter),
        ("data/agent-state/m1/claude", PathKind::State),
        ("data/agent-state/m1/notes", PathKind::Notes),
        ("data/claude-home", PathKind::ClaudeStore),
    ];
    let answers: [fn() -> io::Result<FolderSupport>; 4] = [
        || Ok(FolderSupport::CantChangePermissions),
        || Ok(FolderSupport::Network),
        || Ok(FolderSupport::NoPermissions),
        || {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "C:\\m\\Client v Other is missing",
            ))
        },
    ];
    for (rel, kind) in cases {
        for answer in answers {
            let check = answering(w.canonical(rel), answer);
            let error = admit_with(&policy, &[], check, &FakeGroups::default()).unwrap_err();
            let Error::FolderUnsupported { path_kind, support } = &error else {
                panic!("{error:?}");
            };
            assert_eq!(*path_kind, kind);
            let want = match answer() {
                Ok(support) => FolderRefusal::Support(support),
                Err(error) => FolderRefusal::CheckFailed(error.kind()),
            };
            assert_eq!(*support, want);
            let text = format!("{error} {error:?}");
            assert!(!text.contains(&*root.to_string_lossy()), "a path in {text}");
            assert!(
                !text.contains("Client v Other"),
                "the io error's text in {text}"
            );
        }
    }
}

#[test]
fn a_refusal_leaves_the_ledger_byte_identical() {
    let w = World::new("admit-ledger-unchanged");
    let policy = w.full_policy();
    let (mut ledger, before) = ledger_with_an_entry(&w);
    let path = ledger.path().to_owned();
    for rel in [
        "matter",
        "data/agent-state/m1/claude",
        "data/agent-state/m1/notes",
    ] {
        let check = answering(w.canonical(rel), || Ok(FolderSupport::Network));
        let refused = prepare(&policy, &[], check, &FakeGroups::default(), &mut ledger);
        assert!(matches!(refused, Err(Error::FolderUnsupported { .. })));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "after a refusal on {rel}"
        );
    }
    // The same launch, admitted, does write: the refusal is what kept the file as it was.
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let planned = prepare(&policy, &[], check, &FakeGroups::default(), &mut ledger).unwrap();
    assert!(planned > 0);
    assert_ne!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn a_refusal_on_a_fresh_data_folder_makes_no_ledger() {
    let w = World::new("admit-no-ledger");
    let policy = w.full_policy();
    let path = ledger_path(&w.at("data"));
    let mut ledger = Ledger::open(&path).unwrap();
    let check = answering(w.canonical("matter"), || Ok(FolderSupport::NoPermissions));
    assert!(prepare(&policy, &[], check, &FakeGroups::default(), &mut ledger).is_err());
    assert!(!path.exists());
    assert!(!w.at("data/sandbox").exists());
}

#[test]
fn other_matters_a_group_can_reach_are_closed() {
    let w = World::new("admit-others");
    let policy = w.builder().build().unwrap();
    let (m2, m3) = (w.canonical("other/m2"), w.canonical("other/m3"));
    let unread = w.at("other/unread");
    let gone = w.at("other/gone");
    let groups = FakeGroups {
        open: vec![m2.clone()],
        failing: vec![unread.clone()],
        missing: vec![gone.clone()],
        ..FakeGroups::default()
    };
    let others = [m2.clone(), m3, unread.clone(), gone];
    let admitted = admit_with(&policy, &others, |_| Ok(FolderSupport::Ok), &groups).unwrap();
    // A DACL that can't be read counts as open; a missing folder has nothing to close.
    assert_eq!(admitted.closed(), [m2, unread]);
    assert_eq!(groups.asked.borrow().len(), 4);
}

#[test]
fn another_matter_that_cant_be_closed_refuses_the_launch() {
    let w = World::new("admit-others-refused");
    let policy = w.builder().build().unwrap();
    let m2 = w.canonical("other/m2");
    let groups = FakeGroups {
        open: vec![m2.clone()],
        ..FakeGroups::default()
    };
    let (mut ledger, before) = ledger_with_an_entry(&w);
    for answer in [
        FolderSupport::CantChangePermissions,
        FolderSupport::NoPermissions,
    ] {
        let check = |path: &Path| {
            Ok(if path == m2 {
                answer
            } else {
                FolderSupport::Ok
            })
        };
        let error = prepare(
            &policy,
            std::slice::from_ref(&m2),
            check,
            &groups,
            &mut ledger,
        )
        .unwrap_err();
        let Error::FolderUnsupported { path_kind, support } = error else {
            panic!("{error:?}");
        };
        assert_eq!(path_kind, PathKind::OtherMatter);
        assert_eq!(support, FolderRefusal::Support(answer));
        assert_eq!(std::fs::read(ledger.path()).unwrap(), before);
    }
}

#[test]
fn another_matter_on_a_share_or_gone_is_left_alone() {
    let w = World::new("admit-others-network");
    let policy = w.builder().build().unwrap();
    let m2 = w.canonical("other/m2");
    let groups = FakeGroups {
        open: vec![m2.clone()],
        ..FakeGroups::default()
    };
    let network = |path: &Path| {
        Ok(if path == m2 {
            FolderSupport::Network
        } else {
            FolderSupport::Ok
        })
    };
    let admitted = admit_with(&policy, std::slice::from_ref(&m2), network, &groups).unwrap();
    assert_eq!(admitted.closed(), [] as [PathBuf; 0]);
    let gone = |path: &Path| {
        if path == m2 {
            Err(io::Error::from(io::ErrorKind::NotFound))
        } else {
            Ok(FolderSupport::Ok)
        }
    };
    let admitted = admit_with(&policy, std::slice::from_ref(&m2), gone, &groups).unwrap();
    assert_eq!(admitted.closed(), [] as [PathBuf; 0]);
}

#[test]
fn other_matters_are_not_read_when_a_grant_folder_is_refused() {
    let w = World::new("admit-order");
    let policy = w.builder().build().unwrap();
    let groups = FakeGroups::default();
    let check = answering(w.canonical("matter"), || Ok(FolderSupport::Network));
    let others = [w.canonical("other/m2")];
    assert!(admit_with(&policy, &others, check, &groups).is_err());
    assert!(groups.asked.borrow().is_empty());
}

/// The gate with the real folder check, on CI's Windows job.
#[cfg(windows)]
mod on_windows {
    use super::*;
    use crate::test_support::remove_dir;
    use crate::windows::folder_support;
    use crate::windows::test_dacl::protect_with;
    use crate::windows::tests::admin_share_path;

    /// `OWNER RIGHTS` with Modify only: the owner loses `WRITE_DAC`.
    const OWNER_RIGHTS_MODIFY: &str = "D:P(A;OICI;0x1301bf;;;OW)";

    /// Asserts that `policy` is refused for `kind` with `want`, and that the ledger is as it was.
    fn assert_refused(w: &World, policy: &Policy, kind: PathKind, want: FolderRefusal) {
        let (mut ledger, before) = ledger_with_an_entry(w);
        let groups = FakeGroups::default();
        let admitted = admit(policy, &[], &groups);
        let Err(Error::FolderUnsupported { path_kind, support }) = admitted else {
            panic!("not refused: {admitted:?}");
        };
        assert_eq!((path_kind, support), (kind, want));
        // The launch's own steps refuse at the gate too, before a plan or an entry.
        let refused = prepare(policy, &[], folder_support, &groups, &mut ledger);
        assert!(matches!(refused, Err(Error::FolderUnsupported { .. })));
        assert_eq!(std::fs::read(ledger.path()).unwrap(), before);
    }

    /// Whether this machine has the admin share CI's runners have; elsewhere the test passes.
    fn has_admin_share(w: &World) -> bool {
        if admin_share_path(w.dir.path()).is_dir() {
            return true;
        }
        assert!(
            std::env::var_os("CI").is_none(),
            "CI's Windows runners have the admin shares, and this run has none"
        );
        eprintln!("skipped: this machine has no admin share for the temp folder");
        false
    }

    #[test]
    fn a_local_world_is_admitted() {
        let w = World::new("admit-win-ok");
        let policy = w.full_policy();
        let admitted = admit(&policy, &[], &FakeGroups::default()).unwrap();
        assert_ne!(plan(&admitted, &sids()).unwrap().len(), 0);
    }

    #[test]
    fn a_matter_whose_permissions_cant_change_is_refused() {
        let w = World::new("admit-win-cant-change");
        let policy = w.full_policy();
        protect_with(&w.at("matter"), OWNER_RIGHTS_MODIFY).unwrap();
        let want = FolderRefusal::Support(FolderSupport::CantChangePermissions);
        assert_refused(&w, &policy, PathKind::Matter, want);
    }

    #[test]
    fn a_notes_folder_whose_permissions_cant_change_is_refused() {
        let w = World::new("admit-win-notes-cant-change");
        let policy = w.full_policy();
        protect_with(&w.at("data/agent-state/m1/notes"), OWNER_RIGHTS_MODIFY).unwrap();
        let want = FolderRefusal::Support(FolderSupport::CantChangePermissions);
        assert_refused(&w, &policy, PathKind::Notes, want);
    }

    #[test]
    fn a_matter_on_a_share_is_refused() {
        let w = World::new("admit-win-network-matter");
        if !has_admin_share(&w) {
            return;
        }
        let policy = Policy::builder(
            admin_share_path(&w.at("matter")),
            w.at("data/agent-state/m1/claude"),
        )
        .home(w.at("home"))
        .build()
        .unwrap();
        let want = FolderRefusal::Support(FolderSupport::Network);
        assert_refused(&w, &policy, PathKind::Matter, want);
    }

    #[test]
    fn a_state_folder_on_a_share_refuses_the_whole_launch() {
        // The matter is local and fine; the state folder alone, through `\\localhost\C$`, is
        // enough to refuse.
        let w = World::new("admit-win-network-state");
        if !has_admin_share(&w) {
            return;
        }
        let policy = Policy::builder(
            w.at("matter"),
            admin_share_path(&w.at("data/agent-state/m1/claude")),
        )
        .home(w.at("home"))
        .build()
        .unwrap();
        assert_eq!(folder_support(policy.matter()).unwrap(), FolderSupport::Ok);
        let want = FolderRefusal::Support(FolderSupport::Network);
        assert_refused(&w, &policy, PathKind::State, want);
    }

    #[test]
    fn a_missing_folder_is_refused() {
        let w = World::new("admit-win-missing");
        let policy = w.full_policy();
        remove_dir(&w.at("data/agent-state/m1/notes"));
        let want = FolderRefusal::CheckFailed(io::ErrorKind::NotFound);
        assert_refused(&w, &policy, PathKind::Notes, want);
    }
}
