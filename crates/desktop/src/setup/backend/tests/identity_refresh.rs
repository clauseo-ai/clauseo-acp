//! R17's refresh-on-check for Codex through the backend (`setup/identity/refresh.rs`): the first
//! check since start forces a refresh and an unchanged stamp does not; a login written under a
//! minute ago reads plain and is forced once at sixty seconds, and Clauseo's own rewrite then
//! forces nothing; a forced read with no answer falls back to the plain read, lets the launch
//! go, and is retried once; a refused refresh or another account refuses.

use clauseo_agents::{AccountHash, AccountRead};
use clauseo_protocol::ErrorCode;
use futures::future::join;

use super::codex;
use super::identity::{IdRig, refused};
use super::timing::start;
use crate::setup::identity::{Verdict, check, poll_once};
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

/// A rig with refresh-on-check on, Codex signed in through Clauseo as [`ONE`] on a login file
/// last written `age` seconds before the rig's clock, then a new login file (the same
/// account's, another file id) last written `age` seconds before it too: due a check.
fn changed_login(test: &str, age: u64) -> IdRig {
    let id = IdRig::with(test, |identity| identity.with_forced_refresh(true));
    let login = id.scratch.file("homes/codex-acp/auth.json", b"{}");
    files::aged(&login, age);
    id.signs_in(&codex(), ONE);
    id.look(0);
    id.replace_codex_login("refreshed");
    files::aged(&id.codex_login(), age);
    id.rig.agents.accounts.names(&codex(), ONE);
    id.rig.host.calls();
    id
}

#[test]
fn the_first_check_since_start_forces_a_refresh_and_an_unchanged_stamp_does_not() {
    let scratch = IdRig::scratch("refresh-first");
    let login = scratch.file("homes/codex-acp/auth.json", b"{}");
    files::aged(&login, 120);
    let id = IdRig::over(scratch, |identity| identity.with_forced_refresh(true));
    id.rig.agents.accounts.names(&codex(), ONE);
    id.look(0);
    assert_eq!(forced_and_plain(&id), (1, 0), "the startup check forces");
    assert_eq!(
        id.rig.backend.wiring.identity.verdict(&codex()),
        Verdict::Same
    );
    // A check with the stamp unchanged reads plain; the looks read nothing.
    block_on(check(&id.rig.backend, &codex()));
    assert_eq!(
        forced_and_plain(&id),
        (1, 1),
        "the confirmed stamp forces nothing"
    );
    id.look(2);
    id.look(120);
    assert_eq!(forced_and_plain(&id), (1, 1));
}

#[test]
fn a_changed_stamp_forces_a_refresh() {
    let id = changed_login("refresh-changed", 120);
    id.look(2);
    assert_eq!(forced_and_plain(&id), (1, 0));
    assert_eq!(
        id.rig.backend.wiring.identity.verdict(&codex()),
        Verdict::Same
    );
    assert_eq!(id.rig.host.calls().last(), Some(&refused(&codex(), None)));
}

#[test]
fn a_login_written_under_a_minute_ago_reads_plain_then_is_forced_once_and_our_rewrite_is_not() {
    let id = changed_login("refresh-guard", 10);
    id.look(2);
    assert_eq!(
        forced_and_plain(&id),
        (0, 1),
        "read plain now; its answer counts"
    );
    assert_eq!(
        id.rig.backend.wiring.identity.verdict(&codex()),
        Verdict::Same
    );
    id.look(30);
    assert_eq!(
        forced_and_plain(&id),
        (0, 1),
        "not due yet: last write plus 60 s"
    );
    // The forced re-check at last write plus 60 s; Codex's refresh rewrites the file in place
    // while it runs.
    let open = id.rig.agents.accounts.hold(&codex());
    let refresh = async {
        files::rewrite_in_place(&id.codex_login());
        open.send(()).unwrap();
    };
    block_on(join(poll_once(&id.rig.backend, id.at(50)), refresh));
    assert_eq!(forced_and_plain(&id), (1, 1), "one forced re-check");
    assert_eq!(
        id.rig.backend.wiring.identity.verdict(&codex()),
        Verdict::Same
    );
    // The rewrite is a change the next look checks, but plainly: it is the confirmed stamp.
    id.look(52);
    assert_eq!(
        forced_and_plain(&id),
        (1, 2),
        "our own rewrite forces nothing"
    );
    id.look(200);
    assert_eq!(forced_and_plain(&id), (1, 2), "nothing more is due");
}

#[test]
fn a_forced_refresh_with_no_answer_falls_back_to_the_plain_read_and_is_retried_once() {
    let id = changed_login("refresh-no-answer", 120);
    id.rig
        .agents
        .accounts
        .forced_answers(&codex(), AccountRead::Failed);
    id.look(2);
    assert_eq!(forced_and_plain(&id), (1, 1), "the plain read decides");
    assert_eq!(
        id.rig.backend.wiring.identity.verdict(&codex()),
        Verdict::Same
    );
    assert_eq!(
        id.rig.host.calls().last(),
        Some(&refused(&codex(), None)),
        "the launch goes: a Wi-Fi blip is not a mismatch"
    );
    id.look(30);
    assert_eq!(forced_and_plain(&id), (1, 1));
    id.look(62);
    assert_eq!(
        forced_and_plain(&id),
        (2, 2),
        "one retry, falling back again"
    );
    id.look(200);
    id.look(400);
    assert_eq!(forced_and_plain(&id), (2, 2), "no second retry");
    assert_eq!(
        id.rig.backend.wiring.identity.verdict(&codex()),
        Verdict::Same
    );
}

#[test]
fn a_refused_refresh_or_another_account_refuses_the_launch() {
    // A refresh refused (an auth error, refresh_token_reused): S18 answers no account.
    let id = changed_login("refresh-refused", 120);
    id.rig
        .agents
        .accounts
        .forced_answers(&codex(), AccountRead::NoAccount);
    id.look(2);
    assert_eq!(
        forced_and_plain(&id),
        (1, 0),
        "no plain fallback on a definite answer"
    );
    assert_eq!(
        id.rig.backend.wiring.identity.verdict(&codex()),
        Verdict::Unknown
    );
    assert_eq!(
        id.rig.host.calls().last(),
        Some(&refused(&codex(), Some(ErrorCode::AgentIdentityUnknown)))
    );
    // The refresh names another account: the token-only swap, caught.
    let id = changed_login("refresh-other", 120);
    let other = AccountRead::Account(AccountHash::of(OTHER).unwrap());
    id.rig.agents.accounts.forced_answers(&codex(), other);
    id.look(2);
    assert_eq!(forced_and_plain(&id), (1, 0));
    assert_eq!(
        id.rig.backend.wiring.identity.verdict(&codex()),
        Verdict::Changed
    );
}
