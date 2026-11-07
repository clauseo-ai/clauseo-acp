//! A Codex sign-in over Clauseo's own login: `codex login` signs that account out before it
//! starts, so `StartSignIn` and `UseDeviceCode` ask first (`Ask::SwitchAccount`). A decline
//! starts nothing, answers `Done` and leaves no starting mark; an accept starts the sign-in.
//! Signed out, a reused login (refused as before) and Claude ask nothing.

use std::time::UNIX_EPOCH;

use clauseo_agents::{SigninState, Source};
use clauseo_protocol::{AgentId, CommandOutcome, SetupCommand};
use serde_json::json;

use super::fakes::{Rig, command, rig};
use super::{MAIN, claude, codex};
use crate::bridge::lock;
use crate::setup::confirm::{Ask, words};
use crate::test_support::block_on;

fn signed_in(source: Source) -> SigninState {
    SigninState::SignedIn {
        email: Some("clauseo@firm.example".to_owned()),
        plan: None,
        source,
        at: UNIX_EPOCH,
    }
}

fn sign_ins(rig: &Rig) -> usize {
    rig.calls()
        .iter()
        .filter(|(call, _)| *call == "sign_in")
        .count()
}

fn start(agent: &AgentId) -> SetupCommand {
    command("start_sign_in", agent, &json!({}))
}

fn device(agent: &AgentId) -> SetupCommand {
    command("use_device_code", agent, &json!({}))
}

fn switch() -> Ask {
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
    assert_eq!(sign_ins(&rig), 0, "no codex login started");
    assert_eq!(*lock(&rig.confirm.asked), [switch(), switch()]);
    // The decline left no starting mark: a later start is not joined to a start that never ran.
    assert!(
        rig.events.begin_start(&codex()),
        "the starting mark was left behind"
    );
}

#[test]
fn an_accepted_switch_starts_the_sign_in() {
    let rig = rig(true);
    rig.s18_was(&codex(), signed_in(Source::Own));
    let done = Ok(CommandOutcome::Done);
    assert_eq!(block_on(rig.setup().run(MAIN, start(&codex()))), done);
    assert_eq!(sign_ins(&rig), 1);
    assert_eq!(*lock(&rig.confirm.asked), [switch()]);
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
    let (title, message, ok) = words(&switch());
    assert_eq!(title, "Sign in with a different account?");
    assert_eq!(
        message,
        "This signs you out of Codex first, in every matter. Then you can sign in again."
    );
    assert_eq!(ok, "Sign out and continue");
}
