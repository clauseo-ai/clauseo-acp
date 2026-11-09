//! R17's record across starts (`setup/identity/store.rs`): a swap while Clauseo was closed is
//! caught by the first status at the next start; the first run records; a record that cannot
//! be read is unknown (refused, retryable), never the same; and a sign-in through Clauseo to
//! another account records first, so it never reads as a swap.

use clauseo_agents::{AccountHash, Source};
use clauseo_protocol::{ErrorCode, SignIn};
use serde_json::json;

use super::MAIN;
use super::fakes::{HostCall, command};
use super::identity::{IdRig, signed_in, stop};
use super::switching::shown;
use super::{claude, codex};
use crate::bridge::lock;
use crate::setup::identity::Verdict;
use crate::setup::refusal::{sync_all, wanted};
use crate::test_support::block_on;

const ONE: &str = "lawyer@firm.example";
const OTHER: &str = "someone-else@elsewhere.example";

/// S18's first status at a start: `SignedIn` with `email`, through the pump.
fn first_status(id: &IdRig, email: &str) {
    let rig = &id.rig;
    rig.pumped(async {
        rig.s18_sends(&codex(), signed_in(email, Source::Own));
    });
}

#[test]
fn a_swap_while_clauseo_was_closed_is_caught_at_the_next_start() {
    let earlier = IdRig::new("identity-closed");
    earlier.signs_in(&codex(), ONE);
    let record = earlier.scratch.path().join("data/agents/identity.json");
    let text = std::fs::read_to_string(&record).unwrap();
    assert!(!text.contains(ONE), "a hash, never the email: {text}");
    assert!(
        text.contains(AccountHash::of(ONE).unwrap().as_hex()),
        "{text}"
    );
    // The next start: the record says ONE; until the first status answers, launches wait.
    let id = IdRig::over(earlier.scratch, |identity| identity);
    let rig = &id.rig;
    assert_eq!(rig.backend.wiring.identity.verdict(&codex()), Verdict::Due);
    assert_eq!(
        wanted(&rig.backend, &codex()),
        Some(ErrorCode::AgentIdentityChecking)
    );
    assert_eq!(
        wanted(&rig.backend, &claude()),
        Some(ErrorCode::AgentIdentityChecking),
        "nothing recorded: it waits for its startup check all the same (first run)"
    );
    first_status(&id, OTHER);
    let calls = rig.host.calls();
    assert!(calls.contains(&stop(&codex())), "{calls:?}");
    assert!(
        !calls.contains(&HostCall::RestartAgents(codex())),
        "{calls:?}"
    );
    assert_eq!(
        wanted(&rig.backend, &codex()),
        Some(ErrorCode::SigninAccountChanged)
    );
    assert!(matches!(
        shown(rig, &codex()),
        Some(SignIn::Failed {
            code: ErrorCode::SigninAccountChanged,
            ..
        })
    ));
}

#[test]
fn the_same_account_at_the_next_start_lifts_the_wait() {
    let earlier = IdRig::new("identity-closed-same");
    earlier.signs_in(&codex(), ONE);
    let id = IdRig::over(earlier.scratch, |identity| identity);
    block_on(sync_all(&id.rig.backend));
    first_status(&id, " Lawyer@Firm.example ");
    assert_eq!(
        id.rig.backend.wiring.identity.verdict(&codex()),
        Verdict::Same
    );
    let refusals: Vec<HostCall> = id
        .rig
        .host
        .calls()
        .into_iter()
        .filter(|call| {
            matches!(
                call,
                HostCall::SetLaunchRefused(agent, _) | HostCall::StopAgentsFor(agent, _)
                    if *agent == codex()
            )
        })
        .collect();
    assert_eq!(
        refusals,
        [
            HostCall::SetLaunchRefused(codex(), Some(ErrorCode::AgentIdentityChecking)),
            HostCall::SetLaunchRefused(codex(), None),
        ]
    );
}

#[test]
fn the_first_run_records_the_first_account_seen() {
    let id = IdRig::new("identity-first-run");
    let rig = &id.rig;
    assert_eq!(rig.backend.wiring.identity.verdict(&codex()), Verdict::Idle);
    first_status(&id, ONE);
    assert_eq!(
        rig.backend.wiring.identity.recorded(&codex()),
        AccountHash::of(ONE)
    );
    assert_eq!(rig.backend.wiring.identity.verdict(&codex()), Verdict::Same);
    assert!(
        id.scratch
            .path()
            .join("data/agents/identity.json")
            .is_file()
    );
    let calls = rig.host.calls();
    let identity = |call: &HostCall| {
        matches!(
            call,
            HostCall::SetLaunchRefused(..) | HostCall::StopAgentsFor(..)
        )
    };
    assert!(
        !calls.iter().any(identity),
        "nothing refused, nothing stopped: {calls:?}"
    );
}

#[test]
fn a_record_that_cannot_be_read_is_unknown_never_the_same() {
    let earlier = IdRig::new("identity-corrupt");
    earlier
        .scratch
        .file("data/agents/identity.json", b"{ not json");
    let id = IdRig::over(earlier.scratch, |identity| identity);
    let rig = &id.rig;
    // Both agents wait at start, recorded or not: no record can be trusted.
    for agent in [claude(), codex()] {
        assert_eq!(rig.backend.wiring.identity.verdict(&agent), Verdict::Due);
    }
    first_status(&id, ONE);
    assert_eq!(
        rig.backend.wiring.identity.verdict(&codex()),
        Verdict::Unknown
    );
    assert_eq!(
        wanted(&rig.backend, &codex()),
        Some(ErrorCode::AgentIdentityUnknown),
        "refused, retryable"
    );
    assert!(!rig.host.calls().contains(&stop(&codex())));
    // A sign-in through Clauseo writes the record again and lifts it.
    id.signs_in(&codex(), ONE);
    assert_eq!(wanted(&rig.backend, &codex()), None);
    let text = std::fs::read_to_string(id.scratch.path().join("data/agents/identity.json"));
    assert!(
        text.unwrap()
            .contains(AccountHash::of(ONE).unwrap().as_hex())
    );
}

#[test]
fn a_sign_in_through_clauseo_to_another_account_records_first_and_restarts() {
    let id = IdRig::new("identity-rerecord");
    let rig = &id.rig;
    id.signs_in(&codex(), ONE);
    lock(&rig.agents.signin).insert(codex(), signed_in(ONE, Source::Own));
    // "Sign in with a different account", then a fresh sign-in: each ends on another account.
    let steps = [
        ("use_different_account", "use_different_account", OTHER),
        ("start_sign_in", "sign_in", ONE),
    ];
    for (name, call, email) in steps {
        lock(&rig.agents.answers).insert(call, signed_in(email, Source::Own));
        let sent = command(name, &codex(), &json!({}));
        rig.pumped(async { rig.setup().run(MAIN, sent).await.unwrap() });
        assert_eq!(
            rig.backend.wiring.identity.recorded(&codex()),
            AccountHash::of(email),
            "{name}"
        );
        assert_eq!(rig.backend.wiring.identity.verdict(&codex()), Verdict::Same);
    }
    let calls = rig.host.calls();
    assert!(!calls.contains(&stop(&codex())), "never a swap: {calls:?}");
    let restarts = calls
        .iter()
        .filter(|call| **call == HostCall::RestartAgents(codex()))
        .count();
    assert_eq!(restarts, 2, "{calls:?}");
}

#[test]
fn a_refused_sign_in_leaves_nothing_expected() {
    let id = IdRig::new("identity-refused-start");
    let rig = &id.rig;
    id.signs_in(&codex(), ONE);
    *lock(&rig.agents.refusal) = Some(clauseo_agents::SigninError::NotReady(codex()));
    let sent = command("start_sign_in", &codex(), &json!({}));
    assert!(block_on(rig.setup().run(MAIN, sent)).is_err());
    *lock(&rig.agents.refusal) = None;
    // A status check's `SignedIn` of another account is then a change, not a sign-in.
    first_status(&id, OTHER);
    assert_eq!(
        rig.backend.wiring.identity.verdict(&codex()),
        Verdict::Changed
    );
}
