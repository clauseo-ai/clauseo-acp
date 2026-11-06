//! R17's watcher in every state (the security review's M1, M2, M3 and M4; the desktop review's
//! A1 and A2; the sweep's G1 to G3): a swap right after a record, during a sign-in that never
//! confirms, after a sign-out, before the first look of a start, or on a macOS Keychain login
//! with a file; a burst of changes gives one check; a check a sign-out overtakes leaves the
//! sign-out's verdict; and one agent's hung status holds up no other's check.

use std::future::poll_fn;
use std::path::Path;
use std::task::Poll;

use clauseo_agents::{AccountRead, LoginFrom, SigninState, sign_in_limit};
use clauseo_protocol::{AgentId, AgentKind, ErrorCode};
use futures::future::join;

use super::identity::{IdRig, refused, stop};
use super::{claude, codex};
use crate::setup::identity::{KEYCHAIN_STALE, Verdict, check, on_state, poll_once};
use crate::setup::refusal::wanted;
use crate::test_support::block_on;

const ONE: &str = "lawyer@firm.example";
const OTHER: &str = "someone-else@elsewhere.example";

/// Claude's store file in Clauseo's home, relative to the scratch folder.
fn claude_store() -> String {
    format!(
        "homes/claude-acp/{}",
        clauseo_sandbox::CLAUDE_STORE_FILES[0]
    )
}

#[expect(
    clippy::disallowed_methods,
    reason = "test set-up swaps and removes dummy login files in its scratch folder"
)]
pub(super) mod files {
    use std::path::Path;

    pub(super) fn rename(from: &Path, to: &Path) {
        std::fs::rename(from, to).unwrap();
    }

    pub(crate) fn remove(path: &Path) {
        std::fs::remove_file(path).unwrap();
    }
}

/// Another file put in place of `rel` (relative to the scratch folder): another file id.
fn swap(id: &IdRig, rel: &str, tag: &str) {
    let to = id.scratch.path().join(rel);
    let parent = Path::new(rel).parent().unwrap().to_str().unwrap();
    let other = id.scratch.file(&format!("{parent}/{tag}"), b"{}");
    files::rename(&other, &to);
}

fn verdict(id: &IdRig, agent: &AgentId) -> Verdict {
    id.rig.backend.wiring.identity.verdict(agent)
}

/// Lets the other futures of a join run once.
async fn yield_now() {
    let mut yielded = false;
    poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}

/// Yields until `holds`, or panics after many tries.
pub(super) async fn until(what: &str, holds: impl Fn() -> bool) {
    for _ in 0..1000 {
        if holds() {
            return;
        }
        yield_now().await;
    }
    panic!("never: {what}");
}

#[test]
fn a_swap_right_after_a_record_is_caught_at_the_first_look() {
    let id = IdRig::new("identity-after-record");
    let (rig, agent) = (&id.rig, codex());
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    id.signs_in(&agent, ONE);
    // Inside the 2 s before the first look: the record holds for the stamps before its read.
    id.replace_codex_login("swapped");
    rig.agents.accounts.names(&agent, OTHER);
    id.look(0);
    assert_eq!(verdict(&id, &agent), Verdict::Changed);
    assert!(rig.host.calls().contains(&stop(&agent)));
}

#[test]
fn a_first_run_record_from_a_check_keeps_the_stamps_from_before_its_read() {
    let id = IdRig::over(IdRig::scratch("identity-first-stamps"), |identity| identity);
    let (rig, agent) = (&id.rig, codex());
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    rig.agents.accounts.names(&agent, ONE);
    rig.agents
        .accounts
        .answers(&claude(), AccountRead::NoAccount);
    id.look(0);
    assert_eq!(verdict(&id, &agent), Verdict::Same, "trusted on first use");
    id.replace_codex_login("swapped");
    rig.agents.accounts.names(&agent, OTHER);
    id.look(2);
    assert_eq!(verdict(&id, &agent), Verdict::Changed);
}

#[test]
fn a_sign_in_that_never_confirms_holds_off_the_check_for_its_bound_only() {
    let id = IdRig::new("identity-pending-bound");
    let (rig, agent) = (&id.rig, codex());
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    id.signs_in(&agent, ONE);
    // Another sign-in starts and never ends; meanwhile the file changes to another account.
    crate::setup::identity::expect(&rig.backend, &agent);
    id.replace_codex_login("swapped");
    rig.agents.accounts.names(&agent, OTHER);
    id.look(60);
    assert_eq!(id.reads(), 0, "a sign-in under way is not checked");
    let bound = sign_in_limit(AgentKind::Codex).as_secs();
    id.look(bound + 1);
    assert_eq!(id.reads(), 1, "past its bound it is checked as any other");
    assert_eq!(verdict(&id, &agent), Verdict::Changed);
}

#[test]
fn a_deleted_login_written_again_with_another_account_is_refused() {
    let states = [
        (
            "identity-deleted-out",
            SigninState::SignedOut { found: None },
        ),
        (
            "identity-deleted-found",
            SigninState::LoginFound {
                from: LoginFrom::Codex,
                email: None,
            },
        ),
    ];
    for (test, gone) in states {
        let id = IdRig::new(test);
        let (rig, agent) = (&id.rig, codex());
        let login = id.scratch.file("homes/codex-acp/auth.json", b"{}");
        id.signs_in(&agent, ONE);
        id.look(0);
        // The login goes: S18 says signed out (or found another), which refuses nothing.
        files::remove(&login);
        block_on(on_state(&rig.backend, &agent, &gone));
        assert_eq!(verdict(&id, &agent), Verdict::Idle);
        rig.agents.accounts.answers(&agent, AccountRead::NoAccount);
        id.look(2);
        assert_eq!(
            verdict(&id, &agent),
            Verdict::Unknown,
            "still watched: {gone:?}"
        );
        // Another account's login is written in: checked at once, and refused.
        id.scratch.file("homes/codex-acp/auth.json", b"{ }");
        rig.agents.accounts.names(&agent, OTHER);
        id.look(4);
        assert_eq!(verdict(&id, &agent), Verdict::Changed, "{gone:?}");
        assert_eq!(
            wanted(&rig.backend, &agent),
            Some(ErrorCode::SigninAccountChanged)
        );
    }
}

#[test]
fn a_start_with_an_account_recorded_checks_at_the_first_look_never_takes_the_file() {
    let earlier = IdRig::new("identity-load-swap");
    earlier.scratch.file("homes/codex-acp/auth.json", b"{}");
    earlier.signs_in(&codex(), ONE);
    let id = IdRig::over(earlier.scratch, |identity| identity);
    let (rig, agent) = (&id.rig, codex());
    // Swapped while Clauseo was closed, and no status comes before the first look.
    id.replace_codex_login("swapped");
    rig.agents.accounts.names(&agent, OTHER);
    id.look(0);
    assert_eq!(verdict(&id, &agent), Verdict::Changed);
    assert_eq!(
        wanted(&rig.backend, &agent),
        Some(ErrorCode::SigninAccountChanged)
    );
}

#[test]
fn a_macos_claude_login_is_due_again_every_ten_minutes_with_a_file_and_signed_out() {
    let id = IdRig::with("identity-keychain-always", |identity| {
        identity.with_keychain_fallback(true)
    });
    let (rig, agent) = (&id.rig, claude());
    // A store file is there (a session may plant one): the timer runs all the same.
    id.scratch.file(&claude_store(), b"{}");
    id.signs_in(&agent, ONE);
    let stale = KEYCHAIN_STALE.as_secs();
    id.look(stale - 1);
    assert_eq!(verdict(&id, &agent), Verdict::Same);
    id.look(stale);
    assert_eq!(verdict(&id, &agent), Verdict::Due);
    assert_eq!(
        rig.host.calls().last(),
        Some(&refused(&agent, Some(ErrorCode::AgentIdentityChecking)))
    );
    rig.agents.accounts.names(&agent, ONE);
    assert_eq!(block_on(check(&rig.backend, &agent)), Verdict::Same);
    // Signed out, with the account recorded: the timer runs on.
    block_on(on_state(
        &rig.backend,
        &agent,
        &SigninState::SignedOut { found: None },
    ));
    assert_eq!(verdict(&id, &agent), Verdict::Idle);
    id.look(stale - 1);
    assert_eq!(verdict(&id, &agent), Verdict::Idle);
    id.look(stale);
    assert_eq!(verdict(&id, &agent), Verdict::Due);
}

#[test]
fn a_burst_of_changes_between_looks_gives_one_check() {
    let id = IdRig::new("identity-burst");
    let (rig, agent) = (&id.rig, codex());
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    id.signs_in(&agent, ONE);
    id.look(0);
    for tag in ["one", "two", "three", "four"] {
        id.replace_codex_login(tag);
    }
    rig.agents.accounts.names(&agent, ONE);
    id.look(2);
    id.look(3);
    assert_eq!(id.reads(), 1);
    assert_eq!(verdict(&id, &agent), Verdict::Same);
}

#[test]
fn a_check_a_sign_out_overtakes_leaves_the_sign_out() {
    let id = IdRig::new("identity-overtaken");
    let (rig, agent) = (&id.rig, codex());
    id.signs_in(&agent, ONE);
    let open = rig.agents.accounts.hold(&agent);
    rig.agents.accounts.names(&agent, OTHER);
    let backend = &rig.backend;
    let signs_out = async {
        until("the check runs", || {
            verdict(&id, &agent) == Verdict::Checking
        })
        .await;
        on_state(backend, &agent, &SigninState::SignedOut { found: None }).await;
        open.send(()).unwrap();
    };
    let (ended, ()) = block_on(join(check(backend, &agent), signs_out));
    assert_eq!(ended, Verdict::Idle, "the sign-out's verdict");
    assert!(!rig.host.calls().contains(&stop(&agent)), "nothing stopped");
}

#[test]
fn one_agents_hung_status_holds_up_no_other_agents_check() {
    let id = IdRig::new("identity-parallel");
    let rig = &id.rig;
    id.scratch.file(&claude_store(), b"{}");
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    id.signs_in(&claude(), ONE);
    id.signs_in(&codex(), ONE);
    id.look(0);
    swap(&id, &claude_store(), "claude-swapped");
    id.replace_codex_login("codex-swapped");
    for agent in [claude(), codex()] {
        rig.agents.accounts.names(&agent, ONE);
    }
    // Claude's status hangs (looked at first); Codex's is checked meanwhile.
    let open = rig.agents.accounts.hold(&claude());
    let backend = &rig.backend;
    let watch = async {
        let codex_first = || {
            verdict(&id, &codex()) == Verdict::Same && verdict(&id, &claude()) == Verdict::Checking
        };
        until("Codex is checked while Claude's hangs", codex_first).await;
        open.send(()).unwrap();
    };
    block_on(join(poll_once(backend, id.at(2)), watch));
    assert_eq!(verdict(&id, &claude()), Verdict::Same);
    assert_eq!(id.reads(), 2);
}
