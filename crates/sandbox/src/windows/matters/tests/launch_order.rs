//! The VM harness's launch order (`examples/s15_reclaim_vm`, S15 `PR4i`), dry-run on the tests'
//! volume: `before_launch` marks the session open, then `assign` picks a pool user, then the
//! session ends with `after_session`; a forget's `release` then finds no session open, and a
//! later launch keeps the user. Without `before_launch` first, `assign` refuses a new pick: the
//! VM run that stopped at set-up.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::pool::{ledger_in, pool, reread, sids_in, user};
use super::*;
use crate::windows::set_aside::tests::{exit_matter_on_test_volume, launch_matter_on_test_volume};

/// The harness's clock: one tick per pass.
fn tick(n: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_800_000_000 + n)
}

#[test]
fn the_harness_order_assigns_each_matter_and_lets_a_forget_release_it() {
    let data = TempDir::new("launch-order-data");
    let roots = TempDir::new("launch-order-matters");
    let records = TempDir::new("launch-order-ledger");
    let ledger = ledger_in(records.path(), &pool(4));
    let mut sids = sids_in(&data);
    let matters: Vec<(MatterId, std::path::PathBuf)> = ["one", "two", "three", "four"]
        .iter()
        .map(|name| (matter(), roots.dir(&format!("m/{name}"))))
        .collect();
    let (one, _) = &matters[0];

    // What the VM run did: assign before any session is marked.
    let open = sids.open_sessions().unwrap();
    let refused = sids.assign(one, "vmr-none", &pool(4), reread(&ledger), &open);
    assert!(matches!(refused, Err(Assign::Records(_))), "{refused:?}");

    // The harness's order: one, three, four, then two.
    let mut picked = Vec::new();
    for (n, i) in [0, 2, 3, 1].into_iter().enumerate() {
        let (matter, root) = &matters[i];
        let session = format!("vmr-{n}");
        let at = u64::try_from(2 * n).unwrap();
        launch_matter_on_test_volume(root, data.path(), matter, &session, tick(at)).unwrap();
        let open = sids.open_sessions().unwrap();
        let got = sids
            .assign(matter, &session, &pool(4), reread(&ledger), &open)
            .unwrap();
        exit_matter_on_test_volume(root, data.path(), matter, &session, tick(at + 1)).unwrap();
        picked.push(got);
    }
    picked.sort();
    assert_eq!(
        picked,
        (1..=4).map(user).collect::<Vec<_>>(),
        "four distinct users"
    );
    assert!(
        sids.open_sessions().unwrap().is_empty(),
        "every session ended"
    );

    // A forget's release finds no session open; a later launch of a held matter keeps its user.
    assert!(
        sids.release(one, tick(100)).unwrap(),
        "released: no session open"
    );
    let (three, root_three) = &matters[2];
    let held = sids.pool_user(three).cloned();
    launch_matter_on_test_volume(root_three, data.path(), three, "vmr-again", tick(101)).unwrap();
    let open = sids.open_sessions().unwrap();
    let again = sids
        .assign(three, "vmr-again", &pool(4), reread(&ledger), &open)
        .unwrap();
    assert_eq!(Some(again), held, "kept");
    exit_matter_on_test_volume(root_three, data.path(), three, "vmr-again", tick(102)).unwrap();
}
