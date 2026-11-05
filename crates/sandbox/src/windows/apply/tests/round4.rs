//! The S15 `PR3b` round 4 fixes in the apply layer, from the second VM run.

use super::*;

/// Round 4, rows 4b and 16 of the VM run: the walk reports a child the lawyer can't open by its
/// verbatim final path (`\\?\C:\…`), the launch's roots are plain, so `clearing` found it
/// outside every root and refused `ChildShut` instead of setting it aside. Both sides are now
/// compared in one form (verbatim since round 5), and the child is set aside as reported.
#[test]
fn a_shut_child_reported_by_its_verbatim_path_is_set_aside_at_launch() {
    let w = world("r4-verbatim-launch", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let matter = w.canonical("matter");
    acl.prefix = r"\\?\";
    acl.shut = vec![matter.join("shut.txt")];
    let mut plan = launch_plan(&w, &["sib1"]);
    let mut around = around(&w);
    apply(&mut ledger, &mut plan, &mut acl, &mut around).unwrap();
    assert_eq!(around.set_aside, vec![reported(&matter.join("shut.txt"))]);
}

/// Round 4, row 16: the same for forget's hand-back walk.
#[test]
fn a_shut_child_reported_by_its_verbatim_path_is_set_aside_by_forget() {
    let w = world("r4-verbatim-forget", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut plan = launch_plan(&w, &["sib1"]);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    let matter = w.canonical("matter");
    acl.prefix = r"\\?\";
    acl.shut = vec![matter.join("x.txt")];
    let mut around = around(&w);
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around).unwrap();
    assert_eq!(around.set_aside, vec![reported(&matter.join("x.txt"))]);
}

/// `path` as the fake walk reports it with the `\\?\` prefix.
fn reported(path: &Path) -> PathBuf {
    PathBuf::from(format!(r"\\?\{}", path.display()))
}

/// Round 4: a verbatim path compares with its plain form, on either side.
#[test]
fn a_verbatim_path_compares_with_its_plain_form() {
    use crate::windows::acl_plan::in_folder;
    let matter = PathBuf::from("/m/matter");
    let verbatim = PathBuf::from(r"\\?\/m/matter/x.txt");
    assert!(in_folder(&verbatim, &matter));
    assert!(in_folder(&matter, &PathBuf::from(r"\\?\/m/matter")));
}
