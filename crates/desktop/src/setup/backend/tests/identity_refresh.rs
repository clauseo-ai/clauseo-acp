//! R17's refresh-on-check for Codex through the backend (`setup/identity/refresh.rs`): the first
//! check since start forces a refresh, which confirms only when Codex rewrote the login file and
//! a plain read of it names the recorded account, and an unchanged stamp then forces nothing; a
//! rewrite naming another account is a change; an answer with no rewrite is retried once at
//! 30 s and the same again is unknown with the sign-in-again card; a forced read with no
//! answer or a timeout confirms nothing, lets the launch go on its plain answer, and is tried
//! again with the backoff; Codex's refused login refuses; forcing again waits for 60 s after the
//! last force, however the file was written.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use clauseo_agents::{AccountHash, AccountRead, SigninState, Source};
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::{ErrorCode, SignIn};
use futures::future::{Either, join, select};
use serde_json::json;

use super::MAIN;
use super::codex;
use super::fakes::command;
use super::identity::{IdRig, refused, signed_in};
use super::identity_watch::until;
use super::switching::shown;
use super::timing::{Stepping, start};
use crate::bridge::lock;
use crate::setup::identity::{Identities, Verdict, check, expect, on_state, poll_once};
use crate::setup::refusal::wanted;
use crate::test_support::block_on;

const ONE: &str = "lawyer@firm.example";
const OTHER: &str = "someone-else@elsewhere.example";

#[expect(
    clippy::disallowed_methods,
    reason = "test set-up dates and rewrites a dummy login file in its scratch folder"
)]
mod files {
    use std::io::Write as _;
    use std::path::Path;
    use std::time::Duration;

    use super::start;

    /// Sets `path`'s last write to `secs` before the rig's clock.
    pub(super) fn aged(path: &Path, secs: u64) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(start() - Duration::from_secs(secs))
            .unwrap();
    }

    /// Writes `path`'s bytes back over it in place, as Codex's refresh rewrites `auth.json`.
    pub(super) fn rewrite_in_place(path: &Path) {
        let bytes = std::fs::read(path).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .unwrap();
        file.write_all(&bytes).unwrap();
    }
}

fn forced_and_plain(id: &IdRig) -> (usize, usize) {
    id.rig.agents.accounts.forced_and_plain(&codex())
}

fn verdict(id: &IdRig) -> Verdict {
    id.rig.backend.wiring.identity.verdict(&codex())
}

/// Codex's forced reads rewrite its login file in place first, as a refresh that succeeds does.
fn refresh_rewrites(id: &IdRig) {
    let login = id.codex_login();
    let accounts = &id.rig.agents.accounts;
    accounts.on_forced(&codex(), move || files::rewrite_in_place(&login));
}

fn on(identity: Identities) -> Identities {
    identity.with_forced_refresh(true)
}

/// A rig changed by `with`, Codex signed in through Clauseo as [`ONE`] on a login file, then a
/// new login file (the same account's, another file id): due a check, forced (the first).
fn changed_login_with(test: &str, with: impl FnOnce(Identities) -> Identities) -> IdRig {
    let id = IdRig::with(test, with);
    let login = id.scratch.file("homes/codex-acp/auth.json", b"{}");
    files::aged(&login, 120);
    id.signs_in(&codex(), ONE);
    id.look(0);
    id.replace_codex_login("refreshed");
    files::aged(&id.codex_login(), 120);
    id.rig.agents.accounts.names(&codex(), ONE);
    id.rig.host.calls();
    id
}

fn changed_login(test: &str) -> IdRig {
    changed_login_with(test, on)
}

#[test]
fn the_first_check_since_start_confirms_on_a_rewrite_and_an_unchanged_stamp_forces_nothing() {
    let scratch = IdRig::scratch("refresh-first");
    let login = scratch.file("homes/codex-acp/auth.json", b"{}");
    files::aged(&login, 120);
    let id = IdRig::over(scratch, on);
    id.rig.agents.accounts.names(&codex(), ONE);
    refresh_rewrites(&id);
    id.look(0);
    assert_eq!(
        forced_and_plain(&id),
        (1, 1),
        "the startup check forces, then reads the rewritten file plainly"
    );
    assert_eq!(verdict(&id), Verdict::Same);
    // A check with the stamp unchanged reads plain; Clauseo's own rewrite forces nothing.
    block_on(check(&id.rig.backend, &codex()));
    id.look(2);
    id.look(120);
    id.look(600);
    let (forced, _) = forced_and_plain(&id);
    assert_eq!(forced, 1, "the confirmed stamp forces nothing");
    assert_eq!(verdict(&id), Verdict::Same);
}

#[test]
fn a_rewrite_whose_plain_read_names_another_account_is_a_change() {
    let id = changed_login("refresh-other");
    refresh_rewrites(&id);
    // The forced answer still comes from the stored id_token; the rewritten file is another's.
    let one = AccountRead::Account(AccountHash::of(ONE).unwrap());
    id.rig.agents.accounts.forced_answers(&codex(), one);
    id.rig.agents.accounts.names(&codex(), OTHER);
    id.look(2);
    assert_eq!(forced_and_plain(&id), (1, 1));
    assert_eq!(verdict(&id), Verdict::Changed);
}

/// Whether the card says sign in again for refresh-on-check (`signin.unconfirmed`).
fn sign_in_again(id: &IdRig) -> bool {
    matches!(
        shown(&id.rig, &codex()),
        Some(SignIn::Failed {
            code: ErrorCode::SigninUnconfirmed,
            ..
        })
    )
}

#[test]
fn an_answer_with_no_rewrite_is_retried_once_at_thirty_seconds_and_the_same_again_is_unknown() {
    let id = changed_login("refresh-no-rewrite");
    id.look(2);
    assert_eq!(forced_and_plain(&id), (1, 0), "its own answer decides");
    assert_eq!(verdict(&id), Verdict::Same);
    assert_eq!(
        id.rig.host.calls().last(),
        Some(&refused(&codex(), None)),
        "the same account: the launch goes, once"
    );
    id.look(31);
    assert_eq!(forced_and_plain(&id), (1, 0));
    id.look(32);
    assert_eq!(forced_and_plain(&id), (2, 0), "one retry 30 s later");
    assert_eq!(
        verdict(&id),
        Verdict::Unknown,
        "a refresh that fails online"
    );
    assert_eq!(
        id.rig.host.calls().last(),
        Some(&refused(&codex(), Some(ErrorCode::AgentIdentityUnknown)))
    );
    assert!(sign_in_again(&id), "the card says sign in again");
    // Forcing goes on with the backoff.
    id.look(91);
    assert_eq!(forced_and_plain(&id).0, 2);
    id.look(92);
    assert_eq!(forced_and_plain(&id).0, 3);
}

/// A rig whose Codex login reached refresh-on-check's unknown by two forced answers with no
/// rewrite (at 2 s and 32 s): held, the card saying sign in again.
fn held_unknown(test: &str) -> IdRig {
    let id = changed_login(test);
    id.look(2);
    id.look(32);
    assert_eq!(forced_and_plain(&id), (2, 0));
    assert_eq!(verdict(&id), Verdict::Unknown);
    assert!(sign_in_again(&id));
    id
}

/// Whether `agent.identity_unknown` refuses Codex's launches now, its verdict unknown.
fn refused_unknown(id: &IdRig) -> bool {
    let refusal = crate::bridge::lock(&id.rig.backend.wiring.refused)
        .get(&codex())
        .copied();
    refusal == Some(ErrorCode::AgentIdentityUnknown) && verdict(id) == Verdict::Unknown
}

#[test]
fn a_held_unknown_survives_a_touch_of_the_login_and_plain_checks() {
    let id = held_unknown("refresh-held-touch");
    // The session touches auth.json: the look checks, plainly (forced 2 s ago), and the old
    // id_token names the recorded account; it stays unknown.
    files::rewrite_in_place(&id.codex_login());
    id.look(34);
    assert_eq!(forced_and_plain(&id), (2, 1), "a plain check ran");
    assert!(refused_unknown(&id), "launches still refused");
    assert!(sign_in_again(&id), "the card still says sign in again");
    // And a check a launch asks for, as plainly.
    block_on(check(&id.rig.backend, &codex()));
    assert_eq!(forced_and_plain(&id), (2, 2));
    assert_eq!(verdict(&id), Verdict::Unknown);
    files::rewrite_in_place(&id.codex_login());
    id.look(36);
    assert_eq!(verdict(&id), Verdict::Unknown, "again and again");
}

#[test]
fn a_held_unknown_is_lifted_by_a_confirming_forced_read() {
    let id = held_unknown("refresh-held-confirm");
    refresh_rewrites(&id);
    id.look(91);
    assert_eq!(forced_and_plain(&id).0, 2);
    id.look(92);
    assert_eq!(
        forced_and_plain(&id),
        (3, 1),
        "forced, rewritten, its file read"
    );
    assert_eq!(verdict(&id), Verdict::Same);
    assert_eq!(id.rig.host.calls().last(), Some(&refused(&codex(), None)));
    assert!(!sign_in_again(&id));
}

#[test]
fn a_held_unknown_is_lifted_by_a_sign_in_through_clauseo() {
    let id = held_unknown("refresh-held-sign-in");
    id.signs_in(&codex(), ONE);
    assert_eq!(verdict(&id), Verdict::Same);
    assert!(!sign_in_again(&id));
    // A plain check after it is the same account again.
    block_on(check(&id.rig.backend, &codex()));
    assert_eq!(verdict(&id), Verdict::Same);
    assert_eq!(id.rig.host.calls().last(), Some(&refused(&codex(), None)));
}

#[test]
fn codexs_refused_login_stays_unknown_through_a_plain_check() {
    let id = changed_login("refresh-refused-held");
    id.rig
        .agents
        .accounts
        .forced_answers(&codex(), AccountRead::Refused);
    id.look(2);
    assert_eq!(verdict(&id), Verdict::Unknown);
    files::rewrite_in_place(&id.codex_login());
    id.look(4);
    assert_eq!(
        forced_and_plain(&id),
        (1, 1),
        "a plain check, naming the account"
    );
    assert!(refused_unknown(&id));
    assert!(sign_in_again(&id));
}

#[test]
fn an_answer_with_no_rewrite_then_a_rewrite_with_the_same_account_confirms() {
    let id = changed_login("refresh-no-rewrite-then-rewrite");
    id.look(2);
    assert_eq!(forced_and_plain(&id), (1, 0));
    refresh_rewrites(&id);
    id.look(32);
    assert_eq!(
        forced_and_plain(&id),
        (2, 1),
        "the retry rewrote: its file read"
    );
    assert_eq!(verdict(&id), Verdict::Same);
    assert!(!sign_in_again(&id));
    id.look(40);
    id.look(600);
    assert_eq!(forced_and_plain(&id).0, 2, "confirmed: nothing more forced");
}

#[test]
fn an_answer_with_no_rewrite_then_no_answer_keeps_the_allowance() {
    let id = changed_login("refresh-no-rewrite-then-none");
    id.look(2);
    id.rig
        .agents
        .accounts
        .forced_answers(&codex(), AccountRead::Failed);
    id.look(32);
    assert_eq!(forced_and_plain(&id), (2, 1), "the plain fallback decides");
    assert_eq!(verdict(&id), Verdict::Same);
    assert_eq!(
        id.rig.host.calls().last(),
        Some(&refused(&codex(), None)),
        "no answer is not a refusal"
    );
    // Another account in an answer with no rewrite is still a change.
    let other = AccountRead::Account(AccountHash::of(OTHER).unwrap());
    id.rig.agents.accounts.forced_answers(&codex(), other);
    id.look(600);
    assert_eq!(verdict(&id), Verdict::Changed);
}

#[test]
fn a_forced_read_with_no_answer_falls_back_and_is_tried_again_with_the_backoff() {
    // No answer, or any JSON-RPC error but Codex's refused login (S18 reads both `Failed`).
    let id = changed_login("refresh-no-answer");
    id.rig
        .agents
        .accounts
        .forced_answers(&codex(), AccountRead::Failed);
    id.look(2);
    assert_eq!(forced_and_plain(&id), (1, 1), "the plain read decides");
    assert_eq!(verdict(&id), Verdict::Same);
    assert_eq!(
        id.rig.host.calls().last(),
        Some(&refused(&codex(), None)),
        "the launch goes: a Wi-Fi blip is not a mismatch"
    );
    // Missed at 2 s: 30 s backoff, but 60 s after the last force.
    id.look(30);
    assert_eq!(forced_and_plain(&id), (1, 1));
    id.look(62);
    assert_eq!(
        forced_and_plain(&id),
        (2, 2),
        "tried again, falling back again"
    );
    // Two misses: 1 min; three: 2 min.
    id.look(121);
    assert_eq!(forced_and_plain(&id), (2, 2));
    id.look(122);
    assert_eq!(
        forced_and_plain(&id),
        (3, 3),
        "a third try after two misses"
    );
    id.look(241);
    assert_eq!(forced_and_plain(&id), (3, 3));
    id.look(242);
    assert_eq!(forced_and_plain(&id), (4, 4));
    assert_eq!(verdict(&id), Verdict::Same);
}

#[test]
fn a_hung_forced_read_is_dropped_at_its_share_and_the_fallback_ends_within_the_bound() {
    let bound = Duration::from_millis(400);
    let id = changed_login_with("refresh-hung", |mut identity| {
        identity.bound = bound;
        on(identity)
    });
    id.rig.agents.accounts.hang_forced(&codex());
    // Two bounds back to back would take 800 ms; one, with slack for a loaded machine, ends
    // first.
    let look = std::pin::pin!(poll_once(&id.rig.backend, id.at(2)));
    let late = std::pin::pin!(AsyncIoTimer.after(bound + Duration::from_millis(350)));
    let ended = block_on(select(look, late));
    assert!(
        matches!(ended, Either::Left(_)),
        "one bound for both reads, not two"
    );
    assert_eq!(forced_and_plain(&id), (1, 1), "the fallback ran");
    assert_eq!(
        id.rig.agents.accounts.dropped.load(Ordering::SeqCst),
        1,
        "the hung read was let go: S18 closes its app-server's stdin, never kills it"
    );
    assert_eq!(verdict(&id), Verdict::Same, "the launch goes");
    id.look(62);
    assert_eq!(forced_and_plain(&id), (2, 2), "unconfirmed: forced again");
}

#[test]
fn codexs_refused_login_is_unknown_and_tried_again_with_the_backoff() {
    let id = changed_login("refresh-refused");
    id.rig
        .agents
        .accounts
        .forced_answers(&codex(), AccountRead::Refused);
    id.look(2);
    assert_eq!(
        forced_and_plain(&id),
        (1, 0),
        "no fallback on a definite answer"
    );
    assert_eq!(verdict(&id), Verdict::Unknown);
    assert_eq!(
        id.rig.host.calls().last(),
        Some(&refused(&codex(), Some(ErrorCode::AgentIdentityUnknown)))
    );
    assert!(sign_in_again(&id), "the card says sign in again");
    id.look(62);
    assert_eq!(forced_and_plain(&id), (2, 0));
}

#[test]
fn a_write_after_the_reply_is_not_taken_for_the_rewrite_and_is_forced_again() {
    // The files' clock behind every real write: each rewrite lands after the reply.
    let id = changed_login_with("refresh-late-write", |mut identity| {
        identity.refresh.files_clock = Arc::new(Stepping::default());
        on(identity)
    });
    refresh_rewrites(&id);
    id.look(2);
    assert_eq!(
        forced_and_plain(&id),
        (1, 0),
        "no rewrite of its own: nothing read again"
    );
    assert_eq!(verdict(&id), Verdict::Same, "the launch goes on its answer");
    id.look(4);
    id.look(31);
    assert_eq!(forced_and_plain(&id).0, 1);
    id.look(32);
    assert_eq!(forced_and_plain(&id).0, 2, "no rewrite of its own: retried");
}

#[test]
fn a_change_ten_seconds_after_our_last_force_is_forced_sixty_seconds_after_it() {
    let id = changed_login("refresh-rate");
    refresh_rewrites(&id);
    id.look(2);
    assert_eq!(forced_and_plain(&id), (1, 1));
    // Settles the look after Clauseo's own rewrite (a plain check of the confirmed stamp).
    id.look(3);
    let (_, plain) = forced_and_plain(&id);
    id.replace_codex_login("again");
    id.look(12);
    assert_eq!(
        forced_and_plain(&id),
        (1, plain + 1),
        "pending, read plain inside the minute"
    );
    assert_eq!(verdict(&id), Verdict::Same);
    id.look(61);
    assert_eq!(forced_and_plain(&id).0, 1);
    id.look(62);
    assert_eq!(forced_and_plain(&id).0, 2, "60 s after the last force");
}

#[test]
fn a_sign_in_during_a_check_leaves_no_held_unknown_after_it() {
    let id = changed_login("refresh-overtaken");
    let (backend, accounts) = (&id.rig.backend, &id.rig.agents.accounts);
    let open = accounts.hold(&codex());
    // Codex's refused login: an answer that would hold the unknown, had nothing overtaken it.
    accounts.answers(&codex(), AccountRead::Refused);
    let signs_in = async {
        until("the check runs", || verdict(&id) == Verdict::Checking).await;
        expect(backend, &codex());
        on_state(backend, &codex(), &signed_in(ONE, Source::Own)).await;
        open.send(()).unwrap();
    };
    block_on(join(check(backend, &codex()), signs_in));
    assert!(
        !backend.wiring.identity.refresh.stuck(&codex()),
        "the overtaken check holds nothing"
    );
    assert!(!sign_in_again(&id), "no sign-in-again card");
    assert_ne!(verdict(&id), Verdict::Unknown);
    assert_eq!(wanted(backend, &codex()), None, "launches go");
}

#[test]
fn a_reused_login_held_unknown_offers_a_different_account_whose_sign_in_clears_it() {
    let id = changed_login("refresh-reused-unknown");
    let rig = &id.rig;
    // The login is the lawyer's own Codex login, reused; then Codex refuses its refresh.
    super::leave::reuse(rig, &codex());
    rig.agents.accounts.answers(&codex(), AccountRead::Refused);
    id.look(2);
    assert_eq!(verdict(&id), Verdict::Unknown);
    assert!(
        matches!(
            shown(rig, &codex()),
            Some(SignIn::Failed {
                code: ErrorCode::SigninUnconfirmed,
                reused: true,
                ..
            })
        ),
        "the card says the login is reused: its way out is a different account"
    );
    // A plain sign-in over the reused login is refused (review M1), so it is no way out.
    let sent = command("start_sign_in", &codex(), &json!({}));
    let refused_start = rig.pumped(async { rig.setup().run(MAIN, sent).await });
    assert!(refused_start.is_err(), "no sign-in over the lawyer's login");
    // "Use a different account": the reuse is left first, then Clauseo's own sign-in, which
    // records and lifts the unknown.
    let out = SigninState::SignedOut { found: None };
    lock(&rig.agents.answers).insert("stop_using_login", out);
    let own = signed_in(ONE, Source::Own);
    lock(&rig.agents.answers).insert("use_different_account", own);
    rig.agents.accounts.names(&codex(), ONE);
    let sent = command("use_different_account", &codex(), &json!({}));
    rig.pumped(async { rig.setup().run(MAIN, sent).await.unwrap() });
    assert!(!rig.backend.wiring.identity.refresh.stuck(&codex()));
    assert!(!sign_in_again(&id), "the card is gone");
    assert_eq!(wanted(&rig.backend, &codex()), None, "launches go");
}
