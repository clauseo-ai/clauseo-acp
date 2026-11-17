//! A marked agent whose keychain prompt was denied or left unanswered (the VM's rows D7, A-K6
//! and K11, macOS, a self-update's first read): its card is `Failed { agent.key_denied }` after
//! Deny and `Failed { agent.key_unreadable }` past the bound, and its launches are refused with
//! that code, never `SignedOut` or a sign-in; a sign-in
//! press (the card's Try again) is refused with that code too, and reads the key again, so the
//! key is taken in once the prompt is answered, with no restart. Under a firm plan the same.
//! Remove key (Settings › Plans) deletes it with no read, and the agent is signed out.
//!
//! Deny on macOS's prompt fails the read with `errSecAuthFailed` (-25293), Cancel with
//! `errSecUserCanceled` (-128): apple-native-keyring-store 1.0.2's `keychain::decode_error`
//! maps both to keyring's `PlatformFailure`, which `clauseo_agents::keys` gives as
//! `KeyError::Keychain { kind: PLATFORM_FAILURE }`, as `Keychain::deny_reads` does here.

use std::time::UNIX_EPOCH;

use clauseo_agents::SigninState;
use clauseo_agents::keys::Keychain;
use clauseo_process::Timer;
use clauseo_protocol::{AgentId, CommandOutcome, ErrorCode, SignIn, SignInMethod};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use serde_json::json;

use super::fakes::{Rig, command, rig_keys};
use super::key_reads::{Never, Passed, stored};
use super::switching::shown;
use super::{MAIN, SETTINGS, claude, codex};
use crate::setup::compose::{Facts, compose};
use crate::setup::keys::{KEYS_LOAD_BOUND, KeyCache, KeyMarker, KeyNote, KeyNotes};
use crate::setup::refusal::in_force;
use crate::test_support::{Scratch, within};

pub(super) const OUT: SigninState = SigninState::SignedOut { found: None };

/// How the prompt went: denied, or not answered by the first load's bound.
#[derive(Clone, Copy, Debug)]
pub(super) enum Prompt {
    Denied,
    Unanswered,
}

impl Prompt {
    /// The code of a marked key read so: the keychain's error is `agent.key_denied`, no answer
    /// by the bound `agent.key_unreadable`.
    pub(super) fn code(self) -> ErrorCode {
        match self {
            Self::Denied => ErrorCode::AgentKeyDenied,
            Self::Unanswered => ErrorCode::AgentKeyUnreadable,
        }
    }
}

/// A rig whose first load read Claude's stored key, marked by the "key stored" marker in
/// `scratch`, with the prompt going as `prompt`; both agents signed out as S18 sees them, and the
/// windows told as `start.rs` tells them. The notes of each mark's change come with it.
pub(super) fn marked_rig(scratch: &Scratch, prompt: Prompt) -> (Rig, UnboundedReceiver<KeyNote>) {
    let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
    marker.set(&claude()).unwrap();
    let keychain = stored();
    let (keys, notes) = match prompt {
        Prompt::Denied => {
            // Denied inside the bound (S-D7, at 8 s): the load ends with every read answered,
            // `outcome="read"`, not at the bound.
            keychain.deny_reads(true);
            load(&keychain, marker, &Never)
        }
        Prompt::Unanswered => {
            keychain.hang_reads(true);
            load(&keychain, marker, &Passed)
        }
    };
    let rig = rig_keys(keychain, keys);
    for agent in [claude(), codex()] {
        rig.s18_was(&agent, OUT);
        rig.backend.compose.show_key(&agent, &rig.backend.keys);
    }
    (rig, notes.take().unwrap())
}

/// The setup thread's first load over `keychain`, `marker` and `timer`, with its notes.
pub(super) fn load(
    keychain: &Keychain,
    marker: KeyMarker,
    timer: &impl Timer,
) -> (KeyCache, KeyNotes) {
    let notes = KeyNotes::new();
    let agents = [&claude(), &codex()];
    let keys = KeyCache::load_bounded(
        keychain.clone(),
        agents,
        marker,
        notes.sender(),
        timer,
        KEYS_LOAD_BOUND,
    );
    (keys, notes)
}

/// Whether `shown` is a stored key not read, of either kind.
pub(super) fn is_key_unreadable(shown: Option<&SignIn>) -> bool {
    matches!(
        shown,
        Some(SignIn::Failed {
            code: ErrorCode::AgentKeyUnreadable | ErrorCode::AgentKeyDenied,
            ..
        })
    )
}

/// The code of `shown` when it is `Failed`.
pub(super) fn failed_code(shown: Option<&SignIn>) -> Option<ErrorCode> {
    match shown {
        Some(SignIn::Failed { code, .. }) => Some(*code),
        _ => None,
    }
}

/// A sign-in press for `agent` from the first run (the card's Try again): its refusal's code.
pub(super) fn press(rig: &Rig, agent: &AgentId) -> Option<ErrorCode> {
    let start = command("start_sign_in", agent, &json!({}));
    within("the press", rig.setup().run(MAIN, start))
        .err()
        .map(|error| error.code())
}

/// Waits until a read lands Claude's key in memory, note by note.
pub(super) fn until_read(rig: &Rig, notes: &mut UnboundedReceiver<KeyNote>) {
    while !rig.backend.keys.in_memory(&claude()) {
        let note = within("a key note", notes.next());
        assert!(note.is_some(), "the notes ended before the key was read");
    }
}

/// What a marked agent shows and how its launches go while its key is unreadable.
fn assert_refused(rig: &Rig, prompt: Prompt) {
    let keys = &rig.backend.keys;
    assert!(
        keys.unreadable(&claude()) && keys.has(&claude()),
        "{prompt:?}"
    );
    assert_eq!(keys.get(&claude()), None);
    let line = shown(rig, &claude());
    assert_eq!(failed_code(line.as_ref()), Some(prompt.code()), "{line:?}");
    assert!(
        !matches!(line, Some(SignIn::SignedOut { .. })),
        "never Sign in"
    );
    for firm in [false, true] {
        assert_eq!(
            in_force(false, firm, keys, &claude()),
            Some(prompt.code()),
            "{prompt:?}, firm {firm}"
        );
    }
    // Try again: refused with the key's own code, and no plan sign-in starts (S-D7 saw
    // `kind="sign_in" why="claude_ai"` and claude.ai's login open in the browser).
    assert_eq!(press(rig, &claude()), Some(prompt.code()));
    assert!(
        rig.calls().iter().all(|(call, _)| *call != "sign_in"),
        "{prompt:?}: {:?}",
        rig.calls()
    );
}

#[test]
fn a_denied_prompt_for_a_marked_agent_is_key_denied_never_signed_out() {
    let scratch = Scratch::folder("key-denied-deny", &[]);
    let (rig, _notes) = marked_rig(&scratch, Prompt::Denied);
    assert_refused(&rig, Prompt::Denied);
    // Codex, unmarked, keeps #1132's rule: a failed read launches it without a key.
    assert_eq!(in_force(false, false, &rig.backend.keys, &codex()), None);
    assert!(matches!(
        shown(&rig, &codex()),
        Some(SignIn::SignedOut { .. })
    ));
}

#[test]
fn an_unanswered_prompt_for_a_marked_agent_is_key_unreadable_never_signed_out() {
    let scratch = Scratch::folder("key-denied-timeout", &[]);
    let (rig, mut notes) = marked_rig(&scratch, Prompt::Unanswered);
    assert_refused(&rig, Prompt::Unanswered);
    rig.keychain.hang_reads(false);
    until_read(&rig, &mut notes);
}

#[test]
fn a_prompt_answered_late_lands_the_key_without_a_restart() {
    // A-K6: the prompt is answered (Always Allow) minutes after the bound; the read that timed
    // out still runs and is taken in. A press meanwhile starts no second read.
    let scratch = Scratch::folder("key-denied-late", &[]);
    let (rig, mut notes) = marked_rig(&scratch, Prompt::Unanswered);
    assert_eq!(press(&rig, &claude()), Some(ErrorCode::AgentKeyUnreadable));
    rig.keychain.hang_reads(false);
    until_read(&rig, &mut notes);
    rig.backend.compose.show_key(&claude(), &rig.backend.keys);
    assert!(matches!(
        shown(&rig, &claude()),
        Some(SignIn::SignedIn {
            method: SignInMethod::ApiKey,
            ..
        })
    ));
    assert_eq!(in_force(false, false, &rig.backend.keys, &claude()), None);
}

#[test]
fn denied_then_allowed_the_card_s_try_again_reads_the_key_again() {
    // D7: Deny; then the lawyer presses Try again and, at the prompt that brings back, Always
    // Allow. The press reads the key again: no restart, no sign-in.
    let scratch = Scratch::folder("key-denied-retry", &[]);
    let (rig, mut notes) = marked_rig(&scratch, Prompt::Denied);
    rig.keychain.deny_reads(false);
    assert_eq!(press(&rig, &claude()), Some(ErrorCode::AgentKeyDenied));
    until_read(&rig, &mut notes);
    rig.backend.compose.show_key(&claude(), &rig.backend.keys);
    assert!(matches!(
        shown(&rig, &claude()),
        Some(SignIn::SignedIn {
            method: SignInMethod::ApiKey,
            ..
        })
    ));
    assert_eq!(in_force(false, false, &rig.backend.keys, &claude()), None);
    assert!(rig.calls().iter().all(|(call, _)| *call != "sign_in"));
    // Read: the key bars a sign-in as any stored key does.
    assert_eq!(press(&rig, &claude()), Some(ErrorCode::SigninKeyInUse));
}

#[test]
fn under_a_firm_plan_a_denied_or_unanswered_prompt_refuses_and_offers_no_sign_in() {
    for (name, prompt) in [
        ("key-denied-firm-deny", Prompt::Denied),
        ("key-denied-firm-timeout", Prompt::Unanswered),
    ] {
        let scratch = Scratch::folder(name, &[]);
        let (rig, mut notes) = marked_rig(&scratch, prompt);
        rig.backend.compose.set_firm(&claude(), true);
        let line = shown(&rig, &claude());
        assert_eq!(failed_code(line.as_ref()), Some(prompt.code()), "{line:?}");
        assert_eq!(
            in_force(false, true, &rig.backend.keys, &claude()),
            Some(prompt.code())
        );
        // The key's bar goes first: never `signin.endpoint_in_use`, never a sign-in.
        assert_eq!(press(&rig, &claude()), Some(prompt.code()));
        rig.backend.set_firm_policy(&claude(), true);
        assert_eq!(press(&rig, &claude()), Some(prompt.code()));
        assert!(rig.calls().iter().all(|(call, _)| *call != "sign_in"));
        rig.keychain.hang_reads(false);
        if matches!(prompt, Prompt::Unanswered) {
            until_read(&rig, &mut notes);
        }
    }
}

#[test]
fn an_unmarked_unanswered_read_refuses_only_under_a_firm_plan() {
    // Nothing marked: no key Clauseo stored. At the bound such a read launches the agent without
    // a key (#1132's option A), unless a firm plan, which a failed read refuses.
    let scratch = Scratch::folder("key-denied-unmarked", &[]);
    let keychain = Keychain::memory();
    keychain.hang_reads(true);
    let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
    let (keys, notes) = load(&keychain, marker, &Passed);
    assert!(!keys.unreadable(&claude()) && keys.read_failed(&claude()));
    assert_eq!(in_force(false, false, &keys, &claude()), None);
    assert_eq!(
        in_force(false, true, &keys, &claude()),
        Some(ErrorCode::AgentKeyUnreadable)
    );
    let facts = |firm: bool| Facts {
        firm: firm.then_some(UNIX_EPOCH),
        read_failed: Some(UNIX_EPOCH),
        ..Facts::default()
    };
    assert!(matches!(
        compose(&claude(), Some(&OUT), facts(false)),
        Some(SignIn::SignedOut { .. })
    ));
    assert!(is_key_unreadable(
        compose(&claude(), Some(&OUT), facts(true)).as_ref()
    ));
    // The read answers no key: nothing refuses it, firm or not.
    let mut notes = notes.take().unwrap();
    keychain.hang_reads(false);
    while keys.read_failed(&claude()) || keys.read_failed(&codex()) {
        let note = within("a key note", notes.next());
        assert!(note.is_some(), "the notes ended before the reads answered");
    }
    assert_eq!(in_force(false, true, &keys, &claude()), None);
}

#[test]
fn remove_key_under_an_unreadable_key_never_reads_it_and_ends_the_mark() {
    // The way out when the keychain keeps refusing (decision record 0048): Settings › Plans'
    // Remove key deletes the item, which needs no read. Any read from here on would wait for
    // good, so a removal that read first would not answer.
    for (name, prompt) in [
        ("key-denied-remove-deny", Prompt::Denied),
        ("key-denied-remove-timeout", Prompt::Unanswered),
    ] {
        let scratch = Scratch::folder(name, &[]);
        let (rig, _notes) = marked_rig(&scratch, prompt);
        rig.keychain.deny_reads(false);
        rig.keychain.hang_reads(true);
        let clear = command("clear_agent_key", &claude(), &json!({}));
        assert_eq!(
            within("the removal", rig.setup().run(SETTINGS, clear)),
            Ok(CommandOutcome::Done),
            "{prompt:?}"
        );
        let keys = &rig.backend.keys;
        assert!(!keys.unreadable(&claude()), "{prompt:?}");
        assert!(!keys.denied(&claude()), "{prompt:?}");
        assert!(!keys.has(&claude()), "{prompt:?}");
        assert!(!keys.read_failed(&claude()), "{prompt:?}");
        let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
        assert!(
            !marker.marked().contains(&claude()),
            "{prompt:?}: the mark is cleared"
        );
        assert_eq!(in_force(false, false, keys, &claude()), None, "{prompt:?}");
        // Plain "no key": signed out, and a plan sign-in is offered again.
        assert_eq!(
            shown(&rig, &claude()),
            Some(SignIn::SignedOut { found: None }),
            "{prompt:?}"
        );
        assert_eq!(
            rig.backend.compose.sign_in_bar(&claude()),
            None,
            "{prompt:?}"
        );
        let pressed = press(&rig, &claude());
        assert!(
            !matches!(
                pressed,
                Some(
                    ErrorCode::AgentKeyUnreadable
                        | ErrorCode::AgentKeyDenied
                        | ErrorCode::SigninKeyInUse
                )
            ),
            "{prompt:?}: {pressed:?}"
        );
        rig.keychain.hang_reads(false);
        assert_eq!(
            rig.keychain.load(&claude()).unwrap(),
            None,
            "{prompt:?}: deleted"
        );
    }
}
