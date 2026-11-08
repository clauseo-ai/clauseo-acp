//! The lawyer's "Allow this site" (ADR 0053, #685) through the session's real proxy: each launch's
//! proxy takes its matter's live view of the allowed sites, so a `Matter` allow opens a listed
//! site for that matter's sessions only, an `Always` allow for every session, and a revoke closes
//! a tunnel already open to it (the proxy's revoke watcher, held as long as the proxy).

use std::io::{Read, Write};
use std::sync::Arc;

use clauseo_blocklist::Current;
use clauseo_connectors::MemoryStore;
use clauseo_host::BlockListSeams;
use clauseo_protocol::{AllowScope, CommandOutcome, HostCommand};

use crate::egress_block::{
    FIRST, LISTED, list, next_notice, no_fetch, open_session, real_proxy, start_host,
};

/// A host with the list naming [`LISTED`], its sessions in matters `a` and then `others`, all
/// on the real proxy; the matters and the app subscription.
fn host_with(
    dir: &crate::support::Dir,
    others: &[&str],
) -> (
    crate::support::Running,
    clauseo_host::AppSubscription,
    Arc<crate::egress_block::RealProxy>,
    Vec<clauseo_protocol::MatterId>,
) {
    let current = Arc::new(Current::new(list(FIRST, &[LISTED])));
    let proxy = real_proxy(Some(Arc::clone(&current)));
    // A secret store, so an allow is saved as well as applied (without one, main answers
    // `blocklist.allow_not_saved`).
    let seams =
        BlockListSeams::new(current, None, no_fetch()).with_secrets(Arc::new(MemoryStore::new()));
    let sites = Arc::clone(seams.allowed());
    let (host, sub) = start_host(dir, &proxy, 0, Some(seams), |_clock| {});
    crate::allowed_sites::wait_loaded(&sites);
    let mut matters = vec![open_session(dir, &host, "a")];
    for name in others {
        matters.push(open_session(dir, &host, name));
    }
    (host, sub, proxy, matters)
}

/// Blocks [`LISTED`] in `matter` through the proxy and allows its notice for `scope`.
fn allow_after_a_block(
    host: &crate::support::Running,
    sub: &mut clauseo_host::AppSubscription,
    proxy: &crate::egress_block::RealProxy,
    matter: clauseo_protocol::MatterId,
    scope: AllowScope,
) {
    assert_eq!(proxy.connect_in(matter, LISTED), 403);
    let (block_id, site, _count) = next_notice(sub);
    assert_eq!(site, LISTED);
    let allow = HostCommand::AllowBlockedSite { block_id, scope };
    assert_eq!(host.ok(allow), CommandOutcome::Done);
}

/// A `Matter` allow opens the listed site for that matter's session only.
#[test]
fn a_matter_allow_opens_the_site_for_that_matters_sessions_only() {
    let dir = crate::support::Dir::new("egress_allow_matter", &[]);
    let (host, mut sub, proxy, matters) = host_with(&dir, &["b"]);
    let (a, b) = (matters[0], matters[1]);
    allow_after_a_block(&host, &mut sub, &proxy, a, AllowScope::Matter);
    assert_eq!(proxy.connect_in(a, LISTED), 200, "allowed in a");
    assert_eq!(proxy.connect_in(b, LISTED), 403, "still blocked in b");
    host.shutdown();
}

/// An `Always` allow opens it for every session.
#[test]
fn an_always_allow_opens_the_site_for_every_session() {
    let dir = crate::support::Dir::new("egress_allow_always", &[]);
    let (host, mut sub, proxy, matters) = host_with(&dir, &["b"]);
    let (a, b) = (matters[0], matters[1]);
    allow_after_a_block(&host, &mut sub, &proxy, b, AllowScope::Always);
    assert_eq!(proxy.connect_in(a, LISTED), 200);
    assert_eq!(proxy.connect_in(b, LISTED), 200);
    host.shutdown();
}

/// A revoke closes a tunnel already open to the site, and the next one is refused.
#[test]
fn a_revoke_closes_an_open_tunnel() {
    let dir = crate::support::Dir::new("egress_allow_revoke", &[]);
    let (host, mut sub, proxy, matters) = host_with(&dir, &[]);
    let a = matters[0];
    allow_after_a_block(&host, &mut sub, &proxy, a, AllowScope::Matter);
    let (status, mut tunnel) = proxy.tunnel(Some(a), LISTED);
    assert_eq!(status, 200);
    tunnel.write_all(b"ping").unwrap();
    let mut echoed = [0_u8; 4];
    tunnel.read_exact(&mut echoed).unwrap();
    assert_eq!(&echoed, b"ping", "the tunnel is open");
    let revoke = HostCommand::RevokeAllowedSite {
        host: LISTED.to_owned(),
        scope: AllowScope::Matter,
        matter: Some(a),
    };
    assert_eq!(host.ok(revoke), CommandOutcome::Done);
    // The read times out (60 s) rather than hang if the tunnel stayed open.
    let mut rest = Vec::new();
    let closed = tunnel.read_to_end(&mut rest);
    assert!(closed.is_ok() && rest.is_empty(), "{closed:?} {rest:?}");
    assert_eq!(proxy.connect_in(a, LISTED), 403, "blocked again");
    host.shutdown();
}
