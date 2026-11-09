//! S15 owned roots on NTFS: under a parent that gives `Users` `LC` and `DC` down its folders, as
//! `C:\` does (the Windows VM's observer run: the data folder sat directly under `C:\`), the
//! folders Clauseo owns for a launch end protected, with exactly the plan's entries, and nothing
//! inside them, made before the launch or after, holds a `Users` entry or any other broad one.
//! The elevated row tries it as another pool user's Low restricted token.

use std::path::{Path, PathBuf};

use super::Around;
use crate::windows::acl::owner;
use crate::windows::acl_bytes::{
    Acl, CONTAINER_INHERIT, Descriptor, INHERITED, OBJECT_INHERIT, flags, kind, mask, sid, sid_text,
};
use crate::windows::acl_plan::{
    AdmittedPlan, Inherit, LOW_LABEL_SID, OwnedKind, Rights, Sid, Subjects, owned_sddl, plan,
};
use crate::windows::admit::admit_with;
use crate::windows::apply::apply;
use crate::windows::fence::Neighbours;
use crate::windows::ledger::{Ledger, ledger_path};
use crate::windows::record::current_user_sid;
use crate::windows::test_acl::{elevated_tests_enabled, entries, grant, has, protected};
use crate::windows::test_dacl::{protect_with, sddl_bytes};
use crate::windows::test_junction::junction;
use crate::windows::test_pool::{Impersonating, TestUser, log_on, low_restricted_token};
use crate::windows::test_world::{
    FakeFolders, NOTES, OTHER_POOL_SIDS, STATE, World, matter_one, reserved, sids, trusted,
};
use crate::windows::writer::AclWriter as _;
use crate::windows::{FolderSupport, NtfsAcl};

/// `C:\`'s entries for `Users` (`icacls C:\`): read and run down the tree, and `LC` (add a
/// folder) and `DC` (add a file) down the folders: the two that let an agent plant files.
const LIKE_C_ROOT: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;OW)\
                           (A;OICI;0x1200a9;;;BU)(A;CI;LC;;;BU)(A;CI;DC;;;BU)";

/// The SIDs no owned folder may name, nor anything in one: `Users`, `Authenticated Users`,
/// `Everyone`, `CREATOR OWNER`, `INTERACTIVE`, and a pool SID the plan never heard of.
const BROAD: [&str; 6] = [
    "S-1-5-32-545",
    "S-1-5-11",
    "S-1-1-0",
    "S-1-3-0",
    "S-1-5-4",
    "S-1-5-21-1111111111-2222222222-3333333333-1004",
];

/// The session's run folder, relative to the world.
const RUN: &str = "data/run/s1";

/// A world whose root hands `C:\`'s `Users` entries down, the session's run, work and temp
/// folders and the state folder's `AppData\Roaming` made under it, and matter one's plan with
/// notes and the temp folder.
fn session(test: &str) -> (World, AdmittedPlan) {
    let w = World::new(test);
    protect_with(w.dir.path(), LIKE_C_ROOT).unwrap();
    for rel in [
        &format!("{RUN}/tmp"),
        &format!("{RUN}/work"),
        &format!("{STATE}/AppData/Roaming"),
    ] {
        w.dir.dir(rel);
    }
    let state = w.canonical(STATE);
    assert!(has(&state, BROAD[0], true), "the parent hands Users down");
    let plan = session_plan(&w);
    (w, plan)
}

/// Matter one's plan for `w`, with notes and the session's temp folder.
fn session_plan(w: &World) -> AdmittedPlan {
    session_plan_for(w, &sids())
}

/// [`session_plan`] with `subjects`.
fn session_plan_for(w: &World, subjects: &Subjects) -> AdmittedPlan {
    let policy = w
        .builder()
        .notes(w.at(NOTES))
        .tmpdir(w.at(&format!("{RUN}/tmp")))
        .build()
        .unwrap();
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let none = FakeFolders::default();
    let admitted = admit_with(&policy, &w.at("data"), &Neighbours::default(), ok, &none).unwrap();
    plan(&admitted, subjects, matter_one()).unwrap()
}

fn launch(w: &World, plan: &mut AdmittedPlan) {
    let mut ledger = Ledger::open(ledger_path(&w.at("data")), &trusted(), &reserved()).unwrap();
    apply(&mut ledger, plan, &mut NtfsAcl::new().unwrap(), &mut Around).unwrap();
}

/// A DACL's entries as (type, flags, mask, SID), sorted.
fn dacl_entries(descriptor: &[u8]) -> Vec<(u8, u8, u32, Sid)> {
    let read = Descriptor::parse(descriptor).unwrap();
    let mut out: Vec<(u8, u8, u32, Sid)> = Acl::parse(read.dacl.unwrap())
        .unwrap()
        .aces()
        .iter()
        .filter_map(|ace| {
            Some((
                kind(ace),
                flags(ace),
                mask(ace)?,
                sid(ace).and_then(sid_text)?,
            ))
        })
        .collect();
    out.sort();
    out
}

/// Whether `path` names any of [`BROAD`], inherited or not.
fn names_broad(path: &Path) -> Option<Sid> {
    entries(path)
        .into_iter()
        .map(|(_, _, _, s)| s)
        .find(|s| BROAD.contains(&s.as_str()))
}

#[test]
fn owned_roots_are_protected_with_exactly_the_plans_entries_and_nothing_inside_is_broad() {
    let (w, mut plan) = session("owned-ntfs");
    let roots: Vec<PathBuf> = plan
        .owned_roots()
        .iter()
        .map(|r| r.path().to_owned())
        .collect();
    assert_eq!(roots.len(), 9, "{roots:?}");
    // Another session's recorded allow on a sign-in file stays through the strict walk (N7).
    let sign_in = w
        .dir
        .file("data/agents/claude-acp/home/.credentials.json", "{}");
    let theirs = grant(
        &sign_in,
        OTHER_POOL_SIDS[1],
        Rights::Modify,
        Inherit::ThisOnly,
    );
    NtfsAcl::new().unwrap().add(&theirs).unwrap();
    launch(&w, &mut plan);
    let me = current_user_sid().unwrap();
    for root in plan.owned_roots() {
        let path = root.path();
        assert!(protected(path), "{path:?} protected");
        let want = dacl_entries(&sddl_bytes(&owned_sddl(&plan, root, &me)).unwrap());
        let got = dacl_entries(&crate::windows::acl::descriptor(path).unwrap());
        assert_eq!(got, want, "{:?} {path:?}", root.kind());
        assert_eq!(names_broad(path), None, "{path:?}");
        let labelled = matches!(
            root.kind(),
            OwnedKind::State | OwnedKind::Notes | OwnedKind::TempFolder
        );
        assert_eq!(has(path, LOW_LABEL_SID, false), labelled, "{path:?} label");
    }
    // What was there before the launch, and what is made after: only inherited entries, from
    // the protected roots, none of them broad.
    let made_after = [
        w.dir.dir(&format!("{STATE}/AppData/Local")),
        w.dir.file(&format!("{RUN}/tmp/scratch.txt"), "x"),
        w.dir.file(&format!("{NOTES}/note.md"), "x"),
    ];
    let before = [
        w.canonical(&format!("{STATE}/AppData")),
        w.canonical(&format!("{STATE}/AppData/Roaming")),
        w.canonical(&format!("{RUN}/work")),
    ];
    for path in before.iter().chain(&made_after) {
        let all = entries(path);
        assert!(!all.is_empty(), "{path:?}");
        assert!(
            all.iter().all(|(_, f, _, _)| f & INHERITED != 0),
            "{path:?} {all:?}"
        );
        assert_eq!(names_broad(path), None, "{path:?} {all:?}");
    }
    // OWNER RIGHTS: what the agent makes in its own folders leaves its owner no WRITE_DAC. The
    // roots' entry is `OI|CI|IO`: a file inherits it bare, a folder keeps `OI|CI` to hand on.
    for path in &made_after {
        let handed_on = if path.is_dir() {
            OBJECT_INHERIT | CONTAINER_INHERIT
        } else {
            0
        };
        let owner_rights = (
            0,
            handed_on | INHERITED,
            Rights::Modify.mask(),
            Sid::parse("S-1-3-4").unwrap(),
        );
        let all = entries(path);
        assert!(all.contains(&owner_rights), "{path:?} {all:?}");
    }
    // The data folder's other contents, there before the launch, are walked once: the Claude
    // sign-in folder and the host's `updates` keep no Users entry.
    for rel in ["data/agents/claude-acp/home", "data/updates"] {
        let path = w.canonical(rel);
        assert_eq!(names_broad(&path), None, "{rel}: {:?}", entries(&path));
    }
    assert!(w.at("data/sandbox/owned-walked").exists());
    assert!(
        has(&sign_in, OTHER_POOL_SIDS[1], false),
        "another session's allow stays"
    );
    // A second launch finds every root as it left it and writes none again.
    let before_again: Vec<Vec<u8>> = roots
        .iter()
        .map(|p| crate::windows::acl::descriptor(p).unwrap())
        .collect();
    let mut again = session_plan(&w);
    launch(&w, &mut again);
    for (path, then) in roots.iter().zip(&before_again) {
        assert_eq!(
            &crate::windows::acl::descriptor(path).unwrap(),
            then,
            "{path:?}"
        );
    }
}

/// Making a file, then a folder, in `folder` as the impersonated token: `Ok`, or the Win32
/// error each gives.
#[expect(
    clippy::disallowed_methods,
    reason = "the test makes files as another pool user's token, as its agent would"
)]
fn try_make(folder: &Path) -> [Result<(), i32>; 2] {
    let code = |e: std::io::Error| e.raw_os_error().unwrap_or(-1);
    [
        std::fs::write(folder.join("planted.json"), b"{}").map_err(code),
        std::fs::create_dir(folder.join("planted")).map_err(code),
    ]
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_another_pool_users_low_token_plants_nothing_in_the_owned_folders() {
    if !elevated_tests_enabled() {
        return;
    }
    let (w, mut plan) = session("elevated-owned-ntfs");
    // The control: a Low-labelled folder still under the parent's `Users` entries, as the
    // state folder was before (the finding).
    let control = w.dir.dir("control");
    let label = grant(&control, LOW_LABEL_SID, Rights::LowLabel, Inherit::Tree);
    NtfsAcl::new().unwrap().add(&label).unwrap();
    launch(&w, &mut plan);
    let user = TestUser::new();
    let token = low_restricted_token(&user);
    let folders = [
        w.canonical(STATE),
        w.canonical(&format!("{STATE}/AppData/Roaming")),
        w.canonical(NOTES),
        w.canonical(&format!("{RUN}/tmp")),
    ];
    let (control_made, made) = {
        let _as_pool = Impersonating::start(&token);
        let made: Vec<[Result<(), i32>; 2]> = folders.iter().map(|f| try_make(f)).collect();
        (try_make(&control), made)
    };
    assert_eq!(
        control_made,
        [Ok(()), Ok(())],
        "the control: Users' LC and DC let it"
    );
    for (folder, got) in folders.iter().zip(&made) {
        assert_eq!(got, &[Err(5), Err(5)], "must refuse: {folder:?}");
    }
}

/// Makes `folder`, a file in it and a junction in it to `target`, as the impersonated token.
#[expect(
    clippy::disallowed_methods,
    reason = "the test makes a run folder as another matter's agent would, before Clauseo does"
)]
fn plant(folder: &Path, target: &Path) {
    std::fs::create_dir(folder).unwrap();
    std::fs::write(folder.join("planted.json"), b"{}").unwrap();
    junction(&folder.join("j"), target).unwrap();
}

/// The review's L1: another matter's agent makes the session's run folder first, with a file
/// and a junction in it. The launch hands the folder, the file and the junction itself (never
/// what it names) to the lawyer, so that agent keeps no `WRITE_DAC` on any of them.
#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_run_folder_another_agent_made_first_goes_to_the_lawyer_with_what_it_planted() {
    if !elevated_tests_enabled() {
        return;
    }
    let w = World::new("elevated-owned-planted");
    protect_with(w.dir.path(), LIKE_C_ROOT).unwrap();
    w.dir.dir("data/run");
    let elsewhere = w.canonical("elsewhere");
    let before = crate::windows::acl::descriptor(&elsewhere).unwrap();
    let user = TestUser::new();
    let run = w.at(RUN);
    {
        let _as_agent = Impersonating::start(&log_on(&user));
        plant(&run, &elsewhere);
    }
    let run = w.canonical(RUN);
    let (planted, link) = (run.join("planted.json"), run.join("j"));
    for path in [&run, &planted, &link] {
        assert_eq!(owner(path).unwrap(), user.sid, "{path:?} made by the agent");
    }
    w.dir.dir(&format!("{RUN}/tmp"));
    let mut others: Vec<Sid> = OTHER_POOL_SIDS
        .iter()
        .map(|s| Sid::parse(s).unwrap())
        .collect();
    others.push(user.sid.clone());
    let subjects = Subjects {
        other_pool_users: others,
        ..sids()
    };
    let mut plan = session_plan_for(&w, &subjects);
    launch(&w, &mut plan);
    let me = current_user_sid().unwrap();
    for path in [&run, &planted, &link] {
        assert_eq!(owner(path).unwrap(), me, "{path:?} handed to the lawyer");
        assert_eq!(names_broad(path), None, "{path:?}");
    }
    assert!(protected(&run));
    let after = crate::windows::acl::descriptor(&elsewhere).unwrap();
    assert_eq!(after, before, "what the junction names is never touched");
}
