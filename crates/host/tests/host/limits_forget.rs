//! The host carries out `limits`' `Forget`: a sign-out deletes the agent's stored limits, and a
//! read that names another account deletes every entry but that account's, so the next start
//! shows neither an account that signed out nor one that was replaced. Another agent's entry
//! stays.

use clauseo_limits::ReadOutcome;
use clauseo_protocol::{AccountLimits, AccountRef, AgentKind, Freshness, LimitSource};
use clauseo_store::{Store, StoreOptions};

use crate::limits_host::{CLAUDE, Feed, Reads, at, entry, plan, seed, start, usage};
use crate::support::{Dir, Scenario, wait};

/// Claude's plan, checked an hour before the start, kept in `dir`'s store; and Codex's, which
/// no settings entry reads.
fn seed_both(dir: &Dir) -> AccountLimits {
    let mut claude = plan(AgentKind::Claude, 40.0, Some(at(7200)));
    claude.source = LimitSource::Fresh {
        checked_at: at(-3600),
    };
    seed(dir, claude);
    let codex = plan(AgentKind::Codex, 10.0, None);
    seed(dir, codex.clone());
    codex
}

/// Every entry in `dir`'s store, by agent, once the host is shut down.
fn stored(dir: &Dir) -> Vec<AccountLimits> {
    let data = dir.data().to_owned();
    let mut stored = wait("reading the store", async move {
        let store = Store::open(StoreOptions::new(data)).await.unwrap();
        let stored = store.account_limits().await.unwrap();
        store.close().await.unwrap();
        stored
    });
    stored.sort_by_key(|limits| limits.agent);
    stored
}

/// A host on `dir` that shows Claude's stored plan, stale, before its start-up read ends.
fn start_stale(dir: &Dir, reads: &Reads) -> (crate::support::Running, Feed) {
    let host = start(dir, reads);
    let mut feed = Feed::new(&host);
    let view = feed.view("the current view");
    assert_eq!(entry(&view, AgentKind::Claude).freshness, Freshness::Stale);
    (host, feed)
}

#[test]
fn a_sign_out_deletes_the_agents_stored_limits_and_keeps_the_other_agents() {
    let dir = Dir::new(
        "limits_forget_sign_out",
        &[(CLAUDE, Scenario::Bundled("claude-prompt-stream"))],
    );
    let codex = seed_both(&dir);
    let reads = Reads::new();
    let (host, mut feed) = start_stale(&dir, &reads);

    reads.answer(AgentKind::Claude, ReadOutcome::SignedOut);
    let view = feed.view("the signed-out view");
    let claude = entry(&view, AgentKind::Claude);
    assert_eq!(claude.freshness, Freshness::MissingScope, "{view:?}");
    assert_eq!(claude.plan, None);
    host.settle();
    assert_eq!(
        feed.ready(),
        [],
        "a Forget publishes no AccountLimitsChanged"
    );
    host.shutdown();
    assert_eq!(stored(&dir), [codex]);
}

#[test]
fn a_read_of_another_account_keeps_only_that_accounts_stored_limits() {
    let dir = Dir::new(
        "limits_forget_other_account",
        &[(CLAUDE, Scenario::Bundled("claude-prompt-stream"))],
    );
    let codex = seed_both(&dir);
    let reads = Reads::new();
    let (host, mut feed) = start_stale(&dir, &reads);

    let ReadOutcome::Usage {
        mut limits,
        reached,
        account_read,
    } = usage(AgentKind::Claude, 5.0, None)
    else {
        unreachable!("usage is a Usage")
    };
    limits.account = AccountRef("partner@example.com".to_owned());
    reads.answer(
        AgentKind::Claude,
        ReadOutcome::Usage {
            limits,
            reached,
            account_read,
        },
    );
    let (changed, _) = feed.changed("the other account's read");
    host.settle();
    host.shutdown();
    assert_eq!(stored(&dir), [changed, codex]);
}
