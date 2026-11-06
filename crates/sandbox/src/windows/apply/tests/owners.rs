//! S15 `PR4a2`: a protected item a pool user owns goes to the lawyer before its deny is
//! written, since an owner gets `WRITE_DAC` before any deny is read; so does what a pool user
//! owns inside a protected folder newly denied. The agent's other files stay its own.

use super::fake::LAWYER;
use super::*;

#[test]
fn a_pool_owned_protected_item_is_handed_to_the_lawyer_before_its_deny() {
    let w = world("apply-owners", &[]);
    let claude = w
        .dir
        .file("matter/sub/CLAUDE.md", "# restored from the set-aside\n");
    let settings = w.dir.file("matter/a/.claude/settings.json", "{}");
    let notes = w.dir.file("matter/sub/notes.md", "the agent's\n");
    let canon = |p: &Path| dunce::canonicalize(p).unwrap();
    let (claude, settings, notes) = (canon(&claude), canon(&settings), canon(&notes));
    let folder = settings.parent().unwrap().to_owned();
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    for path in [&claude, &folder, &settings, &notes] {
        acl.owners.insert(path.clone(), USER_SID.to_owned());
    }
    let mut plan = launch_plan(&w, &[]);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();

    for path in [&claude, &folder, &settings] {
        assert_eq!(acl.owners[path], LAWYER, "{}", path.display());
    }
    assert_eq!(
        acl.owners[&notes], USER_SID,
        "the agent's own file stays its own"
    );
    for path in [&claude, &folder] {
        assert!(
            has(&acl, path, USER_SID, Rights::DenyWrite),
            "{}",
            path.display()
        );
    }
    assert!(
        acl.reclaimed
            .iter()
            .any(|(root, pool)| *root == folder && pool.contains(&sid(USER_SID))),
        "{:?}",
        acl.reclaimed
    );
}
