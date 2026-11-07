//! The lawyer's notice of a blocked site (decision record 0053) through the host: what the egress
//! proxy's sink reports with `HostHandle::site_blocked` reaches the app feed as `SiteBlocked`,
//! attributed to the running turn, once per site per turn with a count, and saying whether the
//! lawyer may allow the site (`network.allow_unsafe_site_override`).

use clauseo_blocklist::{Generated, Tag};
use clauseo_host::{AppSubscription, BlockedSite, SessionKey};
use clauseo_protocol::{
    BlockedSiteId, CommandOutcome, HostCommand, HostEvent, MatterId, Principal, SiteTag,
    StopReason, StreamId, ThreadId, TurnId,
};

use crate::permissions::{PERM, until_first_request};
use crate::settings::ready;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};

/// The fields of one notice the tests compare.
#[derive(Debug, PartialEq, Eq)]
struct Notice {
    block_id: BlockedSiteId,
    turn: Option<TurnId>,
    host: String,
    count: u32,
}

fn site(host: &str, tag: Tag) -> BlockedSite {
    BlockedSite {
        host: host.to_owned(),
        tag,
        generated: Generated::parse("2026-11-07T06:00:00Z").unwrap(),
    }
}

fn subscribe(host: &Running) -> AppSubscription {
    wait(
        "subscribe_app",
        host.handle.subscribe_app(Principal::LocalDesktop),
    )
    .unwrap()
}

/// The `SiteBlocked` events published so far, each checked against `matter`, `thread` and
/// `override_allowed`.
fn notices(
    host: &Running,
    sub: &mut AppSubscription,
    (matter, thread): (MatterId, ThreadId),
    override_allowed: bool,
) -> Vec<Notice> {
    host.settle();
    ready(sub)
        .into_iter()
        .filter_map(|event| match event {
            HostEvent::SiteBlocked {
                block_id,
                matter: m,
                thread: t,
                turn,
                host,
                tag,
                list_generated,
                override_allowed: allowed,
                count,
            } => {
                assert_eq!((m, t), (matter, Some(thread)));
                assert_eq!(allowed, override_allowed);
                assert_eq!(list_generated.as_millis(), 1_794_031_200_000);
                let want = if host == "1.2.3.4" {
                    SiteTag::Malware
                } else {
                    SiteTag::Phishing
                };
                assert_eq!(tag, want, "{host}");
                Some(Notice {
                    block_id,
                    turn,
                    host,
                    count,
                })
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_blocked_site_is_noticed_once_per_turn_with_a_count() {
    let dir = Dir::new(
        "site_blocked_per_turn",
        &[(PERM, Scenario::Bundled("permission-kinds"))],
    );
    let host = dir.start(0);
    let mut sub = subscribe(&host);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], PERM);
    let session = SessionKey {
        matter,
        thread,
        launch: 1,
    };
    let key = (matter, thread);
    // Before any turn: noticed with no turn.
    host.handle
        .site_blocked(session, site("evil.example", Tag::Phishing));
    let before = notices(&host, &mut sub, key, true);
    assert_eq!(before.len(), 1, "{before:?}");
    assert_eq!((before[0].turn, before[0].count), (None, 1));

    // A turn starts fresh: a new id, counted within the turn.
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Edit the four clauses.");
    until_first_request(&mut events);
    host.handle
        .site_blocked(session, site("evil.example", Tag::Phishing));
    host.handle
        .site_blocked(session, site("evil.example", Tag::Phishing));
    host.handle
        .site_blocked(session, site("1.2.3.4", Tag::Malware));
    let seen = notices(&host, &mut sub, key, true);
    assert_eq!(seen.len(), 3, "{seen:?}");
    let evil = seen[0].block_id;
    assert_ne!(evil, before[0].block_id);
    assert_eq!(
        (&seen[0].host, seen[0].turn, seen[0].count),
        (&"evil.example".to_owned(), Some(first), 1)
    );
    assert_eq!(
        seen[1],
        Notice {
            block_id: evil,
            turn: Some(first),
            host: "evil.example".to_owned(),
            count: 2,
        }
    );
    assert_ne!(seen[2].block_id, evil);
    assert_eq!((seen[2].turn, seen[2].count), (Some(first), 1));

    // The turn ended: no turn running, still noticed, under a new id.
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    host.handle
        .site_blocked(session, site("evil.example", Tag::Phishing));
    let idle = notices(&host, &mut sub, key, true);
    assert_eq!(idle.len(), 1, "{idle:?}");
    assert_eq!((idle[0].turn, idle[0].count), (None, 1));
    assert!(idle[0].block_id != evil && idle[0].block_id != before[0].block_id);
    host.shutdown();
}

#[test]
fn override_allowed_follows_the_setting() {
    let site_in = |dir: &Dir, host: Running, allowed: bool| {
        let mut sub = subscribe(&host);
        let (matter, thread, _) = matter_with_thread(dir, &host, "m", &[], FAKE);
        let session = SessionKey {
            matter,
            thread,
            launch: 1,
        };
        host.handle
            .site_blocked(session, site("phish.test", Tag::Phishing));
        let seen = notices(&host, &mut sub, (matter, thread), allowed);
        assert_eq!(seen.len(), 1, "{seen:?}");
        host.shutdown();
    };
    let agents = [(FAKE, Scenario::Bundled("prompt-stream"))];
    let dir = Dir::new("site_blocked_override_default", &agents);
    site_in(&dir, dir.start(0), true);
    let dir = Dir::new("site_blocked_override_policy", &agents);
    let policy = r#"{"network": {"allow_unsafe_site_override": false}}"#;
    site_in(&dir, dir.start_with_policy(0, policy), false);
    let dir = Dir::new("site_blocked_override_lawyer", &agents);
    dir.set_settings(
        &agents,
        serde_json::json!({"network": {"allow_unsafe_site_override": false}}),
    );
    site_in(&dir, dir.start(0), false);
}
