//! Under `sandbox.required` the account-limits reader never runs outside the jail (decision
//! record 0051): no read starts and the plan-limits line shows it could not read, until R17
//! PR-D/E runs the reader in the jail. With the setting off nothing changes.

use clauseo_protocol::{AgentKind, Freshness, RefreshWhy};

use crate::limits_host::{CLAUDE, Feed, Reads, refresh, start};
use crate::support::{Dir, Scenario};

/// A dir with the Claude agent and `sandbox.required` as `required`.
fn dir(test: &str, required: bool) -> Dir {
    let agents = [(CLAUDE, Scenario::Bundled("claude-limit-warning"))];
    let dir = Dir::new(test, &agents);
    dir.set_settings(
        &agents,
        serde_json::json!({"sandbox": {"required": required}}),
    );
    dir
}

#[test]
fn under_sandbox_required_no_reader_starts_and_the_limits_show_unavailable() {
    let dir = dir("limits_sandbox_required", true);
    let reads = Reads::new();
    let host = start(&dir, &reads);
    host.settle();
    let mut feed = Feed::new(&host);
    let view = feed.view("the current view");
    let claude = view.0.get(&AgentKind::Claude).expect("Claude's entry");
    assert_eq!(claude.freshness, Freshness::Unavailable, "{view:?}");
    // Asked again, from the window and by its button: still no reader.
    refresh(&host, RefreshWhy::Shown);
    refresh(&host, RefreshWhy::TryAgain);
    host.settle();
    assert_eq!(reads.count(AgentKind::Claude), 0, "a reader started");
    host.shutdown();
}

#[test]
fn with_sandbox_required_off_the_reader_starts_as_before() {
    let dir = dir("limits_sandbox_not_required", false);
    let reads = Reads::new();
    let host = start(&dir, &reads);
    reads.until_count(AgentKind::Claude, 1);
    let mut feed = Feed::new(&host);
    let view = feed.view("the current view");
    let claude = view.0.get(&AgentKind::Claude).expect("Claude's entry");
    assert_eq!(claude.freshness, Freshness::Loading, "{view:?}");
    host.shutdown();
}
