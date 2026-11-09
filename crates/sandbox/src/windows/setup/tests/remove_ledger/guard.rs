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
        g(
            &pool_user,
            Rights::DenyPermissionChange,
            Inherit::FilesBelow,
        ),
        g(&owner_rights, Rights::OwnerModify, Inherit::FilesBelow),
        // Crafted: `OWNER RIGHTS` with another right, or that right for another SID. Neither is
        // trusted, so neither is listed or acted on.
        g(&owner_rights, Rights::Modify, Inherit::Tree),
        g(&pool_user, Rights::OwnerModify, Inherit::FilesBelow),
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

/// #808's M2: an `OWNER RIGHTS` entry IT wrote before ours was listed (`Entry::found`) stays
/// through an administrator's removal too; the pool's entries go.
#[test]
fn as_an_administrator_its_own_owner_rights_entry_stays() {
    let (mut machine, mut acl) = pool("remove-ledger-guard-found", None);
    let data_dir = machine.m.data_dir();
    let m1 = matters(&machine).join("m1");
    let owner_rights = Sid::parse(OWNER_RIGHTS_SID).unwrap();
    let g = |sid: &Sid, rights, inherit| grant(&m1, sid, rights, inherit, PathKind::Matter);
    let grants = vec![
        g(&pool_sid(1001), Rights::Modify, Inherit::Tree),
        g(&owner_rights, Rights::OwnerModify, Inherit::FilesBelow),
    ];
    let mut ledger = Ledger::open(ledger_path(&data_dir), &named(), &Reserved::new([])).unwrap();
    let mut plan = AdmittedPlan::from_grants(matter_one(), grants);
    ledger.record_pending(&mut plan).unwrap();
    ledger.set_found(&plan, plan.entries()[1], true).unwrap();
    machine.admin_owned = vec![(m1.clone(), owner())];

    let removed = remove(&mut machine, &mut acl, true).unwrap();
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    let work = LedgerWork::AsAdmin {
        removed: 1,
        left: 1,
    };
    assert_eq!(report.ledger, work);
    let lines = machine.lines();
    let removes: Vec<&String> = lines
        .iter()
        .filter(|l| l.starts_with("remove matters/m1"))
        .collect();
    assert_eq!(
        removes,
        ["remove matters/m1 1001"],
        "IT's entry is never removed"
    );
}
