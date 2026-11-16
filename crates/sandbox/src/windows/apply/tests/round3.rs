//! The S15 `PR3b` round 3 review's R1 across the apply layer and the set-aside: the check when the
//! agent exits doesn't depend on the repair after the session.

use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::windows::set_aside::tests::{exit_on_test_volume, launch_on_test_volume};
use crate::windows::write::rename_new;

/// Makes `file` unreadable (`chmod 000`), as a child the lawyer can't open; the fake's `shut`
/// list is what the repair's walk goes by. Nothing on Windows, where the fake alone stands in.
fn unreadable(file: &Path) {
    #[cfg(unix)]
    crate::test_support::set_mode(file, 0o000);
    #[cfg(not(unix))]
    let _ = file;
}

/// R1: the agent makes 65 children the lawyer can't open, swaps the lawyer's root `.git` for
/// its own and plants `sub\.git`. `repair_after_session` sets 64 aside, then refuses at the
/// cap. `after_session` still sets aside both `.git`s: its own scan found them.
#[test]
fn a_repair_refused_at_the_set_aside_cap_still_leaves_both_gits_to_the_exit_check() {
    let w = world("r3-r1-cap", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let matter = w.canonical("matter");
    let data = w.canonical("data");
    let at = |secs: u64| UNIX_EPOCH + Duration::from_secs(1_762_337_700 + secs);
    let mut plan = launch_plan(&w, &["sib1"]);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    launch_on_test_volume(&matter, &data, "s1", at(0)).unwrap();

    let shut: Vec<PathBuf> = (0..65)
        .map(|i| {
            unreadable(&w.dir.file(&format!("matter/d{i}"), "shut out"));
            matter.join(format!("d{i}"))
        })
        .collect();
    rename_new(&matter.join(".git"), &matter.join("lawyer-git")).unwrap();
    w.dir.file("matter/.git/hooks/post-checkout", "#!/bin/sh\n");
    w.dir
        .file("matter/sub/.git/hooks/post-checkout", "#!/bin/sh\n");
    acl.shut.clone_from(&shut);
    let mut around = around(&w);
    let repaired = repair_after_session(&ledger, matter_one(), &mut acl, &mut around);
    match repaired {
        Err(Error::AclApply {
            failure: ApplyFailure::ChildShut,
            at: Some(child),
            ..
        }) => assert_eq!(child, shut[64]),
        other => panic!("{other:?}"),
    }
    assert_eq!(around.set_aside.len(), 64);

    let checked = exit_on_test_volume(&matter, &data, "s1", at(3600)).unwrap();
    let from: Vec<_> = checked.moved.iter().map(|e| e.from.clone()).collect();
    assert_eq!(from, [matter.join(".git"), matter.join("sub").join(".git")]);
    assert!(!checked.partial());
    assert!(!matter.join(".git").exists() && !matter.join("sub/.git").exists());
    assert!(matter.join("lawyer-git/HEAD").is_file());
}
