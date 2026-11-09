//! The apply, check and remove orchestration against the fake machine (`fake.rs`): the VM's
//! lifted WSL, an inbox one with no value, malformed and misplaced values, failed reads and
//! writes, and a machine with no WSL.

use super::fake::{FakeWsl, INBOX_APP, Key, LXSS_APP, WSLC_APP, app, group, stock};
use super::*;
use crate::Blocked;
use crate::windows::sids::Sid;

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

#[test]
fn a_class_naming_an_appid_with_no_key_is_a_listed_failure_and_refused() {
    let mut fake = FakeWsl::lifted();
    fake.keys.remove(&app(WSLC_APP));
    let applied = apply(&mut fake, &group()).unwrap();
    assert_eq!((applied.targets, applied.written), (2, 1));
    assert_eq!(applied.failed.len(), 1, "{:?}", applied.failed);
    assert!(
        applied.failed[0].starts_with(WSLC_APP) && applied.failed[0].contains("key isn't there"),
        "{:?}",
        applied.failed
    );
    assert!(!fake.keys.contains_key(&app(WSLC_APP)), "never made");
    assert!(applied.outcome().is_err());
    assert_eq!(check(&fake), Err(Blocked::WslDenyMissing));
}

#[test]
fn a_group_sid_that_isnt_a_local_groups_is_refused_and_nothing_written() {
    assert!(local_group(&group()));
    for text in [
        "S-1-1-0",
        "S-1-5-11",
        "S-1-5-4",
        "S-1-5-18",
        "S-1-5-32-545",
        "S-1-5-32-544",
        "S-1-5-21-1-2-3-500",
        "S-1-5-21-1-2-3-513",
        "S-1-5-21-1-2-1001",
        "S-1-5-21-1-2-3-4-1001",
        "S-1-5-80-1-2-3-4-5",
    ] {
        let sid = Sid::parse(text).unwrap();
        assert!(!local_group(&sid), "{text}");
        let mut fake = FakeWsl::lifted();
        let error = apply(&mut fake, &sid).unwrap_err();
        assert!(error.0.contains("GroupSid"), "{text}: {error}");
        assert!(!error.0.contains(text), "no SID in the line: {error}");
        assert!(fake.writes.is_empty(), "{text}: {:?}", fake.writes);
    }
}

/// The WSL package's packaged-COM full name at `version`.
fn package(version: &str) -> String {
    format!("{PACKAGE_NAME}_{version}_x64__8wekyb3d8bbwe")
}

#[test]
fn a_pending_wsl_upgrade_refuses_the_launch_as_wsls_installer_judges_it() {
    let applied = |packages: &[&str], msi: Option<&str>| {
        let mut fake = FakeWsl::lifted();
        apply(&mut fake, &group()).unwrap();
        fake.packages = packages.iter().map(|v| package(v)).collect();
        fake.msi_version = msi.map(str::to_owned);
        fake
    };
    // No package: nothing to upgrade through; the MSI's version isn't asked.
    assert_eq!(check(&applied(&[], None)), Ok(Found::Denied(2)));
    // The MSI as new as the package (its fourth part left out, as WSL does), or newer.
    for msi in ["3.0.3", "3.0.3.0", "3.0.4", "3.1.0"] {
        assert_eq!(
            check(&applied(&["3.0.3.0"], Some(msi))),
            Ok(Found::Denied(2)),
            "{msi}"
        );
    }
    // Older: an upgrade is pending. The newest package listed counts.
    for (packages, msi) in [
        (&["3.0.3.0"][..], Some("3.0.2")),
        (&["3.0.3.0"], Some("2.9.9.0")),
        (&["2.0.0.0", "3.0.3.0"], Some("2.5.0")),
    ] {
        let fake = applied(packages, msi);
        assert_eq!(
            check(&fake),
            Err(Blocked::WslUpgradePending),
            "{packages:?} {msi:?}"
        );
    }
    // A version that can't be read or parsed can't be compared.
    for (packages, msi) in [
        (&["3.0.3.0"][..], Some("3.0")),
        (&["3.0.3.0"], Some("3.0.3-preview")),
        (&["3.0.x.0"], Some("3.0.3")),
        (&["3.0.3.0", "junk"], Some("3.0.3")),
    ] {
        let fake = applied(packages, msi);
        assert_eq!(
            check(&fake),
            Err(Blocked::WslDenyUnverifiable),
            "{packages:?} {msi:?}"
        );
    }
    let mut fake = applied(&["3.0.3.0"], Some("3.0.3"));
    fake.fail.push("msi version".to_owned());
    assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable));
    let mut fake = applied(&[], None);
    fake.fail.push("packages".to_owned());
    assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable));
    // A missing deny is still the cause shown first.
    let mut fake = applied(&["3.0.3.0"], Some("3.0.2"));
    fake.keys.get_mut(&app(WSLC_APP)).unwrap().value = Some(stock());
    assert_eq!(check(&fake), Err(Blocked::WslDenyMissing));
}

#[test]
fn a_package_with_no_msi_is_unverifiable_where_wsl_2_can_run_and_refuses_nothing_where_not() {
    // With the lifted WSL (or the inbox one) installed, no MSI version is a state WSL doesn't
    // record: unverifiable, not an update with no end.
    for msi in [None, Some(""), Some(" ")] {
        let mut fake = FakeWsl::lifted();
        apply(&mut fake, &group()).unwrap();
        fake.packages = vec![package("3.0.3.0")];
        fake.msi_version = msi.map(str::to_owned);
        assert_eq!(upgrade(&fake), Upgrade::NoMsi, "{msi:?}");
        assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable), "{msi:?}");
    }
    let mut fake = inbox();
    apply(&mut fake, &group()).unwrap();
    fake.packages = vec![package("3.0.3.0")];
    assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable));
    // The package alone, before its MSI is in (no service, no class): WSL is present, but no VM
    // can run until the MSI's install, whose events start the WSL deny task; nothing is refused.
    let mut fake = FakeWsl {
        group: Some(group()),
        record: Some(WslRecord::default()),
        ..FakeWsl::default()
    };
    assert_eq!(check(&fake), Ok(Found::NoWsl));
    fake.packages = vec![package("3.0.3.0")];
    assert_eq!(check(&fake), Ok(Found::Denied(0)));
}

#[test]
fn a_package_registered_for_the_lawyer_only_is_compared_from_the_lawyers_registration() {
    let alone = || FakeWsl {
        group: Some(group()),
        record: Some(WslRecord::default()),
        user_packages: vec![package("3.0.3.0")],
        ..FakeWsl::default()
    };
    // No service, no class, no machine-wide package: the lawyer's own registration is WSL.
    assert_eq!(upgrade(&alone()), Upgrade::NoMsi);
    assert_eq!(check(&alone()), Ok(Found::Denied(0)));
    let lifted = |msi: Option<&str>| {
        let mut fake = FakeWsl::lifted();
        apply(&mut fake, &group()).unwrap();
        fake.user_packages = vec![package("3.0.3.0")];
        fake.msi_version = msi.map(str::to_owned);
        fake
    };
    // The MSI as new as the lawyer's package, or newer.
    for msi in ["3.0.3", "3.1.0"] {
        assert_eq!(upgrade(&lifted(Some(msi))), Upgrade::LawyerOnly, "{msi}");
        assert_eq!(check(&lifted(Some(msi))), Ok(Found::Denied(2)), "{msi}");
    }
    // Older: an upgrade is pending, the lawyer's package's as a machine-wide one's.
    let fake = lifted(Some("2.0.0"));
    assert_eq!(upgrade(&fake), Upgrade::Pending);
    assert_eq!(check(&fake), Err(Blocked::WslUpgradePending));
    // A version that can't be read or parsed: unverifiable where WSL 2 can run.
    let mut failing = lifted(Some("3.0.3"));
    failing.fail.push("msi version".to_owned());
    let mut junk = lifted(Some("3.0.3"));
    junk.user_packages.push(package("3.x.3.0"));
    for fake in [lifted(Some("3.0")), failing, junk] {
        assert_eq!(upgrade(&fake), Upgrade::LawyerUnreadable);
        assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable));
    }
    let mut fake = inbox();
    apply(&mut fake, &group()).unwrap();
    fake.user_packages = vec![package("3.0.3.0")];
    fake.msi_version = Some("3.0".to_owned());
    assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable));
    // With neither service no WSL 2 can run: nothing is refused.
    let mut fake = alone();
    fake.msi_version = Some("junk".to_owned());
    assert_eq!(upgrade(&fake), Upgrade::LawyerUnreadable);
    assert_eq!(check(&fake), Ok(Found::Denied(0)));
    // A machine-wide registration is still read first, and decides.
    let mut fake = lifted(Some("3.0.3"));
    fake.packages = vec![package("3.1.0.0")];
    assert_eq!(check(&fake), Err(Blocked::WslUpgradePending));
    // The lawyer's registrations unreadable: can't be judged.
    let mut fake = alone();
    fake.fail.push("user packages".to_owned());
    assert_eq!(upgrade(&fake), Upgrade::Unreadable);
    assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable));
    // Neither: no package.
    let mut fake = alone();
    fake.user_packages.clear();
    assert_eq!(upgrade(&fake), Upgrade::NoPackage);
    assert_eq!(check(&fake), Ok(Found::NoWsl));
}

#[test]
fn a_version_is_three_or_four_numbers_and_compares_on_the_first_three() {
    assert_eq!(version("2.6.1"), Some((2, 6, 1)));
    assert_eq!(version("2.6.1.0"), Some((2, 6, 1)));
    for bad in ["", "2", "2.6", "2.6.1.0.0", "2.6.a", "-1.0.0", "2..1"] {
        assert_eq!(version(bad), None, "{bad}");
    }
}
