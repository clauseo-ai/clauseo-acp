//! The registered matters the setup exe finds in the owner's files (`registered.rs`), against a
//! ledger the fake's extend wrote.

use clauseo_protocol::MatterId;

use super::*;
use crate::windows::matters::{MatterRow, Rows};
use crate::windows::setup::registered::registered;

fn held(pool_user: u32) -> MatterRow {
    MatterRow {
        pool_user: Some(fake::sid(pool_user)),
        ..MatterRow::default()
    }
}

#[test]
fn each_matter_holding_a_pool_user_comes_back_with_the_folders_its_ledger_names() {
    let mut machine = FakeMachine::new("setup-registered");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let matters = two_matters(&machine);
    let req = request(&mut machine, 4, matters.clone());
    set_up(&mut machine, &mut acl, &req).unwrap();
    let record = machine.state.record.clone().unwrap();
    let mut trusted = record.pool.clone();
    trusted.push(record.group);
    let ledger = machine.ledger(&trusted).unwrap();

    let never_launched: MatterId = "01890a5d-ac96-774b-8cce-b302099a8059".parse().unwrap();
    let released: MatterId = "01890a5d-ac96-774b-8cce-b302099a805a".parse().unwrap();
    let mut rows = Rows::new();
    rows.insert(matters[0].id, held(1001));
    rows.insert(matters[1].id, held(1002));
    rows.insert(never_launched, held(1003));
    rows.insert(
        released,
        MatterRow {
            released_at: Some(1),
            ..held(1004)
        },
    );

    let found = registered(&rows, &ledger);
    assert_eq!(found.len(), 2, "{found:?}");
    for found in &found {
        let given = matters.iter().find(|m| m.id == found.id).unwrap();
        assert_eq!(found.pool_user, given.pool_user);
        assert!(
            !found.fenced,
            "planned again; entries already listed are kept"
        );
        let canonical = |path: &std::path::Path| dunce::canonicalize(path).unwrap();
        assert_eq!(found.folders.root, canonical(&given.folders.root));
        assert_eq!(found.folders.state, canonical(&given.folders.state));
        assert_eq!(found.folders.notes, None);
    }
    // Nothing held, nothing found.
    assert_eq!(registered(&Rows::new(), &ledger), Vec::new());
}
