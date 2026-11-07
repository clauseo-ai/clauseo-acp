//! The lawyer's "Allow this site" (decision record 0053, PR 2) through the host: an allow names
//! a notice the host sent and covers exactly its host, for its matter or always; it reaches the
//! launch's live view at once, is stored outside the settings and survives a restart; a revoke
//! takes it back; and while `network.allow_unsafe_site_override` is off no allow is made or
//! honoured, though none is deleted.

use std::sync::Arc;

use clauseo_blocklist::{Allowed, Current, Generated, Tag};
use clauseo_host::{AllowedSites, BlockListSeams, BlockListUpdate, BlockedSite, SessionKey};
use clauseo_protocol::{
    AllowScope, AllowedSite, BlockedSiteId, CommandOutcome, ErrorCode, HostCommand, HostEvent,
    HostQuery, MatterId, Principal, QueryResult, ThreadId,
};

use crate::settings::ready;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};

const AGENTS: &[(&str, Scenario)] = &[(FAKE, Scenario::Bundled("prompt-stream"))];

/// Starts run `run` with block list seams that fetch nothing (no host is allowed to fetch
/// from) and `sites` as the allowed sites' views.
fn start(dir: &Dir, run: u16, sites: &Arc<AllowedSites>) -> Running {
    let update = BlockListUpdate {
        allowed_hosts: Vec::new(),
        ..BlockListUpdate::release()
    };
    let seams = BlockListSeams::new(Arc::new(Current::default()), None, update)
        .with_allowed(Arc::clone(sites));
    dir.start_with(run, |config| seams.apply(config))
}

/// Reports `host` blocked in `(matter, thread)` and returns the notice's id.
fn notice(host: &Running, (matter, thread): (MatterId, ThreadId), site: &str) -> BlockedSiteId {
    let mut sub = wait(
        "subscribe_app",
        host.handle.subscribe_app(Principal::LocalDesktop),
    )
    .unwrap();
    host.settle();
    drop(ready(&mut sub));
    let session = SessionKey {
        matter,
        thread,
        launch: 1,
    };
    host.handle.site_blocked(
        session,
        BlockedSite {
            host: site.to_owned(),
            tag: Tag::Phishing,
            generated: Generated::parse("2026-11-07T06:00:00Z").unwrap(),
        },
    );
    host.settle();
    ready(&mut sub)
        .into_iter()
        .find_map(|event| match event {
            HostEvent::SiteBlocked { block_id, host, .. } if host == site => Some(block_id),
            _ => None,
        })
        .expect("a notice")
}

fn allow(block_id: BlockedSiteId, scope: AllowScope) -> HostCommand {
    HostCommand::AllowBlockedSite { block_id, scope }
}

fn listed(host: &Running) -> Vec<(String, AllowScope, Option<MatterId>)> {
    match host.query(HostQuery::AllowedSites).unwrap() {
        QueryResult::AllowedSites(sites) => sites
            .into_iter()
            .map(
                |AllowedSite {
                     host,
                     scope,
                     matter,
                     ..
                 }| (host, scope, matter),
            )
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn hosts(view: &Allowed) -> Vec<String> {
    view.get().iter().cloned().collect()
}

#[test]
fn an_allow_reaches_its_matters_view_at_once_and_survives_a_restart() {
    let dir = Dir::new("allowed_sites_matter", AGENTS);
    let sites = Arc::new(AllowedSites::default());
    let host = start(&dir, 0, &sites);
    let (a, a_thread, _) = matter_with_thread(&dir, &host, "a", &[], FAKE);
    let (b, b_thread, _) = matter_with_thread(&dir, &host, "b", &[], FAKE);
    // A launch's view, taken before the allow: it follows every change.
    let a_view = sites.for_matter(a);
    let b_view = sites.for_matter(b);
    let evil = notice(&host, (a, a_thread), "evil.example");
    assert_eq!(
        host.ok(allow(evil, AllowScope::Matter)),
        CommandOutcome::Done
    );
    assert_eq!(hosts(&a_view), ["evil.example"]);
    assert_eq!(hosts(&b_view), Vec::<String>::new());
    // Allowing it again changes nothing.
    assert_eq!(
        host.ok(allow(evil, AllowScope::Matter)),
        CommandOutcome::Done
    );
    let phish = notice(&host, (b, b_thread), "phish.test");
    host.ok(allow(phish, AllowScope::Always));
    assert_eq!(hosts(&a_view), ["evil.example", "phish.test"]);
    assert_eq!(hosts(&b_view), ["phish.test"]);
    let want = vec![
        ("phish.test".to_owned(), AllowScope::Always, None),
        ("evil.example".to_owned(), AllowScope::Matter, Some(a)),
    ];
    assert_eq!(listed(&host), want);
    // In the host's own file, never in the settings.
    let stored = std::fs::read_to_string(dir.data().join("blocklist/allowed.json")).unwrap();
    assert!(
        stored.contains("evil.example") && stored.contains("phish.test"),
        "{stored}"
    );
    assert!(!dir.settings_json().contains("evil.example"));
    host.shutdown();

    let sites = Arc::new(AllowedSites::default());
    let host = start(&dir, 1, &sites);
    assert_eq!(listed(&host), want);
    assert_eq!(hosts(&sites.for_matter(a)), ["evil.example", "phish.test"]);
    // A revoke reaches the view at once.
    let a_view = sites.for_matter(a);
    let revoke = HostCommand::RevokeAllowedSite {
        host: "evil.example".to_owned(),
        scope: AllowScope::Matter,
        matter: Some(a),
    };
    assert_eq!(host.ok(revoke.clone()), CommandOutcome::Done);
    assert_eq!(hosts(&a_view), ["phish.test"]);
    // Revoking what is not there does nothing.
    assert_eq!(host.ok(revoke), CommandOutcome::Done);
    host.shutdown();
}

#[test]
fn an_allow_names_a_notice_the_host_sent() {
    let dir = Dir::new("allowed_sites_unknown", AGENTS);
    let sites = Arc::new(AllowedSites::default());
    let host = start(&dir, 0, &sites);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let real = notice(&host, (matter, thread), "evil.example");
    let made_up = BlockedSiteId::new(uuid::Uuid::now_v7()).unwrap();
    host.refused(
        allow(made_up, AllowScope::Always),
        ErrorCode::BlocklistNoticeExpired,
    );
    assert_eq!(listed(&host), []);
    assert_eq!(hosts(&sites.for_matter(matter)), Vec::<String>::new());
    assert!(!dir.data().join("blocklist/allowed.json").exists());
    host.ok(allow(real, AllowScope::Always));
    host.shutdown();
}

#[test]
fn with_overrides_off_nothing_is_allowed_or_honoured_and_nothing_is_deleted() {
    let dir = Dir::new("allowed_sites_policy", AGENTS);
    let sites = Arc::new(AllowedSites::default());
    let host = start(&dir, 0, &sites);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let evil = notice(&host, (matter, thread), "evil.example");
    host.ok(allow(evil, AllowScope::Always));
    host.shutdown();

    // The firm locks the key off: the stored allow is listed, honoured nowhere, and kept.
    dir.set_policy(r#"{"network": {"allow_unsafe_site_override": false}}"#);
    let sites = Arc::new(AllowedSites::default());
    let host = start(&dir, 1, &sites);
    let view = sites.for_matter(matter);
    assert_eq!(hosts(&view), Vec::<String>::new());
    let phish = notice(&host, (matter, thread), "phish.test");
    host.refused(
        allow(phish, AllowScope::Matter),
        ErrorCode::BlocklistOverrideOff,
    );
    assert_eq!(
        listed(&host),
        [("evil.example".to_owned(), AllowScope::Always, None)]
    );
    assert_eq!(hosts(&view), Vec::<String>::new());
    host.shutdown();

    // Unlocked: the kept allow is honoured again, and revoking still works.
    dir.set_policy("{}");
    let sites = Arc::new(AllowedSites::default());
    let host = start(&dir, 2, &sites);
    let view = sites.for_matter(matter);
    assert_eq!(hosts(&view), ["evil.example"]);
    dir.set_policy(r#"{"network": {"allow_unsafe_site_override": false}}"#);
    host.shutdown();
    let sites = Arc::new(AllowedSites::default());
    let host = start(&dir, 3, &sites);
    let revoke = HostCommand::RevokeAllowedSite {
        host: "evil.example".to_owned(),
        scope: AllowScope::Always,
        matter: None,
    };
    assert_eq!(host.ok(revoke), CommandOutcome::Done);
    assert_eq!(listed(&host), []);
    host.shutdown();
}

/// A notice the host no longer keeps (here one sent before a restart, which keeps none) is
/// `blocklist.notice_expired`, which is not retryable: only a new notice can be allowed.
#[test]
fn an_allow_from_a_notice_the_host_no_longer_keeps_has_expired() {
    let dir = Dir::new("allowed_sites_expired", AGENTS);
    let sites = Arc::new(AllowedSites::default());
    let host = start(&dir, 0, &sites);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let evil = notice(&host, (matter, thread), "evil.example");
    host.shutdown();

    let sites = Arc::new(AllowedSites::default());
    let host = start(&dir, 1, &sites);
    let error = host.refused(
        allow(evil, AllowScope::Matter),
        ErrorCode::BlocklistNoticeExpired,
    );
    assert!(!error.code().is_retryable());
    assert_eq!(listed(&host), []);
    let again = notice(&host, (matter, thread), "evil.example");
    assert_eq!(
        host.ok(allow(again, AllowScope::Matter)),
        CommandOutcome::Done
    );
    host.shutdown();
}

/// A revoke whose save failed answers `store.io`; its retry writes the file although memory no
/// longer holds the allow, so the allow does not come back at the next start.
#[cfg(unix)]
#[test]
fn a_revoke_whose_save_failed_is_saved_by_its_retry() {
    let dir = Dir::new("allowed_sites_revoke_retry", AGENTS);
    let sites = Arc::new(AllowedSites::default());
    let host = start(&dir, 0, &sites);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let evil = notice(&host, (matter, thread), "evil.example");
    host.ok(allow(evil, AllowScope::Always));
    let folder = dir.data().join("blocklist");
    crate::write::set_mode(&folder, 0o555);
    let revoke = HostCommand::RevokeAllowedSite {
        host: "evil.example".to_owned(),
        scope: AllowScope::Always,
        matter: None,
    };
    host.refused(revoke.clone(), ErrorCode::StoreIo);
    assert_eq!(listed(&host), []);
    crate::write::set_mode(&folder, 0o700);
    assert_eq!(host.ok(revoke), CommandOutcome::Done);
    host.shutdown();

    let sites = Arc::new(AllowedSites::default());
    let host = start(&dir, 1, &sites);
    assert_eq!(listed(&host), []);
    assert_eq!(hosts(&sites.for_matter(matter)), Vec::<String>::new());
    host.shutdown();
}
