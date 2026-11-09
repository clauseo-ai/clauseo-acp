//! `record_values.rs`: the values a record is written as, read back, and the names the
//! installer and the tasks read.

use super::*;

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

fn record() -> MachineRecord {
    MachineRecord {
        owner: sid("S-1-5-21-7-8-9-1001"),
        group: sid("S-1-5-21-7-8-9-1009"),
        pool: vec![sid("S-1-5-21-7-8-9-1010"), sid("S-1-5-21-7-8-9-1011")],
        data_dir: PathBuf::from(r"C:\Users\Rohan\AppData\Local\Clauseo"),
        removing: false,
    }
}

/// The values `encode` gives, read back the way `record_write::read` gathers them.
fn raw_of(values: &[(&'static str, RegValue)]) -> RawRecord {
    let text =
        |units: &[u16]| String::from_utf16(units.split(|unit| *unit == 0).next().unwrap()).unwrap();
    let mut raw = RawRecord::default();
    for (name, value) in values {
        match (*name, value) {
            (OWNER_SID, RegValue::Sz(units)) => raw.owner = Some(text(units)),
            (GROUP_SID, RegValue::Sz(units)) => raw.group = Some(text(units)),
            (DATA_DIR, RegValue::Sz(units)) => raw.data_dir = Some(text(units)),
            (POOL_SIDS, RegValue::MultiSz(units)) => raw.pool = Some(units.clone()),
            (REMOVING, RegValue::Dword(number)) => raw.removing = Some(*number),
            other => panic!("unexpected value {other:?}"),
        }
    }
    raw
}

#[test]
fn the_values_are_the_five_the_readers_read_owner_last() {
    let values = encode(&record()).unwrap();
    let names: Vec<&str> = values.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        ["GroupSid", "DataDir", "Removing", "PoolSids", "OwnerSid"]
    );
    assert_eq!(values[4].1, RegValue::Sz(sz("S-1-5-21-7-8-9-1001")));
    assert!(matches!(&values[4].1, RegValue::Sz(units) if units.ends_with(&[0])));
    assert_eq!(values[2].1, RegValue::Dword(0));
    let RegValue::MultiSz(pool) = &values[3].1 else {
        panic!("{:?}", values[3]);
    };
    let expected: Vec<u16> = "S-1-5-21-7-8-9-1010\0S-1-5-21-7-8-9-1011\0\0"
        .encode_utf16()
        .collect();
    assert_eq!(pool, &expected);
}

#[test]
fn a_record_round_trips_through_its_values() {
    let mut removing = record();
    removing.removing = true;
    let mut empty = record();
    empty.pool.clear();
    for each in [record(), removing, empty] {
        let raw = raw_of(&encode(&each).unwrap());
        assert_eq!(decode(raw), Ok(Some(each)));
    }
}

#[test]
fn an_empty_pool_is_one_nul_and_reads_back_empty() {
    let mut empty = record();
    empty.pool.clear();
    let values = encode(&empty).unwrap();
    assert_eq!(values[3].1, RegValue::MultiSz(vec![0]));
    assert_eq!(pool_sids_from_multi_sz(&[0]), Ok(vec![]));
}

#[test]
fn the_owner_is_never_written_as_a_pool_user() {
    let mut bad = record();
    bad.pool.push(bad.owner.clone());
    assert_eq!(encode(&bad), Err(RecordValueError::OwnerInPool));
}

#[test]
fn bytes_are_little_endian_units_or_the_dword() {
    assert_eq!(bytes(&RegValue::Sz(sz("Ab"))), [b'A', 0, b'b', 0, 0, 0]);
    assert_eq!(
        bytes(&RegValue::MultiSz(vec![0x263A, 0])),
        [0x3A, 0x26, 0, 0]
    );
    assert_eq!(bytes(&RegValue::Dword(1)), [1, 0, 0, 0]);
    assert_eq!(
        bytes(&RegValue::Qword(0x0102_0304_0506_0708)),
        [8, 7, 6, 5, 4, 3, 2, 1]
    );
}

#[test]
fn no_value_is_no_record_and_a_partial_one_reads_as_none() {
    assert_eq!(decode(RawRecord::default()), Ok(None));
    assert!(!RawRecord::default().partial());
    let whole = raw_of(&encode(&record()).unwrap());
    assert!(!whole.partial());
    let mut no_owner = whole.clone();
    no_owner.owner = None;
    let mut no_group = whole.clone();
    no_group.group = None;
    let mut no_dir = whole.clone();
    no_dir.data_dir = None;
    // An intent alone: the owner, nothing else.
    let intent = RawRecord {
        owner: whole.owner.clone(),
        ..RawRecord::default()
    };
    for partial in [no_owner, no_group, no_dir, intent] {
        assert!(partial.partial(), "{partial:?}");
        assert_eq!(decode(partial), Ok(None));
    }
    // Every first write torn after each value: partial until the owner's, the last, is there.
    let values = encode(&record()).unwrap();
    for written in 1..values.len() {
        let torn = raw_of(&values[..written]);
        assert!(torn.partial(), "{written}");
    }
    // A malformed value is still refused, with or without the rest.
    let alone = RawRecord {
        owner: Some("BA".to_owned()),
        ..RawRecord::default()
    };
    assert_eq!(
        decode(alone.clone()),
        Err(RecordValueError::Malformed("OwnerSid"))
    );
    assert!(!alone.partial());
    // Missing PoolSids and Removing read as the readers read them.
    let mut bare = whole.clone();
    bare.pool = None;
    bare.removing = None;
    let read = decode(bare).unwrap().unwrap();
    assert_eq!((read.pool.len(), read.removing), (0, false));
    let mut odd = whole.clone();
    odd.removing = Some(7);
    assert!(decode(odd).unwrap().unwrap().removing);
    let mut alias = whole.clone();
    alias.owner = Some("BA".to_owned());
    assert_eq!(decode(alias), Err(RecordValueError::Malformed("OwnerSid")));
    let mut repeated = whole;
    repeated.pool = Some(
        "S-1-5-18\0S-1-5-18\0\0"
            .encode_utf16()
            .collect::<Vec<u16>>(),
    );
    assert_eq!(
        decode(repeated),
        Err(RecordValueError::Malformed("PoolSids"))
    );
}

/// The installer (PR 1) reads these names and forms before it overwrites the admin copy; its
/// own test pins the same strings against this file.
#[test]
fn the_admin_copy_values_keep_their_names_and_forms() {
    assert_eq!(
        ADMIN_VALUES,
        [
            "AdminCopySha256",
            "UserInstallDir",
            "AdminCopyVersion",
            "AdminCopyOwner"
        ]
    );
    assert_eq!(OWNER_SID, "OwnerSid");
    assert_eq!(CopyOwner::Machine.text(), "machine");
    let lawyer = CopyOwner::User(sid("S-1-5-21-1-2-3-1001"));
    assert_eq!(lawyer.text(), "S-1-5-21-1-2-3-1001");
    for owner in [CopyOwner::Machine, lawyer] {
        assert_eq!(CopyOwner::parse(owner.text()), Some(owner));
    }
    for bad in ["Machine", "", "lawyer", "machine "] {
        assert_eq!(CopyOwner::parse(bad), None, "{bad}");
    }
    for name in ADMIN_VALUES {
        assert!(!SETUP_VALUES.contains(&name), "{name}");
    }
    // The stop task's marks go with the set-up's values, so `--remove` deletes them.
    assert_eq!(RESET_SIDS, "ResetSids");
    assert!(SETUP_VALUES.contains(&RESET_SIDS));
    // And the resets a stop owed without the lock.
    assert_eq!(RESET_OWED, "ResetOwed");
    assert!(SETUP_VALUES.contains(&RESET_OWED));
    // And the install's id, so a reinstall's markers can tell the last install's apart.
    assert_eq!(INSTALL_ID, "InstallId");
    assert!(SETUP_VALUES.contains(&INSTALL_ID));
    let id = new_install_id([0xab; 16]);
    assert_eq!(id, "ab".repeat(16));
    assert!(is_install_id(&id));
    for bad in [
        "",
        "AB".repeat(16).as_str(),
        &"ab".repeat(15),
        &"ab".repeat(17),
    ] {
        assert!(!is_install_id(bad), "{bad}");
    }
    // So do its completion's.
    assert_eq!(
        (STOP_COMPLETED, STOP_SEQUENCE, STOP_NEXT),
        ("StopCompleted", "StopSequence", "StopNext")
    );
    for name in [STOP_COMPLETED, STOP_SEQUENCE, STOP_NEXT] {
        assert!(SETUP_VALUES.contains(&name), "{name}");
    }
    // And the sweep's, each sequence deleted before its next.
    assert_eq!(
        (SWEEP_COMPLETED, SWEEP_SEQUENCE, SWEEP_NEXT),
        ("SweepCompleted", "SweepSequence", "SweepNext")
    );
    let at = |name| {
        SETUP_VALUES
            .iter()
            .position(|value| *value == name)
            .unwrap()
    };
    assert!(at(STOP_SEQUENCE) < at(STOP_NEXT));
    assert!(at(SWEEP_COMPLETED) < at(SWEEP_SEQUENCE));
    assert!(at(SWEEP_SEQUENCE) < at(SWEEP_NEXT));
}

#[test]
fn reset_marks_are_added_once_and_taken_off_alone() {
    let [a, b, c] = ["1001", "1002", "1003"].map(|rid| sid(&format!("S-1-5-21-1-2-3-{rid}")));
    let listed = resets_with(&[], &[a.clone(), b.clone()]);
    assert_eq!(listed, [a.clone(), b.clone()]);
    let listed = resets_with(&listed, &[b.clone(), c.clone(), c.clone()]);
    assert_eq!(listed, [a.clone(), b.clone(), c.clone()]);
    assert_eq!(
        resets_without(&listed, std::slice::from_ref(&b)),
        [a.clone(), c.clone()]
    );
    assert_eq!(resets_without(&listed, &listed), Vec::<Sid>::new());
    // Written as PoolSids is, and read back by the same reader.
    let RegValue::MultiSz(units) = sids_value(&listed) else {
        panic!("a REG_MULTI_SZ");
    };
    assert_eq!(pool_sids_from_multi_sz(&units), Ok(vec![a, b, c]));
}

#[test]
fn the_sddl_is_the_one_the_readers_document() {
    assert_eq!(RECORD_SDDL, "D:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)");
}
