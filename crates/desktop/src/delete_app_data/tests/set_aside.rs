//! The set-aside with `--sandbox-removed`: Clauseo's own bookkeeping alone (`record.json` and
//! `set-aside.lock` in each matter's folder, which every tier-B launch leaves) goes with the
//! rest, with no Kept box; anything else there keeps the whole set-aside, untouched, and no link
//! is followed.

use super::{DELETED, KEPT_SET_ASIDE, World, exists, make};

/// Two matters' folders in the set-aside, each holding only the bookkeeping, and the ledger.
const MATTERS: [&str; 2] = [
    "sandbox/set-aside/01890a5d-ac96-774b-bcce-b302099a8057",
    "sandbox/set-aside/01890a5d-ac96-774b-8cce-b302099a8058",
];

fn bookkeeping_world(test: &str) -> World {
    let world = World::new(test);
    for matter in MATTERS {
        world.data_dir(matter);
        world.data_file(&format!("{matter}/record.json"));
        world.data_file(&format!("{matter}/set-aside.lock"));
    }
    world.data_file("sandbox/acl-ledger.json");
    world.data_file("store.sqlite");
    world
}

/// The set-aside is kept, with no failure; every bookkeeping file is still there, the rest went.
fn assert_kept_whole(world: &World) {
    assert_eq!(world.delete(true), (KEPT_SET_ASIDE, String::new()));
    for matter in MATTERS {
        assert!(exists(&world.data(&format!("{matter}/set-aside.lock"))));
    }
    assert!(!exists(&world.data("sandbox/acl-ledger.json")));
    assert!(!exists(&world.data("store.sqlite")));
    assert!(!exists(&world.roaming_root()));
    world.assert_canary_whole();
}

#[test]
fn a_set_aside_holding_only_bookkeeping_goes_and_nothing_is_kept() {
    let world = bookkeeping_world("dad-set-aside-bookkeeping");
    assert_eq!(world.delete(true), (DELETED, String::new()));
    world.assert_both_gone();
    world.assert_canary_whole();
}

#[test]
fn bookkeeping_beside_a_pass_folder_holding_a_file_keeps_the_whole_set_aside() {
    let world = bookkeeping_world("dad-set-aside-pass");
    let pass = format!("{}/20261107T101500-000Z/.git", MATTERS[1]);
    world.data_dir(&pass);
    world.data_file(&format!("{pass}/HEAD"));
    assert_kept_whole(&world);
    assert!(exists(&world.data(&format!("{pass}/HEAD"))));
    assert!(exists(&world.data(&format!("{}/record.json", MATTERS[0]))));
}

#[test]
fn any_other_name_keeps_the_whole_set_aside() {
    // The manifest of a pass whose items were restored, a name differing only in case, any
    // other file, and a file directly in the set-aside: none is the bookkeeping.
    let names = [
        format!("{}/manifest.json", MATTERS[0]),
        format!("{}/Record.json", MATTERS[1]),
        format!("{}/notes.txt", MATTERS[0]),
        "sandbox/set-aside/record.json".to_owned(),
    ];
    for (n, name) in names.iter().enumerate() {
        let world = bookkeeping_world(&format!("dad-set-aside-name-{n}"));
        if name.ends_with("Record.json") && exists(&world.data(name)) {
            // A file system that ignores case already has it as `record.json`.
            continue;
        }
        world.data_file(name);
        assert_kept_whole(&world);
        assert!(exists(&world.data(name)), "{name}");
    }
}

#[test]
fn a_record_that_is_a_folder_keeps_the_whole_set_aside() {
    let world = bookkeeping_world("dad-set-aside-record-folder");
    let record = format!("{}/record.json", MATTERS[0]);
    make::remove_file(&world.data(&record));
    world.data_dir(&record);
    world.data_file(&format!("{record}/inside.txt"));
    assert_kept_whole(&world);
    assert!(exists(&world.data(&format!("{record}/inside.txt"))));
}

#[test]
fn a_record_or_a_matter_folder_that_is_a_link_keeps_the_whole_set_aside_and_is_not_followed() {
    // A folder link (a junction on Windows) as a matter's `record.json`.
    let world = bookkeeping_world("dad-set-aside-record-junction");
    let record = world.data(&format!("{}/record.json", MATTERS[0]));
    make::remove_file(&record);
    make::folder_link(&world.canary(), &record);
    assert_kept_whole(&world);
    assert!(exists(&record));

    // A file symbolic link as a matter's `set-aside.lock`, to the lawyer's file.
    let world = bookkeeping_world("dad-set-aside-lock-link");
    let lock = world.data(&format!("{}/set-aside.lock", MATTERS[1]));
    make::remove_file(&lock);
    if make::made_or_unprivileged(make::symlink_file(
        &world.canary().join("secret.txt"),
        &lock,
    )) {
        assert_kept_whole(&world);
        assert!(exists(&lock));
    }

    // A matter's folder that is itself a link to a folder holding only a `record.json`.
    let world = bookkeeping_world("dad-set-aside-matter-link");
    world.scratch.file("outside/canary/record.json", b"{}");
    let linked = world.data("sandbox/set-aside/01890a5d-ac96-774b-8cce-b302099a8059");
    make::folder_link(&world.canary(), &linked);
    assert_eq!(world.delete(true), (KEPT_SET_ASIDE, String::new()));
    assert!(exists(&linked));
    assert!(exists(&world.at("outside/canary/record.json")));
    world.assert_canary_whole();
}
