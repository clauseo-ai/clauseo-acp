//! Round 3 of the S15 `PR3b` review on NTFS: the scan matches names as NTFS opens them, a
//! folder the lawyer can't list makes a partial scan, and the exit check sets aside what the
//! agent planted even when the repair after the session refused at its cap (R1).

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_protocol::MatterId;

use super::super::ntfs::Ntfs;
use super::super::run::Volume as _;
use super::{after_session, before_launch, exit, launch, list_set_aside, set_aside_path};
use crate::Error;
use crate::windows::acl_plan::{AdmittedPlan, Inherit, Rights};
use crate::windows::admit::{FolderList, ReadDir};
use crate::windows::ledger::{Ledger, ledger_path};
use crate::windows::set_aside::Reason;
use crate::windows::test_acl::grant;
use crate::windows::test_dacl::{drop_privileges, protect_with};
use crate::windows::test_world::{USER_SID, World, matter_one, reserved, trusted};
use crate::windows::{
    ApplyFailure, FolderSupport, NtfsAcl, Surroundings, apply, repair_after_session,
};

/// A DACL that shuts the lawyer (the owner, this process's user) out of a file or folder:
/// `OWNER RIGHTS` with nothing, so the owner keeps no implicit right.
const SHUT: &str = "D:P(A;;FA;;;SY)(A;;0x0;;;OW)";

/// Names as NTFS matches them: a folder named `.GIT` is what the `.git` probe opens, so it
/// is set aside under the protected name.
#[test]
fn a_folder_named_git_in_capitals_is_found_by_the_git_probe() {
    let w = World::new("ntfs-aside-upper-git");
    w.dir.dir("matter/sub");
    let policy = w.builder().build().unwrap();
    let data = w.canonical("data");
    let root = policy.matter().to_owned();
    before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();

    w.dir.file("matter/sub/.GIT/hooks/pre-commit", "exit 0\r\n");
    let checked = after_session(&policy, &data, &matter_one(), "s1", exit()).unwrap();

    let seen: Vec<_> = checked
        .moved
        .iter()
        .map(|e| (e.from.clone(), e.reason))
        .collect();
    assert_eq!(seen, [(root.join("sub").join(".git"), Reason::New)]);
    assert!(checked.moved[0].to.join("hooks/pre-commit").is_file());
    assert!(!root.join("sub/.GIT").exists());
}

/// Names as NTFS matches them: whether `.gıt` (a dotless i) is `.git` is NTFS's answer, not
/// ours. The scan sets it aside exactly when a plain case-insensitive open of `.git` opens it.
#[test]
fn a_dotless_i_git_is_set_aside_exactly_when_ntfs_opens_it_as_git() {
    let w = World::new("ntfs-aside-dotless-i");
    w.dir.dir("matter/sub");
    let policy = w.builder().build().unwrap();
    let data = w.canonical("data");
    let root = policy.matter().to_owned();
    before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();

    w.dir
        .file("matter/sub/.g\u{131}t/HEAD", "ref: refs/heads/main\n");
    let opened = Ntfs.file_id(&root.join("sub").join(".git")).unwrap();
    println!("NTFS opens .g\u{131}t as .git: {}", opened.is_some());
    let checked = after_session(&policy, &data, &matter_one(), "s1", exit()).unwrap();

    let from: Vec<_> = checked.moved.iter().map(|e| e.from.clone()).collect();
    if opened.is_some() {
        assert_eq!(from, [root.join("sub").join(".git")]);
    } else {
        assert_eq!(from, Vec::<PathBuf>::new());
        assert!(root.join("sub/.g\u{131}t/HEAD").is_file());
    }
}

/// R1: a folder whose DACL shuts the lawyer out can't be listed. The pass sets aside what it
/// found elsewhere, names the folder, and keeps the session open, so the next launch checks.
#[test]
fn a_folder_the_lawyer_cant_list_makes_a_partial_scan_that_keeps_the_session_open() {
    drop_privileges();
    let w = World::new("ntfs-aside-partial");
    let locked = w.dir.dir("matter/locked");
    let policy = w.builder().build().unwrap();
    let data = w.canonical("data");
    let root = policy.matter().to_owned();
    before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();

    w.dir.file("matter/.claude/settings.json", "{}");
    // The owner keeps `WRITE_DAC` only, to give the folder back at the end.
    protect_with(&locked, "D:P(A;;FA;;;SY)(A;;0x40000;;;OW)").unwrap();
    let checked = after_session(&policy, &data, &matter_one(), "s1", exit()).unwrap();

    let from: Vec<_> = checked.moved.iter().map(|e| e.from.clone()).collect();
    assert_eq!(from, [root.join(".claude")]);
    assert!(checked.partial());
    assert_eq!(checked.unscanned, [root.join("locked")]);
    w.dir.file("matter/.mcp.json", "{}");
    let later = exit() + Duration::from_secs(60);
    let next = before_launch(&policy, &data, &matter_one(), "s2", &[], later).unwrap();
    let names: Vec<_> = next
        .moved
        .iter()
        .map(|e| e.name.to_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [".mcp.json"],
        "s1 stayed open, so this launch checked"
    );
    protect_with(&locked, "D:P(A;OICI;FA;;;OW)").unwrap();
}

/// Around a temp matter for the R1 case: the real listing, every folder `Ok`, and each child
/// the lawyer can't open set aside through the real set-aside.
struct Aside {
    data: PathBuf,
}

impl FolderList for Aside {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        ReadDir.subfolders(parent)
    }
}

impl Surroundings for Aside {
    fn check(&self, _folder: &Path) -> io::Result<FolderSupport> {
        Ok(FolderSupport::Ok)
    }

    fn set_aside(&mut self, matter: MatterId, path: &Path) -> io::Result<()> {
        let now = exit() - Duration::from_secs(60);
        set_aside_path(&self.data, &matter, path, Reason::LawyerCantOpen, now)
            .map(drop)
            .map_err(io::Error::other)
    }

    fn credentials(&self) -> Option<PathBuf> {
        None
    }
}

/// R1, the reviewer's case through the real writer: the agent makes 65 children the lawyer
/// can't open, and plants a root `.git` and `sub\.git`. `repair_after_session` sets 64 aside,
/// then refuses at the cap. `after_session` still sets aside both `.git`s.
#[test]
fn a_repair_refused_at_the_set_aside_cap_still_leaves_both_gits_to_the_exit_check() {
    drop_privileges();
    let w = World::new("ntfs-aside-cap");
    w.dir.dir("matter/sub");
    let policy = w.builder().build().unwrap();
    let data = w.canonical("data");
    let root = policy.matter().to_owned();
    let mut ledger = Ledger::open(ledger_path(&data), &trusted(), &reserved()).unwrap();
    let modify = grant(&root, USER_SID, Rights::Modify, Inherit::Tree);
    let mut plan = AdmittedPlan::from_grants(matter_one(), vec![modify]);
    let (mut acl, mut around) = (NtfsAcl::new().unwrap(), Aside { data: data.clone() });
    before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();
    apply(&mut ledger, &mut plan, &mut acl, &mut around).unwrap();

    for i in 0..65 {
        let child = w.dir.file(&format!("matter/d{i}.txt"), "shut out");
        protect_with(&child, SHUT).unwrap();
    }
    w.dir.file("matter/.git/hooks/pre-commit", "exit 0\r\n");
    w.dir.file("matter/sub/.git/hooks/pre-commit", "exit 0\r\n");
    let refused = repair_after_session(&ledger, matter_one(), &mut acl, &mut around).unwrap_err();
    assert!(
        matches!(
            refused,
            Error::AclApply {
                failure: ApplyFailure::ChildShut,
                ..
            }
        ),
        "{refused:?}"
    );
    assert_eq!(list_set_aside(&data, &matter_one()).unwrap().len(), 64);

    let checked = after_session(&policy, &data, &matter_one(), "s1", exit()).unwrap();
    let from: Vec<_> = checked.moved.iter().map(|e| e.from.clone()).collect();
    assert_eq!(from, [root.join(".git"), root.join("sub").join(".git")]);
    assert!(!checked.partial());
    assert!(!root.join(".git").exists() && !root.join("sub/.git").exists());
}
