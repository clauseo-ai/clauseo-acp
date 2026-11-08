//! `--remove` as an administrator takes the guard on what the pool writes off a matter folder
//! too (`Policy::native_code_deny`): `OWNER RIGHTS`' entry is trusted by its rights alone.

use super::*;
use crate::windows::shared_writes::OWNER_RIGHTS_SID;

#[test]
fn as_an_administrator_the_guard_comes_off_with_the_pools_entries() {
    let (mut machine, mut acl) = pool("remove-ledger-guard", None);
    let data_dir = machine.m.data_dir();
    let m1 = matters(&machine).join("m1");
    let owner_rights = Sid::parse(OWNER_RIGHTS_SID).unwrap();
    let pool_user = pool_sid(1001);
    let g = |sid: &Sid, rights, inherit| grant(&m1, sid, rights, inherit, PathKind::Matter);
    let grants = vec![
        g(&pool_user, Rights::Modify, Inherit::Tree),
        g(&pool_user, Rights::DenyExecute, Inherit::FilesBelow),
        g(&pool_user, Rights::DenyPermissionChange, Inherit::Below),
        g(&owner_rights, Rights::OwnerModify, Inherit::Below),
        // Crafted: `OWNER RIGHTS` with another right, or that right for another SID. Neither is
        // trusted, so neither is listed or acted on.
        g(&owner_rights, Rights::Modify, Inherit::Tree),
        g(&pool_user, Rights::OwnerModify, Inherit::Below),
    ];
    record(&data_dir, matter_one(), grants, None);
    machine.admin_owned = vec![(m1.clone(), owner())];

    let removed = remove(&mut machine, &mut acl, true).unwrap();
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    let all = LedgerWork::AsAdmin {
        removed: 4,
        left: 0,
    };
    assert_eq!(report.ledger, all);
    let lines = machine.lines();
    let removes = lines.iter().filter(|l| l.starts_with("remove matters/m1"));
    assert_eq!(removes.count(), 4, "{lines:?}");
}
