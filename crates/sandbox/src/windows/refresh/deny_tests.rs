//! The refresh's heal of the group's deny (`deny.rs`) over the fake machine: asked with the
//! recorded group before anything the run may refuse on, never without a group, and a failed
//! heal logged while the run goes on.

use super::fake::*;
use super::*;

const GROUP: &str = "S-1-5-21-1-2-3-1100";

fn group() -> Sid {
    Sid::parse(GROUP).unwrap()
}

/// A machine whose record names the group and whose folder and copy lack its deny.
fn lacking(old: &[u8], new: &[u8], key: &test_signer::TestKey, sig: &str) -> Fake {
    let mut fake = Fake::new(old, new, key, sig);
    fake.record.group_sid = Some(GROUP.to_owned());
    fake
}

const HEALED: &str = "the sandbox's admin copy now denies the sandbox group";

#[test]
fn a_run_heals_the_deny_first_then_refreshes_as_ever() {
    let key = key();
    let mut fake = lacking(&old(), &new(), &key, &good());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert_eq!(fake.denied_for, [group()]);
    assert_eq!(fake.denies, Some(group()));
    assert_eq!(fake.admin_file, Some(new()));
    assert_eq!(
        fake.secured_for,
        [Some(group())],
        "the new copy denies it too"
    );
    let at = |name| fake.calls.iter().position(|call| *call == name).unwrap();
    assert!(at("deny_group") < at("stage"));
    assert_eq!(fake.logs.first(), Some(&(false, HEALED.to_owned())));
}

#[test]
fn a_run_with_nothing_to_copy_or_refused_later_still_heals() {
    let key = key();
    // The copy is current: the old copy's run already swapped this build in.
    let mut current = lacking(&new(), &new(), &key, &good_for(&new()));
    assert_eq!(run_fake(&mut current, &key), RefreshExit::Done);
    assert_eq!(current.denied_for, [group()]);
    assert_eq!(current.denies, Some(group()));
    // Refused on the install folder or its files: healed all the same.
    let mut outside = lacking(&old(), &new(), &key, &good());
    outside.profile = None;
    assert_eq!(run_fake(&mut outside, &key), RefreshExit::OutsideProfile);
    assert_eq!(outside.denies, Some(group()));
    let mut missing = lacking(&old(), &new(), &key, &good());
    missing.source_exe = Err(SourceError::Missing);
    assert_eq!(run_fake(&mut missing, &key), RefreshExit::SourceRefused);
    assert_eq!(missing.denies, Some(group()));
}

#[test]
fn no_group_a_group_that_isnt_a_sid_or_a_held_lock_asks_nothing() {
    let key = key();
    let mut none = Fake::new(&old(), &new(), &key, &good());
    assert_eq!(run_fake(&mut none, &key), RefreshExit::Done);
    assert_eq!(none.denied_for, Vec::<Sid>::new());
    let mut named = lacking(&old(), &new(), &key, &good());
    named.record.group_sid = Some("ClauseoSandboxUsers".to_owned());
    assert_eq!(run_fake(&mut named, &key), RefreshExit::NoRecord);
    assert_eq!(named.denied_for, Vec::<Sid>::new());
    let mut held = lacking(&old(), &new(), &key, &good());
    held.fail = Some("lock");
    assert_ne!(run_fake(&mut held, &key), RefreshExit::Done);
    assert_eq!(held.denied_for, Vec::<Sid>::new());
}

#[test]
fn a_failed_heal_is_logged_and_the_run_goes_on() {
    let key = key();
    let mut fake = lacking(&old(), &new(), &key, &good());
    fake.fail = Some("deny_group");
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert_eq!(fake.admin_file, Some(new()));
    let (error, line) = fake.logs.first().unwrap();
    assert!(
        *error && line.contains("couldn't be given the sandbox group's deny"),
        "{line}"
    );
    // Nothing lacking: nothing logged for it.
    let mut fake = lacking(&old(), &new(), &key, &good());
    fake.denies = Some(group());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert_eq!(fake.denied_for, [group()]);
    assert!(fake.logs.iter().all(|(_, line)| line != HEALED));
}
