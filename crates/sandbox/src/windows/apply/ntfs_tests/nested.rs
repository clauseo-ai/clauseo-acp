//! S15 `PR4a2` on NTFS: a nested `CLAUDE.md` and a nested `.git\config` that the lawyer had at
//! launch can't be edited, renamed, replaced or deleted by the pool user, and forgetting the
//! matter hands their denies back. This test process stands in for the pool user: the plan
//! names this user's own SID as the pool user, and the matter's DACL gives that SID `Modify`
//! alone (no `FILE_DELETE_CHILD`), as tier B gives the pool user. The lawyer's copies are read
//! back byte for byte.

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use super::Around;
use crate::windows::acl_plan::{AdmittedPlan, Sid, Subjects, plan};
use crate::windows::admit::{PathKind, admit_with};
use crate::windows::apply::{apply, forget_matter};
use crate::windows::fence::Neighbours;
use crate::windows::ledger::{Ledger, ledger_path};
use crate::windows::record::current_user_sid;
use crate::windows::test_acl::{entries, has};
use crate::windows::test_dacl::{drop_privileges, protect_with};
use crate::windows::test_world::{
    FakeFolders, GROUP_SID, OTHER_POOL_SIDS, World, matter_one, reserved,
};
use crate::windows::{FolderSupport, NtfsAcl};

const CLAUDE: &str = "# The lawyer's instructions\r\n\r\nCite the bundle.\r\n";
const CONFIG: &str = "[core]\r\n\trepositoryformatversion = 0\r\n\tbare = false\r\n";

/// `ACCESS_DENIED_ACE_TYPE`.
const DENIED: u8 = 1;

/// The read-only denies of matter one's plan for `w`, with `me` as its pool user: what
/// `acl_plan` makes of the policy's `read_only_in_matter`, nothing else.
fn read_only_denies(w: &World, me: &Sid) -> AdmittedPlan {
    let policy = w.builder().build().unwrap();
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let none = FakeFolders::default();
    let admitted = admit_with(&policy, &w.at("data"), &Neighbours::default(), ok, &none).unwrap();
    let subjects = Subjects {
        pool_user: me.clone(),
        matter: None,
        other_pool_users: OTHER_POOL_SIDS
            .iter()
            .map(|s| Sid::parse(s).unwrap())
            .collect(),
        pool_group: Sid::parse(GROUP_SID).unwrap(),
    };
    let full = plan(&admitted, &subjects, matter_one()).unwrap();
    let denies = full
        .grants()
        .iter()
        .filter(|g| g.kind() == PathKind::ReadOnlyInMatter)
        .cloned()
        .collect();
    AdmittedPlan::from_grants(matter_one(), denies)
}

/// One thing the agent tries, through std's own calls, as an agent's tools make them.
#[derive(Debug, Clone, Copy)]
enum Try {
    /// Open the file to write.
    Write,
    /// Open the file to append, and append a line.
    Append,
    /// Rename it to the other name, replacing what is there.
    Rename,
    /// Delete the file.
    Delete,
    /// Delete the folder and everything in it.
    DeleteTree,
    /// Make a file of the agent's.
    Make,
}

/// `what` on `path`; `to` is the name a rename gives it.
#[expect(
    clippy::disallowed_methods,
    reason = "the test plays the agent: it tries std's own writes, renames and deletes"
)]
fn agent(what: Try, path: &Path, to: &Path) -> io::Result<()> {
    match what {
        Try::Write => OpenOptions::new().write(true).open(path).map(drop),
        Try::Append => OpenOptions::new()
            .append(true)
            .open(path)?
            .write_all(b"a line\r\n"),
        Try::Rename => fs::rename(path, to),
        Try::Delete => fs::remove_file(path),
        Try::DeleteTree => fs::remove_dir_all(path),
        Try::Make => fs::write(path, "# the agent's\r\n"),
    }
}

/// `what` on `path` (to `to`), which must be refused for access.
fn denied(what: Try, path: &Path, to: &Path) {
    match agent(what, path, to) {
        Ok(()) => panic!("{what:?} {} went through", path.display()),
        Err(error) => assert_eq!(
            error.kind(),
            io::ErrorKind::PermissionDenied,
            "{what:?} {}: {error}",
            path.display()
        ),
    }
}

/// Each way the agent could change the file at `path`, every one refused: open it to write or
/// to append, rename it away, delete it, and rename `spare` (a file of the agent's at the
/// matter's root) over it.
fn refused(path: &Path, spare: &Path) {
    let away = path.with_extension("moved");
    for what in [Try::Write, Try::Append, Try::Rename, Try::Delete] {
        denied(what, path, &away);
    }
    agent(Try::Make, spare, spare).unwrap();
    denied(Try::Rename, spare, path);
    agent(Try::Delete, spare, spare).unwrap();
}

/// Whether `path` holds a deny entry for `sid`, inherited or explicit.
fn denied_to(path: &Path, sid: &Sid) -> bool {
    entries(path)
        .iter()
        .any(|(kind, _, _, s)| *kind == DENIED && s == sid)
}

#[test]
fn the_pool_user_cant_edit_rename_or_delete_a_nested_claude_md_or_git_config() {
    drop_privileges();
    let w = World::new("apply-ntfs-nested");
    w.dir.file("matter/sub/CLAUDE.md", CLAUDE);
    w.dir.file("matter/repo/.git/config", CONFIG);
    w.dir
        .file("matter/sub/brief.md", "# the lawyer's brief\r\n");
    let me = current_user_sid().unwrap();
    let matter = w.canonical("matter");
    // The pool user's share of the matter: `Modify` and no `FILE_DELETE_CHILD`. `SYSTEM` keeps
    // full control; this user, the owner, keeps `READ_CONTROL` and `WRITE_DAC`.
    let dacl = format!("D:P(A;OICI;0x1301bf;;;{me})(A;OICI;FA;;;SY)");
    protect_with(&matter, &dacl).unwrap();
    let claude = matter.join("sub").join("CLAUDE.md");
    let git = matter.join("repo").join(".git");
    let config = git.join("config");

    let mut plan = read_only_denies(&w, &me);
    let planned: Vec<PathBuf> = plan.grants().iter().map(|g| g.path().to_owned()).collect();
    assert!(planned.contains(&claude), "{planned:?}");
    assert!(planned.contains(&git), "{planned:?}");
    let data = w.canonical("data");
    let mut ledger =
        Ledger::open(ledger_path(&data), std::slice::from_ref(&me), &reserved()).unwrap();
    let mut acl = NtfsAcl::new().unwrap();
    apply(&mut ledger, &mut plan, &mut acl, &mut Around).unwrap();
    let recorded: Vec<PathBuf> = ledger.entries().map(|e| e.path.clone()).collect();
    assert!(recorded.contains(&claude), "{recorded:?}");
    assert!(recorded.contains(&git), "{recorded:?}");
    assert!(has(&claude, me.as_str(), false) && has(&git, me.as_str(), false));
    assert!(
        denied_to(&config, &me),
        "the deny on .git reaches its config"
    );

    let spare = matter.join("agent.tmp");
    for (path, was) in [(&claude, CLAUDE), (&config, CONFIG)] {
        refused(path, &spare);
        let now = fs::read(path).unwrap();
        assert_eq!(now, was.as_bytes(), "{}", path.display());
    }
    // The nested `.git` folder itself can't be renamed or deleted either.
    let moved = matter.join("repo").join("git-moved");
    denied(Try::Rename, &git, &moved);
    denied(Try::DeleteTree, &git, &moved);
    assert_eq!(fs::read(&config).unwrap(), CONFIG.as_bytes());
    // The rest of the matter stays the pool user's to write.
    let brief = matter.join("sub").join("brief.md");
    agent(Try::Append, &brief, &brief).unwrap();

    // Forget hands every deny back: the entries go, and the files can be written again.
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut Around).unwrap();
    assert_eq!(ledger.entries().count(), 0);
    for path in [&claude, &git, &config] {
        assert!(!denied_to(path, &me), "{}", path.display());
    }
    agent(Try::Append, &claude, &claude).unwrap();
}
