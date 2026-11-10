//! A plan set up after start-up (setup's `set_agent_launch`) joins `limits`: a host started
//! with no agent entry has no model, and the first entry written builds one, reads it and
//! sends the view to the windows already watching. Later inputs reach it, and a second plan
//! joins the first.

use std::time::Duration;

use clauseo_protocol::{AgentId, AgentKind, Freshness, LimitsView, RefreshWhy};
use clauseo_settings::{AgentLaunch, EntryWrite};

use crate::codex::CODEX;
use crate::limits_host::{CLAUDE, Feed, Reads, refresh, start, usage};
use crate::support::{Dir, Running, wait};

/// `agent`'s install as desktop writes it: the adapter under `<data>/agents/<agent>/`.
fn install(dir: &Dir, agent: &str) -> AgentLaunch {
    let adapter = dir
        .data()
        .join("agents")
        .join(agent)
        .join("bin")
        .join("adapter");
    AgentLaunch {
        command: adapter.display().to_string(),
        args: Vec::new(),
        env: std::collections::BTreeMap::new(),
    }
}

/// Writes `agent`'s entry, which must be written, and lets the host take it in.
fn set_up(dir: &Dir, host: &Running, agent: &str) {
    let id = AgentId::new(agent).unwrap();
    let written = wait(
        "set_agent_launch",
        host.handle.set_agent_launch(id, install(dir, agent)),
    );
    assert_eq!(written.unwrap(), EntryWrite::Written);
    host.settle();
}

/// The freshness of each agent in `view`, in the view's order.
fn freshness(view: &LimitsView) -> Vec<(AgentKind, Freshness)> {
    view.0
        .iter()
        .map(|(agent, entry)| (*agent, entry.freshness))
        .collect()
}

#[test]
fn a_plan_set_up_after_start_is_read_and_shown() {
    let dir = Dir::new("limits_plan_added_after_start", &[]);
    let reads = Reads::new();
    let host = start(&dir, &reads);
    let mut feed = Feed::new(&host);
    assert_eq!(feed.view("the view with no plan"), LimitsView::default());

    set_up(&dir, &host, CLAUDE);
    reads.until_count(AgentKind::Claude, 1);
    let view = feed.view("the plan added");
    assert_eq!(freshness(&view), [(AgentKind::Claude, Freshness::Loading)]);
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 30.0, None));
    let (limits, view) = feed.changed("the plan's first read");
    assert_eq!(limits.agent, AgentKind::Claude);
    assert_eq!(freshness(&view), [(AgentKind::Claude, Freshness::Fresh)]);
    host.settle();
    assert_eq!(reads.count(AgentKind::Claude), 1, "one read");

    // The model now takes `RefreshLimits`: a shown once the read is 5 minutes old reads.
    host.clock.advance(Duration::from_secs(300));
    refresh(&host, RefreshWhy::Shown);
    assert_eq!(reads.count(AgentKind::Claude), 2, "the shown read");
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 35.0, None));
    feed.changed("the shown read");

    // Written again, the entry adds nothing and reads nothing.
    set_up(&dir, &host, CLAUDE);
    assert_eq!(
        reads.count(AgentKind::Claude),
        2,
        "no read for the same plan"
    );
    assert_eq!(feed.ready(), [], "no view for the same plan");
    host.shutdown();
}

#[test]
fn a_second_plan_joins_the_first() {
    let dir = Dir::new("limits_plan_added_second", &[]);
    let reads = Reads::new();
    let host = start(&dir, &reads);
    let mut feed = Feed::new(&host);
    feed.view("the view with no plan");
    set_up(&dir, &host, CLAUDE);
    feed.view("Claude added");
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 30.0, None));
    feed.changed("Claude's read");

    set_up(&dir, &host, CODEX);
    reads.until_count(AgentKind::Codex, 1);
    let view = feed.view("Codex added");
    assert_eq!(
        freshness(&view),
        [
            (AgentKind::Claude, Freshness::Fresh),
            (AgentKind::Codex, Freshness::Loading),
        ]
    );
    reads.answer(AgentKind::Codex, usage(AgentKind::Codex, 50.0, None));
    let (_, view) = feed.changed("Codex's read");
    assert_eq!(
        freshness(&view),
        [
            (AgentKind::Claude, Freshness::Fresh),
            (AgentKind::Codex, Freshness::Fresh),
        ]
    );
    assert_eq!(
        reads.count(AgentKind::Claude),
        1,
        "Claude is not read again"
    );
    host.shutdown();
}

#[test]
fn a_plan_set_up_with_nobody_watching_shows_on_the_first_subscription() {
    let dir = Dir::new("limits_plan_added_unwatched", &[]);
    let reads = Reads::new();
    let host = start(&dir, &reads);
    set_up(&dir, &host, CLAUDE);
    reads.until_count(AgentKind::Claude, 1);
    let mut feed = Feed::new(&host);
    let view = feed.view("the first subscription's view");
    assert_eq!(freshness(&view), [(AgentKind::Claude, Freshness::Loading)]);
    host.shutdown();
}
