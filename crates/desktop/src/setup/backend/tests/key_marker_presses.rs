//! A key that could not be read, pressed through the backend: every sign-in start is refused
//! with `agent.key_unreadable` (a device code, a found login, a different account too), also
//! before the windows' facts have caught up with the key cache, under a firm plan for an agent
//! the marker does not mark, and after a late answer that fails again. A sign-in already going
//! when the key becomes unreadable ends, and a read whose thread panics is taken in as failed.

use std::time::Duration;

use clauseo_agents::keys::Keychain;
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::{AgentId, ErrorCode, SignIn};
use futures::future::{Either, select};
use serde_json::json;

use super::fakes::{Rig, command, rig_keys};
use super::key_marker_denied::{
    OUT, Prompt, is_key_unreadable, load, marked_rig, press, until_read,
};
use super::key_reads::{Never, Passed, stored};
use super::switching::shown;
use super::{MAIN, claude, codex};
use crate::setup::keys::{KEYS_LOAD_BOUND, KeyCache, KeyMarker, KeyNotes};
use crate::setup::refusal::{follow_keys, in_force};
use crate::test_support::{Scratch, within};

/// Waits, on the clock, until `done` holds.
async fn wait_for(done: impl Fn() -> bool) {
    while !done() {
        AsyncIoTimer.after(Duration::from_millis(10)).await;
    }
}

/// Whether the fake `Agents` was asked `name` for `agent`.
fn called(rig: &Rig, name: &str, agent: &AgentId) -> bool {
    rig.calls()
        .iter()
        .any(|(call, a)| *call == name && a == agent)
}

/// `name`'s press for `agent` through the backend: its refusal's code.
fn run(rig: &Rig, name: &str, agent: &AgentId) -> Option<ErrorCode> {
    within(
        name,
        rig.setup().run(MAIN, command(name, agent, &json!({}))),
    )
    .err()
    .map(|error| error.code())
}

#[test]
fn a_device_code_a_found_login_and_another_account_are_refused_while_the_key_is_unreadable() {
    let scratch = Scratch::folder("key-presses-each", &[]);
    let (rig, _notes) = marked_rig(&scratch, Prompt::Denied);
    for (name, call) in [
        ("use_device_code", "sign_in_device_code"),
        ("use_found_login", "use_found_login"),
        ("use_different_account", "use_different_account"),
    ] {
        assert_eq!(
            run(&rig, name, &claude()),
            Some(ErrorCode::AgentKeyUnreadable),
            "{name}"
        );
        assert!(!called(&rig, call, &claude()), "{name}: {:?}", rig.calls());
    }
}

#[test]
fn a_press_before_the_windows_hear_of_the_unreadable_key_is_refused_too() {
    // The cache has marked the key; the note that tells the windows' facts has not been
    // followed yet (`refusal::follow_keys`), so they still show Claude signed out.
    let scratch = Scratch::folder("key-presses-gap", &[]);
    let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
    marker.set(&claude()).unwrap();
    let keychain = stored();
    keychain.deny_reads(true);
    let (keys, _notes) = load(&keychain, marker, &Never);
    let rig = rig_keys(keychain, keys);
    rig.s18_was(&claude(), OUT);
    assert!(matches!(
        shown(&rig, &claude()),
        Some(SignIn::SignedOut { .. })
    ));
    assert_eq!(press(&rig, &claude()), Some(ErrorCode::AgentKeyUnreadable));
    assert!(!called(&rig, "sign_in", &claude()), "{:?}", rig.calls());
}

#[test]
fn under_a_firm_plan_an_unmarked_failed_or_late_read_refuses_a_press() {
    for (name, late) in [
        ("key-presses-firm-failed", false),
        ("key-presses-firm-late", true),
    ] {
        let scratch = Scratch::folder(name, &[]);
        let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
        let keychain = Keychain::memory();
        let (keys, _notes) = if late {
            keychain.hang_reads(true);
            load(&keychain, marker, &Passed)
        } else {
            keychain.fail_reads(true);
            load(&keychain, marker, &Never)
        };
        assert!(!keys.unreadable(&claude()) && keys.read_failed(&claude()));
        let rig = rig_keys(keychain.clone(), keys);
        for agent in [claude(), codex()] {
            rig.s18_was(&agent, OUT);
            rig.backend.compose.show_key(&agent, &rig.backend.keys);
        }
        rig.backend.compose.set_firm(&claude(), true);
        assert_eq!(
            press(&rig, &claude()),
            Some(ErrorCode::AgentKeyUnreadable),
            "late {late}"
        );
        assert!(!called(&rig, "sign_in", &claude()), "late {late}");
        // Codex, with no firm plan, is not refused for its key.
        assert_ne!(press(&rig, &codex()), Some(ErrorCode::AgentKeyUnreadable));
        keychain.hang_reads(false);
    }
}

#[test]
fn under_a_firm_plan_a_read_denied_after_the_bound_stays_refused() {
    for (name, marked) in [
        ("key-presses-late-deny", true),
        ("key-presses-late-deny-u", false),
    ] {
        let scratch = Scratch::folder(name, &[]);
        let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
        if marked {
            marker.set(&claude()).unwrap();
        }
        let keychain = stored();
        keychain.hang_reads(true);
        let (keys, _notes) = load(&keychain, marker, &Passed);
        let rig = rig_keys(keychain.clone(), keys);
        rig.s18_was(&claude(), OUT);
        rig.backend.compose.set_firm(&claude(), true);
        // The prompt, answered after the bound: Deny.
        keychain.deny_reads(true);
        keychain.hang_reads(false);
        let keys = &rig.backend.keys;
        within("the late read", wait_for(|| !keys.reading(&claude())));
        rig.backend.compose.show_key(&claude(), keys);
        assert_eq!(keys.unreadable(&claude()), marked);
        assert_eq!(
            in_force(false, true, keys, &claude()),
            Some(ErrorCode::AgentKeyUnreadable),
            "marked {marked}"
        );
        let line = shown(&rig, &claude());
        assert!(
            is_key_unreadable(line.as_ref()),
            "marked {marked}: {line:?}"
        );
        assert_eq!(press(&rig, &claude()), Some(ErrorCode::AgentKeyUnreadable));
        assert!(!called(&rig, "sign_in", &claude()), "marked {marked}");
    }
}

#[test]
fn a_sign_in_going_when_the_key_becomes_unreadable_ends() {
    // Claude's read waits on macOS's prompt; a sign-in starts before the bound marks it.
    let scratch = Scratch::folder("key-presses-ends", &[]);
    let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
    marker.set(&claude()).unwrap();
    let keychain = stored();
    keychain.hang_reads(true);
    let notes = KeyNotes::new();
    let (keys, first) = KeyCache::start_load(
        keychain.clone(),
        [&claude(), &codex()],
        marker,
        notes.sender(),
    );
    let rig = rig_keys(keychain.clone(), keys);
    rig.s18_was(&claude(), OUT);
    assert_eq!(press(&rig, &claude()), None);
    assert!(called(&rig, "sign_in", &claude()), "{:?}", rig.calls());
    let _outcome = first.wait(&rig.backend.keys, &Passed, KEYS_LOAD_BOUND);
    assert!(rig.backend.keys.unreadable(&claude()));
    let follow = follow_keys(&rig.backend, notes.take().unwrap());
    let ended = wait_for(|| called(&rig, "cancel_sign_in", &claude()));
    within("the sign-in ends", async {
        match select(std::pin::pin!(follow), std::pin::pin!(ended)).await {
            Either::Left(((), _)) => panic!("the notes ended"),
            Either::Right(((), _)) => {}
        }
    });
    assert!(!called(&rig, "cancel_sign_in", &codex()));
    keychain.hang_reads(false);
}

#[test]
fn a_read_that_panics_is_taken_in_as_failed_and_try_again_reads_again() {
    let scratch = Scratch::folder("key-presses-panic", &[]);
    let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
    marker.set(&claude()).unwrap();
    let keychain = stored();
    keychain.panic_reads(true);
    let (keys, notes) = load(&keychain, marker, &Never);
    assert!(keys.unreadable(&claude()) && !keys.reading(&claude()));
    let rig = rig_keys(keychain.clone(), keys);
    rig.s18_was(&claude(), OUT);
    keychain.panic_reads(false);
    assert_eq!(press(&rig, &claude()), Some(ErrorCode::AgentKeyUnreadable));
    until_read(&rig, &mut notes.take().unwrap());
}
