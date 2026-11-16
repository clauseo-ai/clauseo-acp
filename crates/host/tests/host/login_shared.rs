//! The hook's second call, `LoginCheck::shared` (decision record 0048, P4d): whether the
//! Codex home's `auth.json` is the lawyer's file, whatever the link check says. A symbolic link
//! is a split to `check` and shared to `shared`, and the `/login` guard follows `shared`;
//! `Unknown` counts as shared.

use std::sync::Arc;

use clauseo_host::{LoginLink, Shared};
use clauseo_protocol::{ErrorCode, StopReason, StreamId};

use crate::codex::{CODEX, codex_dir, run_turn};
use crate::login_check::Fake;
use crate::support::{matter_with_thread, send_prompt};

/// A Codex launch whose check answers `Split` and `shared`, the login not reused (so the split
/// check refuses nothing), then a typed `/login`: refused with `signin.login_shared`.
fn guard_stays_on(test: &str, shared: Shared) {
    let dir = codex_dir(test, "codex-handshake");
    let fake = Fake::new(LoginLink::Split);
    fake.set_shared(shared);
    let hook = Arc::clone(&fake);
    let host = dir.start_with(0, |config| config.with_login_check(hook));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(fake.asked().len(), 1, "one launch, one check");
    host.refused(send_prompt(thread, "/login"), ErrorCode::SigninLoginShared);
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn a_symlinked_auth_json_is_a_split_to_check_and_shared_to_shared_so_the_guard_stays_on() {
    guard_stays_on("login_shared_symlink", Shared::Yes);
}

#[test]
fn a_shared_answer_of_unknown_counts_as_shared() {
    guard_stays_on("login_shared_unknown", Shared::Unknown);
}
