//! A sign-in pressed before Codex is installed meets the switch check (`switch.rs`) at the
//! install's `Ready` as well: the press keeps the lawyer's yes to `Ask::SwitchAccount`, given
//! when pressed; a press kept without one that finds Clauseo's own login at `Ready` is refused
//! and dropped, so no `codex login` starts over that login, and nothing is asked then. A yes
//! goes with its press: once that press is cancelled, a new press asks again.

use clauseo_agents::{InstallState, Source};
use std::sync::atomic::Ordering;

use clauseo_protocol::{CommandOutcome, SignIn};
use serde_json::json;

use super::fakes::{command, rig, setup_events};
use super::pending::{not_installed, pending, pump, ready, sign_ins as shown};
use super::switch_account::{asked, device, different, sign_ins, signed_in, start, switch};
use super::{MAIN, codex};
use crate::bridge::lock;
use crate::test_support::block_on;

/// Pressed with nothing to replace, kept while Codex installs; Clauseo's own login is there by
/// `Ready` (a login of an earlier install's, read from the files): no login starts, no confirm
/// shows then, the press is gone and the sign-in shows S18's state again.
#[test]
fn a_kept_press_finding_the_own_login_at_ready_starts_no_codex_login() {
    for command in [start(&codex()), device(&codex()), different(&codex())] {
        let name = format!("{command:?}");
        // A confirm that would say yes: nothing may ask it at `Ready` either.
        let rig = rig(true);
        not_installed(&rig, &codex());
        let answer = block_on(rig.setup().run(MAIN, command));
        assert_eq!(answer, Ok(CommandOutcome::Done), "{name}");
        assert!(pending(&rig, &codex()).is_some(), "{name}: kept");
        rig.s18_was(&codex(), signed_in(Source::Own));
        let mut feed = rig.events.follow();
        pump(&rig, &codex(), vec![InstallState::Installing, ready()]);
        assert_eq!(
            sign_ins(&rig),
            0,
            "{name}: no codex login: {:?}",
            rig.calls()
        );
        assert_eq!(*lock(&rig.confirm.asked), [], "{name}: no confirm");
        assert_eq!(pending(&rig, &codex()), None, "{name}: dropped");
        let states = shown(setup_events(&mut feed), &codex());
        assert!(
            matches!(states.last(), Some(SignIn::SignedIn { .. })),
            "{name}: S18's state again, not Queued: {states:?}"
        );
    }
}

/// A yes given when pressed, while the own login was there, still holds at `Ready`: the kept
/// press starts the confirmed login, asking once only.
#[test]
fn a_kept_press_keeps_the_yes_given_when_pressed() {
    for (command, call) in [
        (start(&codex()), "sign_in_switching"),
        (device(&codex()), "sign_in_device_code_switching"),
        (different(&codex()), "use_different_account_switching"),
    ] {
        let rig = rig(true);
        not_installed(&rig, &codex());
        rig.s18_was(&codex(), signed_in(Source::Own));
        let answer = block_on(rig.setup().run(MAIN, command));
        assert_eq!(answer, Ok(CommandOutcome::Done), "{call}");
        assert_eq!(sign_ins(&rig), 0, "{call}: kept, not started");
        pump(&rig, &codex(), vec![ready()]);
        assert_eq!(asked(&rig, call), 1, "{call}: {:?}", rig.calls());
        assert_eq!(sign_ins(&rig), 1, "{call}");
        assert_eq!(*lock(&rig.confirm.asked), [switch()], "{call}: asked once");
    }
}

/// A yes does not carry over: pressed while the own login is there and Codex not installed,
/// the lawyer says yes, then cancels the kept press. `Ready` starts nothing, and a new press
/// asks again: a yes then starts the confirmed login on that yes; a no starts nothing at all.
/// No `_switching` call ever runs on the old yes.
#[test]
fn a_yes_does_not_outlive_its_cancelled_press() {
    for second_yes in [true, false] {
        let rig = rig(true);
        not_installed(&rig, &codex());
        rig.s18_was(&codex(), signed_in(Source::Own));
        let done = Ok(CommandOutcome::Done);
        assert_eq!(block_on(rig.setup().run(MAIN, start(&codex()))), done);
        assert!(pending(&rig, &codex()).is_some(), "kept with the yes");
        let cancel = command("cancel_sign_in", &codex(), &json!({}));
        assert_eq!(block_on(rig.setup().run(MAIN, cancel)), done);
        assert_eq!(
            pending(&rig, &codex()),
            None,
            "the cancel dropped it, yes and all"
        );
        pump(&rig, &codex(), vec![ready()]);
        assert_eq!(sign_ins(&rig), 0, "nothing at Ready: {:?}", rig.calls());

        rig.confirm.answer.store(second_yes, Ordering::SeqCst);
        assert_eq!(block_on(rig.setup().run(MAIN, start(&codex()))), done);
        assert_eq!(
            *lock(&rig.confirm.asked),
            [switch(), switch()],
            "asked again ({second_yes})"
        );
        let started = usize::from(second_yes);
        assert_eq!(
            asked(&rig, "sign_in_switching"),
            started,
            "{:?}",
            rig.calls()
        );
        assert_eq!(sign_ins(&rig), started, "{second_yes}: {:?}", rig.calls());
    }
}
