//! The record's crash-safe remake: a crash after each step (the copy written, the old record
//! deleted, before the rename) recovers on the next walk to a record with its values; a copy
//! that fails its check is deleted, never adopted or kept.

use std::collections::BTreeMap;

use super::super::Value;
use super::super::walk::parses;
use super::{ADMIN_KEY, FakeTree, USER_KEY, record_pinned, under};
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::vendor_key::{VENDOR_SDDL, ensure};

/// A `REG_SZ`'s data.
fn sz(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// A record's values as set-up writes them.
fn record() -> Vec<Value> {
    let value = |name: &str, kind: u32, data: Vec<u8>| Value {
        name: name.to_owned(),
        kind,
        data,
    };
    let mut pool = sz("S-1-5-21-1-2-3-1101");
    pool.extend([0, 0]);
    vec![
        value("OwnerSid", 1, sz("S-1-5-21-1-2-3-1001")),
        value("GroupSid", 1, sz("S-1-5-21-1-2-3-1100")),
        value("DataDir", 1, sz("C:\\Users\\a\\Clauseo")),
        value("Removing", 4, 0u32.to_le_bytes().to_vec()),
        value("PoolSids", 7, pool),
    ]
}

/// `values` as the fake keeps them.
fn kept(values: &[Value]) -> BTreeMap<String, (u32, Vec<u8>)> {
    values
        .iter()
        .map(|value| (value.name.clone(), (value.kind, value.data.clone())))
        .collect()
}

/// Puts `values` on the fake key `path`.
fn put(tree: &mut FakeTree, path: &str, values: &[Value]) {
    let node = tree.keys.get_mut(path).unwrap();
    node.values = values.iter().map(|value| value.name.clone()).collect();
    node.data = kept(values);
}

/// The record under the root holds `values` with its pinned SD, and no copy is left.
fn assert_whole(tree: &FakeTree, values: &[Value]) {
    let record = &tree.keys[&under("Sandbox")];
    assert_eq!(record.data, kept(values));
    assert_eq!(record.sddl, record_pinned());
    assert!(!tree.has(&under("Sandbox.new")));
}

/// A trusted record with values, and one crash injected by `crash`.
fn crashed(crash: impl FnOnce(&mut FakeTree)) -> FakeTree {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Sandbox", &record_pinned(), false)]);
    put(&mut tree, &under("Sandbox"), &record());
    crash(&mut tree);
    assert!(
        ensure(&mut tree, VENDOR_KEY).is_err(),
        "the crash stops the walk"
    );
    tree.fail_flush = false;
    tree.fail_delete = None;
    tree.fail_rename = false;
    tree
}

#[test]
fn a_walk_writes_the_copy_flushes_it_deletes_the_old_record_and_renames_the_copy() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Sandbox", &record_pinned(), false)]);
    put(&mut tree, &under("Sandbox"), &record());
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.recreated, [under("Sandbox")]);
    assert_eq!(tree.created, [under("Sandbox.new")]);
    assert_eq!(tree.deleted, [under("Sandbox")]);
    assert_eq!(tree.renamed, [(under("Sandbox.new"), under("Sandbox"))]);
    assert_whole(&tree, &record());
}

#[test]
fn a_crash_after_the_copy_is_written_keeps_the_old_record_and_the_next_walk_redoes_it() {
    for crash in [
        (|tree: &mut FakeTree| tree.fail_flush = true) as fn(&mut FakeTree),
        |tree: &mut FakeTree| tree.fail_delete = Some(under("Sandbox")),
    ] {
        let mut tree = crashed(crash);
        assert!(tree.has(&under("Sandbox")) && tree.has(&under("Sandbox.new")));
        let report = ensure(&mut tree, VENDOR_KEY).unwrap();
        assert_eq!(report.dropped_copies, [under("Sandbox.new")]);
        assert_eq!(report.recreated, [under("Sandbox")]);
        assert_eq!(report.recovered, Vec::<String>::new());
        assert_whole(&tree, &record());
    }
}

#[test]
fn a_crash_after_the_old_record_is_deleted_renames_the_copy_into_place_with_its_values() {
    let mut tree = crashed(|tree| tree.fail_rename = true);
    assert!(!tree.has(&under("Sandbox")) && tree.has(&under("Sandbox.new")));
    assert_eq!(tree.sddl_of(&under("Sandbox.new")), record_pinned());
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.recovered, [under("Sandbox.new")]);
    // Then visited as the record, and made again as every walk does.
    assert_eq!(report.recreated, [under("Sandbox")]);
    assert!(report.dropped_copies.is_empty() && report.planted.is_empty());
    assert_whole(&tree, &record());
}

#[test]
fn a_planted_copy_with_a_weak_sd_is_deleted_not_adopted() {
    let user_pinned = record_pinned().replacen("O:BA", "O:S-1-5-21-1-2-3-1001", 1);
    let users_write = "O:BAD:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KA;;;BU)";
    for sddl in [
        USER_KEY,
        ADMIN_KEY,
        user_pinned.as_str(),
        users_write,
        "O:BAD:NO_ACCESS_CONTROL",
    ] {
        let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Sandbox.new", sddl, false)]);
        put(&mut tree, &under("Sandbox.new"), &record());
        let report = ensure(&mut tree, VENDOR_KEY).unwrap();
        assert_eq!(report.dropped_copies, [under("Sandbox.new")], "{sddl}");
        assert!(report.recovered.is_empty(), "{sddl}");
        assert!(!tree.has(&under("Sandbox.new")), "{sddl}");
        assert!(!tree.has(&under("Sandbox")), "{sddl}: nothing is adopted");
        assert!(!report.taken_over.contains(&under("Sandbox.new")), "{sddl}");
    }
}

#[test]
fn a_pinned_copy_with_a_link_value_a_subkey_or_bad_values_is_deleted() {
    let mut bad = record();
    bad[0].data = sz("not a SID");
    let mut wrong_kind = record();
    wrong_kind[3].kind = 1;
    for case in 0..4 {
        let mut tree = FakeTree::with(
            Some(VENDOR_SDDL),
            &[("Sandbox.new", &record_pinned(), false)],
        );
        let values = match case {
            2 => bad.clone(),
            3 => wrong_kind.clone(),
            _ => record(),
        };
        put(&mut tree, &under("Sandbox.new"), &values);
        match case {
            0 => tree.keys.get_mut(&under("Sandbox.new")).unwrap().link_value = true,
            1 => tree.insert(under("Sandbox.new\\Inner"), ADMIN_KEY, false),
            _ => {}
        }
        let report = ensure(&mut tree, VENDOR_KEY).unwrap();
        assert_eq!(report.dropped_copies, [under("Sandbox.new")], "case {case}");
        assert!(!tree.has(&under("Sandbox.new")), "case {case}");
        assert!(!tree.has(&under("Sandbox")), "case {case}");
    }
}

#[test]
fn a_copy_that_is_a_link_is_deleted_itself_never_renamed() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[("Sandbox.new", &record_pinned(), true)],
    );
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.dropped_copies, [under("Sandbox.new")]);
    assert_eq!(tree.renamed, Vec::new());
    assert_eq!(tree.deleted, [under("Sandbox.new")]);
}

#[test]
fn values_parse_as_a_record_when_each_has_its_type_and_decodes() {
    assert!(parses(&record()));
    assert!(parses(&[]), "an empty record");
    let mut missing = record();
    missing.remove(1);
    assert!(!parses(&missing), "no GroupSid");
    let mut pool = record();
    pool[4].kind = 1;
    assert!(!parses(&pool), "PoolSids as a REG_SZ");
}
