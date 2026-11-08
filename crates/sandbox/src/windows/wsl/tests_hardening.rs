//! The hardening `AppID`s ([`HARDENING_APP_IDS`]) against the fake machine: denied with the
//! others when their keys are there, skipped when they aren't, and checked like the others.

use super::fake::{FakeWsl, Key, LXSS_APP, WSLC_APP, app, group, stock};
use super::*;
use crate::Blocked;

/// `wslcsession.exe`'s `AppID`, the per-user WSLC session server.
const SESSION: &str = "{1FAB86C3-F4DF-4271-8E63-6F071C4F708A}";
/// `wsldevicehost.dll`'s `DllSurrogate`'s `AppID`.
const SURROGATE: &str = "{17696EAC-9568-4CF5-BB8C-82515AAD6C09}";

/// The VM's machine, with both hardening keys in place: the MSI's stock value, no service.
fn with_hardening() -> FakeWsl {
    let mut fake = FakeWsl::lifted();
    for id in [SESSION, SURROGATE] {
        fake.keys.insert(
            app(id),
            Key {
                service: None,
                value: Some(stock()),
            },
        );
    }
    fake
}

#[test]
fn the_hardening_appids_are_the_two_named_and_both_parse() {
    assert_eq!(HARDENING_APP_IDS, [SESSION, SURROGATE]);
    assert_eq!(
        HARDENING_APP_IDS
            .into_iter()
            .filter_map(AppId::parse)
            .count(),
        2
    );
}

#[test]
fn present_hardening_appids_get_the_deny_and_are_checked_with_the_others() {
    let mut fake = with_hardening();
    assert_eq!(resolve(&fake).unwrap().app_ids.len(), 4);
    assert_eq!(check(&fake), Err(Blocked::WslDenyMissing));
    let applied = apply(&mut fake, &group()).unwrap();
    assert_eq!((applied.targets, applied.written), (4, 4));
    assert!(applied.complete());
    for id in [LXSS_APP, WSLC_APP, SESSION, SURROGATE] {
        assert!(fake.sd(id).denies_first(&group()), "{id}");
    }
    let record = fake.record.clone().unwrap();
    assert_eq!(
        record.app_ids,
        [app(LXSS_APP), app(WSLC_APP), app(SESSION), app(SURROGATE)]
    );
    assert_eq!(check(&fake), Ok(Found::Denied(4)));
}

#[test]
fn absent_hardening_appids_are_skipped_with_no_write_and_no_refusal() {
    let mut fake = FakeWsl::lifted();
    assert_eq!(
        resolve(&fake).unwrap().app_ids,
        [app(LXSS_APP), app(WSLC_APP)]
    );
    let applied = apply(&mut fake, &group()).unwrap();
    assert_eq!((applied.targets, applied.written), (2, 2));
    assert!(applied.failed.is_empty(), "{:?}", applied.failed);
    assert!(
        fake.writes
            .iter()
            .all(|w| !w.contains(SESSION) && !w.contains(SURROGATE))
    );
    assert!(!fake.keys.contains_key(&app(SESSION)), "never made");
    assert_eq!(check(&fake), Ok(Found::Denied(2)));
}

#[test]
fn hardening_keys_with_no_wsl_refuse_nothing() {
    let mut fake = FakeWsl::default();
    for id in [SESSION, SURROGATE] {
        fake.keys.insert(
            app(id),
            Key {
                service: None,
                value: Some(stock()),
            },
        );
    }
    assert_eq!(check(&fake), Ok(Found::NoWsl));
}

#[test]
fn a_present_hardening_appid_missing_the_deny_is_wsl_deny_missing() {
    let mut fake = FakeWsl::lifted();
    apply(&mut fake, &group()).unwrap();
    assert_eq!(check(&fake), Ok(Found::Denied(2)));
    for id in [SESSION, SURROGATE] {
        fake.keys.insert(
            app(id),
            Key {
                service: None,
                value: Some(stock()),
            },
        );
    }
    assert_eq!(check(&fake), Err(Blocked::WslDenyMissing));
    let applied = apply(&mut fake, &group()).unwrap();
    assert_eq!((applied.targets, applied.written), (4, 2));
    assert_eq!(check(&fake), Ok(Found::Denied(4)));
}

#[test]
fn a_hardening_key_that_cant_be_listed_is_unverifiable() {
    let mut fake = with_hardening();
    fake.fail.push(format!("exists {SURROGATE}"));
    assert_eq!(check(&fake), Err(Blocked::WslDenyUnverifiable));
}

#[test]
fn removal_takes_the_hardening_denies_off_too() {
    let mut fake = with_hardening();
    apply(&mut fake, &group()).unwrap();
    assert_eq!(remove(&mut fake, &group()).unwrap().cleaned, 4);
    for id in [SESSION, SURROGATE, LXSS_APP, WSLC_APP] {
        assert!(!fake.sd(id).names_deny(&group()), "{id}");
    }
}
