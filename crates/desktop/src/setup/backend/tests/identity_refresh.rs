//! R17's refresh-on-check for Codex through the backend (`setup/identity/refresh.rs`): the first
//! check since start forces a refresh, which confirms only when Codex rewrote the login file and
//! a plain read of it names the recorded account, and an unchanged stamp then forces nothing; a
//! rewrite naming another account is a change; a forced read that rewrites nothing, has no
//! answer or times out confirms nothing, lets the launch go on its plain answer, and is tried
//! again with the backoff; Codex's refused login refuses; forcing again waits for 60 s after the
//! last force, however the file was written.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use clauseo_agents::{AccountHash, AccountRead};
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::ErrorCode;
use futures::future::{Either, select};

use super::codex;
use super::identity::{IdRig, refused};
use super::timing::{Stepping, start};
use crate::setup::identity::{Identities, Verdict, check, poll_once};
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

#[test]
fn a_forced_read_that_rewrites_nothing_confirms_nothing_lets_the_launch_go_and_backs_off() {
    let id = changed_login("refresh-no-rewrite");
    id.look(2);
    assert_eq!(forced_and_plain(&id), (1, 0), "its own answer decides");
    assert_eq!(verdict(&id), Verdict::Same);
    assert_eq!(
        id.rig.host.calls().last(),
        Some(&refused(&codex(), None)),
        "the same account: the launch goes"
    );
    id.look(61);
    assert_eq!(forced_and_plain(&id), (1, 0), "60 s after the last force");
    id.look(62);
    assert_eq!(forced_and_plain(&id), (2, 0), "still pending: forced again");
    // Another account in a forced answer with no rewrite is still a change.
    let other = AccountRead::Account(AccountHash::of(OTHER).unwrap());
    id.rig.agents.accounts.forced_answers(&codex(), other);
    id.look(500);
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
        "the hung read was dropped, which kills its app-server"
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
    id.look(61);
    assert_eq!(forced_and_plain(&id).0, 1);
    id.look(62);
    assert_eq!(forced_and_plain(&id).0, 2, "still pending: forced again");
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
