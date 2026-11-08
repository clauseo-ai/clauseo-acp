//! A Codex sign-in over Clauseo's own login: `codex login` signs that account out before it
//! starts, so `StartSignIn` and `UseDeviceCode` ask first (`Ask::SwitchAccount`). A decline
//! starts nothing, answers `Done` and leaves no starting mark; an accept starts the sign-in as
//! confirmed. `UseDifferentAccount` asks the same.
//! Signed out, a reused login (refused as before) and Claude ask nothing.

use std::time::UNIX_EPOCH;

use clauseo_agents::{SigninState, Source};
use clauseo_protocol::{AgentId, CommandOutcome, Language, SetupCommand};
use serde_json::json;

use super::fakes::{Rig, command, rig};
use super::{MAIN, claude, codex};
use crate::bridge::lock;
use crate::setup::confirm::{Ask, words};
use crate::test_support::block_on;

pub(super) fn signed_in(source: Source) -> SigninState {
    SigninState::SignedIn {
        email: Some("clauseo@firm.example".to_owned()),
        plan: None,
        source,
        at: UNIX_EPOCH,
    }
}

/// How many times the fake `Agents` was asked `name`.
pub(super) fn asked(rig: &Rig, name: &str) -> usize {
    rig.calls().iter().filter(|(call, _)| *call == name).count()
}

/// Every call that would start a Codex login, confirmed or not.
pub(super) fn sign_ins(rig: &Rig) -> usize {
    [
        "sign_in",
        "sign_in_device_code",
        "sign_in_switching",
        "sign_in_device_code_switching",
        "use_different_account",
        "use_different_account_switching",
    ]
    .iter()
    .map(|name| asked(rig, name))
    .sum()
}

pub(super) fn start(agent: &AgentId) -> SetupCommand {
    command("start_sign_in", agent, &json!({}))
}

pub(super) fn device(agent: &AgentId) -> SetupCommand {
    command("use_device_code", agent, &json!({}))
}

pub(super) fn different(agent: &AgentId) -> SetupCommand {
    command("use_different_account", agent, &json!({}))
}

pub(super) fn switch() -> Ask {
    Ask::SwitchAccount {
        name: "Codex".to_owned(),
    }
}

#[test]
fn a_declined_switch_starts_no_codex_login_and_leaves_no_mark() {
    let rig = rig(false);
    rig.s18_was(&codex(), signed_in(Source::Own));
    let done = Ok(CommandOutcome::Done);
    assert_eq!(block_on(rig.setup().run(MAIN, start(&codex()))), done);
    assert_eq!(block_on(rig.setup().run(MAIN, device(&codex()))), done);
    assert_eq!(block_on(rig.setup().run(MAIN, different(&codex()))), done);
    assert_eq!(sign_ins(&rig), 0, "no codex login started");
    assert_eq!(*lock(&rig.confirm.asked), [switch(), switch(), switch()]);
    // The decline left no starting mark: a later start is not joined to a start that never ran.
    assert!(
        rig.events.begin_start(&codex()),
        "the starting mark was left behind"
    );
}

/// A yes starts the sign-in with the switch confirmed, so S18 lets it replace the login; the
/// "different account" action asks too.
#[test]
fn an_accepted_switch_starts_the_sign_in_as_confirmed() {
    let done = Ok(CommandOutcome::Done);
    for (command, call) in [
        (start(&codex()), "sign_in_switching"),
        (device(&codex()), "sign_in_device_code_switching"),
        (different(&codex()), "use_different_account_switching"),
    ] {
        let rig = rig(true);
        rig.s18_was(&codex(), signed_in(Source::Own));
        assert_eq!(block_on(rig.setup().run(MAIN, command)), done, "{call}");
        assert_eq!(asked(&rig, call), 1, "{call}");
        assert_eq!(sign_ins(&rig), 1, "{call}");
        assert_eq!(*lock(&rig.confirm.asked), [switch()], "{call}");
    }
}

#[test]
fn signed_out_reused_and_claude_ask_nothing() {
    let rig = rig(true);
    rig.s18_was(&codex(), SigninState::SignedOut { found: None });
    assert!(block_on(rig.setup().run(MAIN, start(&codex()))).is_ok());
    let rig_reused = super::fakes::rig(true);
    rig_reused.s18_was(&codex(), signed_in(Source::Reused));
    assert!(block_on(rig_reused.setup().run(MAIN, start(&codex()))).is_err());
    let rig_claude = super::fakes::rig(true);
    rig_claude.s18_was(&claude(), signed_in(Source::Own));
    assert!(block_on(rig_claude.setup().run(MAIN, start(&claude()))).is_ok());
    for asked in [&rig, &rig_reused, &rig_claude] {
        assert_eq!(*lock(&asked.confirm.asked), [], "no confirm");
    }
}

#[test]
fn the_switch_words_name_the_agent() {
    let words = words(&switch(), Language::En);
    assert_eq!(words.title, "Sign in with a different account?");
    assert_eq!(
        words.message,
        "This signs you out of Codex first, in every matter. Then you can sign in with the account \
         you want."
    );
    assert_eq!(words.ok, "Sign out and continue");
    assert_eq!(words.cancel, "Cancel");
}

/// In Hindi each setup confirm says the Hindi words, its slots filled.
#[test]
fn the_setup_confirms_follow_the_language() {
    let hi = words(&switch(), Language::Hi);
    assert_eq!(hi.title, "किसी दूसरे खाते से साइन इन करें?");
    assert!(
        hi.message.starts_with("इससे पहले हर मामले में Codex से"),
        "{}",
        hi.message
    );
    assert_eq!(
        (hi.ok.as_str(), hi.cancel.as_str()),
        ("साइन आउट करके आगे बढ़ें", "रद्द करें")
    );
    let name = "Claude".to_owned();
    let url = Some("https://ai.firm.example".to_owned());
    let set = words(
        &Ask::Endpoint {
            name: name.clone(),
            url,
        },
        Language::Hi,
    );
    assert_eq!(set.title, "Claude के लिए आपकी फ़र्म का पता इस्तेमाल करें?");
    assert_eq!(
        set.message,
        "Claude आपका काम अपनी सामान्य सेवा के बजाय https://ai.firm.example पर भेजेगा।"
    );
    let clear = words(&Ask::Endpoint { name, url: None }, Language::Hi);
    assert_eq!(clear.message, "Claude अपनी सामान्य सेवा पर लौट जाएगा।");
    assert_eq!(clear.ok, "इस्तेमाल बंद करें");
}
