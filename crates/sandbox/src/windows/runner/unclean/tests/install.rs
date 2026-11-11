//! A marker's install id: one of another install is stale, and one with none, or an id that
//! can't be read, keeps the rule from before the ids.

use super::*;

#[test]
fn a_marker_of_another_install_is_stale_and_removed() {
    let mut fake = Fake::new();
    fake.install = Ok(Some(INSTALL.to_owned()));
    // A removed install's escapee marker, a reinstall's counters starting again: it would
    // refuse every logon until the new StopSequence reached its number.
    let stuck = MarkerState::AwaitingStop { stop: 900 };
    fake.installed("old", Some(stuck), Some(OLD_INSTALL));
    // One from before the ids, after the reinstall wrote one.
    fake.installed("none", Some(stuck), None);
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert!(!fake.has("old") && !fake.has("none"), "{:?}", fake.markers);
    assert_eq!(fake.asked("stop"), 0);
    // A marker the current install wrote isn't stale: it still refuses.
    fake.installed("now", Some(stuck), Some(INSTALL));
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
}

#[test]
fn with_no_install_id_or_one_that_cant_be_read_no_marker_is_stale_by_it() {
    let stuck = MarkerState::AwaitingStop { stop: 900 };
    // An install from before the ids: its markers have none, and keep today's rule.
    let mut old = Fake::new();
    old.installed("a1", Some(stuck), None);
    assert!(logon_check(&mut old, &sid(POOL_A)).is_err());
    assert_eq!(old.state("a1"), Some(stuck));
    // A marker with an id while the record has none: its install was removed.
    old.installed("a2", Some(stuck), Some(OLD_INSTALL));
    drop(logon_check(&mut old, &sid(POOL_A)));
    assert!(!old.has("a2"));
    // An id that can't be read makes nothing stale.
    let mut unread = Fake::new();
    unread.install = Err("RegGetValueW failed (5)".to_owned());
    unread.installed("a3", Some(stuck), Some(OLD_INSTALL));
    assert!(logon_check(&mut unread, &sid(POOL_A)).is_err());
    assert!(unread.has("a3"));
}

#[test]
fn an_unreadable_marker_isnt_written_again_while_the_install_id_cant_be_read() {
    let mut fake = Fake::new();
    fake.install = Err("RegGetValueW failed (5)".to_owned());
    fake.marker("x1", None, None);
    // Refused, the stop requested, but the marker left unreadable: written id-less, the next
    // read would take it for another install's and remove it.
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    assert_eq!(fake.markers["x1"], (None, None, None));
    assert_eq!(fake.asked("stop"), 1);
    assert!(start_up(&mut fake).is_ok());
    assert_eq!(fake.markers["x1"], (None, None, None));
    // Once the id reads, it is replaced under it, and isn't stale.
    fake.install = Ok(Some(INSTALL.to_owned()));
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    let read = MarkerState::AwaitingStop { stop: 5 };
    assert_eq!(
        fake.markers["x1"],
        (None, Some(read), Some(INSTALL.to_owned()))
    );
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    assert!(fake.has("x1"), "not removed as another install's");
}
