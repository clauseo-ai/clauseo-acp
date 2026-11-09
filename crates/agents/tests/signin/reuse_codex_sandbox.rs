//! The Codex login reuse ends with `CodexSandbox` (R17 PR-D2a): a jailed launch met the reused
//! login, so reuse stops with that reason, `no-reuse` is written, and the lawyer's file stays. The
//! helpers are `reuse_codex`'s.

use clauseo_agents::Found;
use clauseo_agents::testing::write_file;

use super::reuse_codex::{home_file, lawyer_file, logouts, reuse, signed_out, start};
use crate::support::{Fixture, codex};

/// A jailed Codex launch met the reused login (`CodexSandbox`): reuse ends with that reason and
/// writes `no-reuse`, so detection does not offer the same login again. The home's link goes;
/// the lawyer's file stays with its content, and no logout runs.
#[test]
fn a_codex_sandbox_leave_ends_reuse_and_writes_no_reuse() {
    let fx = Fixture::new("reuse-codex-sandbox", "ok-after:0");
    write_file(&lawyer_file(&fx), b"lawyer-login\n").unwrap();
    let (agents, events) = start(&fx);
    reuse(&agents, &events);
    agents
        .stop_using_login(&codex(), Some(Found::CodexSandbox))
        .unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::CodexSandbox)));
    assert!(fx.marker(&codex(), "no-reuse").is_file());
    assert!(!fx.marker(&codex(), "login-source").exists());
    assert!(!home_file(&fx).exists());
    assert_eq!(std::fs::read(lawyer_file(&fx)).unwrap(), b"lawyer-login\n");
    assert_eq!(logouts(&fx), []);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReuseOff)));
    assert_eq!(fx.codex_runs(&["login"]), []);
}
