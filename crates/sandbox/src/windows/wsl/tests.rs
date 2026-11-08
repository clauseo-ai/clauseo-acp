//! The apply, check and remove orchestration against the fake machine (`fake.rs`): the VM's
//! lifted WSL, an inbox one with no value, malformed and misplaced values, failed reads and
//! writes, and a machine with no WSL.

use super::fake::{FakeWsl, INBOX_APP, Key, LXSS_APP, WSLC_APP, app, group, stock};
use super::*;
use crate::Blocked;

/// An inbox-only machine: `LxssManager`, its class on an `AppID` with no `LaunchPermission`.
fn inbox() -> FakeWsl {
    let mut fake = FakeWsl {
        services: vec![LXSS_MANAGER],
        group: Some(group()),
        record: Some(WslRecord::default()),
        ..FakeWsl::default()
    };
    fake.classes
        .insert(LXSS_USER_SESSION_INBOX, ClassEntry::AppId(app(INBOX_APP)));
    fake.keys.insert(
        app(INBOX_APP),
        Key {
            service: Some(LXSS_MANAGER),
            value: None,
        },
    );
    fake
}

#[test]
fn the_vms_machine_takes_the_deny_on_both_appids_recorded_first() {
    let mut fake = FakeWsl::lifted();
    assert_eq!(check(&fake), Err(Blocked::WslDenyMissing));
    let applied = apply(&mut fake, &group()).unwrap();
    assert_eq!((applied.targets, applied.written), (2, 2));
    assert!(applied.complete());
    let (lxss, wslc) = (format!("write {LXSS_APP}"), format!("write {WSLC_APP}"));
    // Each target is recorded before its value is written.
    assert_eq!(
        fake.writes,
        ["record", lxss.as_str(), "record", wslc.as_str()]
    );
    for id in [LXSS_APP, WSLC_APP] {
        assert!(fake.sd(id).denies_first(&group()), "{id}");
    }
    let record = fake.record.clone().unwrap();
    assert_eq!(record.app_ids, [app(LXSS_APP), app(WSLC_APP)]);
    assert_eq!(record.absent, Vec::<AppId>::new());
    assert_eq!(check(&fake), Ok(Found::Denied(2)));
    // Again: nothing to write.
    fake.writes.clear();
    let again = apply(&mut fake, &group()).unwrap();
    assert_eq!((again.unchanged, again.written), (2, 0));
    assert!(fake.writes.is_empty(), "{:?}", fake.writes);
}

#[test]
fn removal_gives_back_the_stock_bytes_and_forgets_the_targets() {
    let mut fake = FakeWsl::lifted();
    apply(&mut fake, &group()).unwrap();
    fake.writes.clear();
    let done = remove(&mut fake, &group()).unwrap();
    assert_eq!(
        done,
        WslRemoved {
            cleaned: 2,
            deleted: 0,
            skipped: vec![]
        }
    );
    for id in [LXSS_APP, WSLC_APP] {
        assert_eq!(fake.keys[&app(id)].value, Some(stock()), "{id}");
    }
    assert_eq!(fake.writes.last().map(String::as_str), Some("forget"));
    assert_eq!(fake.record, Some(WslRecord::default()));
    assert_eq!(check(&fake), Err(Blocked::WslDenyMissing));
    // A second removal finds nothing of ours.
    fake.writes.clear();
    let again = remove(&mut fake, &group()).unwrap();
    assert_eq!(again, WslRemoved::default());
    assert_eq!(fake.writes, ["forget"]);
}

#[test]
fn a_missing_value_is_made_from_the_default_and_deleted_on_removal() {
    for default in [None, Some(stock())] {
        let mut fake = inbox();
        fake.default.clone_from(&default);
        assert_eq!(
            check(&fake),
            Err(Blocked::WslDenyMissing),
            "no value: the default"
        );
        let applied = apply(&mut fake, &group()).unwrap();
        assert_eq!(applied.created, 1);
        let made = fake.sd(INBOX_APP);
        let base = default
            .as_deref()
            .map_or_else(LaunchSd::built_in_default, |b| LaunchSd::parse(b).unwrap());
        assert_eq!(made.with_deny(&group()), None);
        assert_eq!(made.without_denies(&group()).0, base);
        assert_eq!(fake.record.as_ref().unwrap().absent, [app(INBOX_APP)]);
        assert_eq!(check(&fake), Ok(Found::Denied(1)));
        remove(&mut fake, &group()).unwrap();
        assert_eq!(fake.keys[&app(INBOX_APP)].value, None, "deleted again");
    }
    // A value an update rewrote since stays, without our deny.
    let mut fake = inbox();
    apply(&mut fake, &group()).unwrap();
    let rewritten = LaunchSd::parse(&stock())
        .unwrap()
        .with_deny(&group())
        .unwrap();
    fake.keys.get_mut(&app(INBOX_APP)).unwrap().value = Some(rewritten.to_bytes());
    let done = remove(&mut fake, &group()).unwrap();
    assert_eq!((done.cleaned, done.deleted), (1, 0));
    assert_eq!(fake.keys[&app(INBOX_APP)].value, Some(stock()));
}

#[test]
fn a_malformed_value_is_never_written_and_is_unverifiable() {
    let mut fake = FakeWsl::lifted();
    fake.keys.get_mut(&app(WSLC_APP)).unwrap().value = Some(vec![1, 0, 4]);
    let applied = apply(&mut fake, &group()).unwrap();
    assert_eq!((applied.written, applied.failed.len()), (1, 1));
    assert!(
        applied.failed[0].starts_with(WSLC_APP),
        "{:?}",
        applied.failed
    );
    assert!(!fake.writes.contains(&format!("write {WSLC_APP}")));
    assert_eq!(fake.keys[&app(WSLC_APP)].value, Some(vec![1, 0, 4]));
    assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable));
    // The inbox WSL alone runs WSL2's VMs too (Windows 10 2004 on): refused the same.
    let mut fake = inbox();
    fake.keys.get_mut(&app(INBOX_APP)).unwrap().value = Some(vec![9; 40]);
    assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable));
    // So is an unreadable one, with only a class registered and no service.
    let mut fake = inbox();
    fake.services.clear();
    fake.fail.push(format!("read {INBOX_APP}"));
    assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable));
    // Removal leaves it and lists it.
    let mut fake = FakeWsl::lifted();
    apply(&mut fake, &group()).unwrap();
    fake.keys.get_mut(&app(WSLC_APP)).unwrap().value = Some(vec![1, 0, 4]);
    let done = remove(&mut fake, &group()).unwrap();
    assert_eq!((done.cleaned, done.skipped.len()), (1, 1));
    assert_eq!(fake.keys[&app(WSLC_APP)].value, Some(vec![1, 0, 4]));
}

#[test]
fn a_deny_after_an_allow_is_missing_until_applied() {
    let mut fake = FakeWsl::lifted();
    // The stock value with our deny last: the merged bytes with the deny moved behind the allows.
    let merged = LaunchSd::parse(&stock())
        .unwrap()
        .with_deny(&group())
        .unwrap();
    let mut bytes = merged.to_bytes();
    bytes[0x34 + 8..].rotate_left(36);
    let late = LaunchSd::parse(&bytes).unwrap();
    assert!(late.names_deny(&group()) && !late.denies_first(&group()));
    apply(&mut fake, &group()).unwrap();
    fake.keys.get_mut(&app(WSLC_APP)).unwrap().value = Some(bytes);
    assert_eq!(check(&fake), Err(Blocked::WslDenyMissing));
    apply(&mut fake, &group()).unwrap();
    assert_eq!(check(&fake), Ok(Found::Denied(2)));
}

#[test]
fn with_no_wsl_nothing_is_refused() {
    let fake = FakeWsl {
        group: Some(group()),
        ..FakeWsl::default()
    };
    assert_eq!(check(&fake), Ok(Found::NoWsl));
    // An `AppID` left by an uninstall, with no service and no class, is no WSL.
    let mut left = FakeWsl::lifted();
    left.services.clear();
    left.classes.clear();
    assert_eq!(check(&left), Ok(Found::NoWsl));
    // A class registered with no service is WSL.
    let mut class_only = FakeWsl::lifted();
    class_only.services.clear();
    assert_eq!(check(&class_only), Err(Blocked::WslDenyMissing));
}

/// A change to the fake machine.
type Change = Box<dyn Fn(&mut FakeWsl)>;

#[test]
fn what_cant_be_read_is_unverifiable_and_missing_comes_first() {
    let applied = || {
        let mut fake = FakeWsl::lifted();
        apply(&mut fake, &group()).unwrap();
        fake
    };
    let cases: Vec<(&str, Change)> = vec![
        (
            "no AppID",
            Box::new(|f| {
                f.classes
                    .insert(LXSS_USER_SESSION_INBOX, ClassEntry::NoAppId);
            }),
        ),
        ("packaged", Box::new(|f| f.packaged = true)),
        (
            "packaged unreadable",
            Box::new(|f| f.fail.push("packaged".to_owned())),
        ),
        (
            "a read",
            Box::new(|f| f.fail.push(format!("read {LXSS_APP}"))),
        ),
        (
            "the AppIDs' list",
            Box::new(|f| f.fail.push("services".to_owned())),
        ),
        (
            "the classes",
            Box::new(|f| f.fail.push("classes".to_owned())),
        ),
    ];
    for (name, change) in &cases {
        let mut fake = applied();
        change(&mut fake);
        assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable), "{name}");
        // A missing deny elsewhere is the cause shown, when the targets could be listed.
        if name.starts_with("the ") {
            continue;
        }
        let mut both = applied();
        change(&mut both);
        both.keys.get_mut(&app(WSLC_APP)).unwrap().value = Some(stock());
        assert_eq!(check(&both), Err(Blocked::WslDenyMissing), "{name}");
    }
    // The inbox WSL alone refuses the same: it runs WSL 2 too (Windows 10 2004 on, and
    // Windows 11's inbox WSL).
    let inbox_cases: Vec<(&str, Change)> = vec![
        (
            "no AppID",
            Box::new(|f| {
                f.classes.insert(WSLC_SESSION_MANAGER, ClassEntry::NoAppId);
            }),
        ),
        ("packaged", Box::new(|f| f.packaged = true)),
        (
            "packaged unreadable",
            Box::new(|f| f.fail.push("packaged".to_owned())),
        ),
        (
            "the AppIDs' list",
            Box::new(|f| f.fail.push("services".to_owned())),
        ),
        (
            "the classes",
            Box::new(|f| f.fail.push("classes".to_owned())),
        ),
    ];
    for (name, change) in &inbox_cases {
        let mut fake = inbox();
        apply(&mut fake, &group()).unwrap();
        assert_eq!(check(&fake), Ok(Found::Denied(1)), "{name}");
        change(&mut fake);
        assert_eq!(
            check(&fake),
            Err(Blocked::WslDenyUnverifiable),
            "inbox, {name}"
        );
    }
    // A service that can't be read counts as present.
    let mut fake = FakeWsl::lifted();
    fake.services.clear();
    fake.classes.clear();
    fake.fail.push(format!("service {WSL_SERVICE}"));
    assert_eq!(check(&fake), Err(Blocked::WslDenyMissing));
    // No group in the record names no deny.
    let mut fake = applied();
    fake.group = None;
    assert_eq!(check(&fake), Err(Blocked::WslDenyMissing));
}

#[test]
fn a_failed_or_changed_write_is_listed_and_the_target_stays_recorded() {
    let mut fake = FakeWsl::lifted();
    fake.fail.push(format!("write {LXSS_APP}"));
    let applied = apply(&mut fake, &group()).unwrap();
    assert_eq!((applied.written, applied.failed.len()), (1, 1));
    assert_eq!(
        fake.record.as_ref().unwrap().app_ids,
        [app(LXSS_APP), app(WSLC_APP)],
        "recorded before the write"
    );
    // A write that stores something else fails its read-back.
    let mut fake = FakeWsl::lifted();
    fake.stores = Some(stock());
    let applied = apply(&mut fake, &group()).unwrap();
    assert_eq!(applied.failed.len(), 2, "{:?}", applied.failed);
    assert!(
        applied.failed[0].contains("read back"),
        "{:?}",
        applied.failed
    );
    // No record to write to: nothing is written.
    let mut fake = FakeWsl::lifted();
    fake.record = None;
    let applied = apply(&mut fake, &group()).unwrap();
    assert_eq!(applied.failed.len(), 2);
    assert!(!fake.writes.iter().any(|line| line.starts_with("write")));
    // Targets that can't be listed are the error.
    let mut fake = FakeWsl::lifted();
    fake.fail.push("services".to_owned());
    assert!(apply(&mut fake, &group()).is_err());
}

#[test]
fn a_removal_that_cant_read_keeps_the_record_for_the_next_run() {
    let mut fake = FakeWsl::lifted();
    apply(&mut fake, &group()).unwrap();
    fake.fail.push(format!("read {WSLC_APP}"));
    assert!(remove(&mut fake, &group()).is_err());
    assert!(!fake.writes.contains(&"forget".to_owned()));
    assert_eq!(fake.record.as_ref().unwrap().app_ids.len(), 2);
    fake.fail.clear();
    let done = remove(&mut fake, &group()).unwrap();
    assert_eq!(done.cleaned, 1, "the first went in the run that stopped");
    // A recorded target that is no longer found is still cleaned.
    let mut fake = FakeWsl::lifted();
    apply(&mut fake, &group()).unwrap();
    fake.classes.clear();
    fake.keys.get_mut(&app(WSLC_APP)).unwrap().service = None;
    assert_eq!(remove(&mut fake, &group()).unwrap().cleaned, 2);
}

#[test]
fn the_refresh_line_names_counts_and_appids_only() {
    let mut fake = FakeWsl::lifted();
    let first = apply(&mut fake, &group()).unwrap();
    assert_eq!(
        first.outcome(),
        Ok(Some(
            "WSL's deny for the sandbox users was put back on 2 of 2 launch permissions".to_owned()
        ))
    );
    assert_eq!(apply(&mut fake, &group()).unwrap().outcome(), Ok(None));
    let mut fake = FakeWsl::lifted();
    fake.fail.push(format!("write {WSLC_APP}"));
    let line = apply(&mut fake, &group()).unwrap().outcome().unwrap_err();
    assert!(
        line.starts_with("1 of 2 launch permissions have no deny: {E9B79997"),
        "{line}"
    );
    assert!(!line.contains("S-1-") && !line.contains('\\'), "{line}");
}
