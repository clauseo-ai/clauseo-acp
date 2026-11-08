//! The block list in the session's proxy (ADR 0053) through the host: each launch's proxy reads
//! the shared list of the host's `BlockListSeams`, refuses a listed site through the session's
//! real browser listener and tells the app (`SiteBlocked`, `site_blocked.rs`'s rule: one event per
//! refusal, the same `block_id` with the count one higher for the same site in a turn); a swap
//! of the list reaches the next connection; with no seams nothing is refused.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_blocklist::{BlockList, Current, Generated, Header, parse_plain};
use clauseo_host::{AppSubscription, BlockListSeams, BlockListUpdate, EgressProxy, SessionKey};
use clauseo_protocol::{BlockedSiteId, HostEvent, Principal, SiteTag};
use clauseo_sandbox::{Denial, DenialSink, ProxyConfig, ProxyHandle, Upstream, start_proxy};
use futures::StreamExt as _;
use serde_json::json;

use crate::egress::claude_then;
use crate::profile_rig::{CLAUDE, Hook, Wired, browser_server, turn};
use crate::settings::decode;
use crate::support::{Dir, Running, wait};

/// Both are seed sites, so the session's allowlist lets them through when no list names them.
const LISTED: &str = "sci.gov.in";
const LATER: &str = "delhihighcourt.nic.in";

const FIRST: &str = "2026-11-07T06:00:00Z";
const SECOND: &str = "2026-11-07T12:00:00Z";

/// A list made at `made` naming `hosts` as phishing.
fn list(made: &str, hosts: &[&str]) -> BlockList {
    let mut lines: Vec<String> = hosts.iter().map(|host| format!("{host} p")).collect();
    lines.sort_unstable();
    let text = format!("{}\n", lines.join("\n"));
    let header = Header {
        generated: Generated::parse(made).unwrap(),
        entries: u32::try_from(lines.len()).unwrap(),
    };
    parse_plain(text.as_bytes(), header).unwrap()
}

/// An upstream proxy that answers every `CONNECT` with 200 and then closes: a site that passed.
fn upstream() -> url::Url {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let _detached = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let _head = read_head(&mut stream);
            let _sent = stream.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
        }
    });
    url::Url::parse(&format!("http://127.0.0.1:{port}")).unwrap()
}

/// The head of an answer, up to its blank line.
fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0_u8];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&head).into_owned()
}

/// The real proxy (`start_proxy`) with the host's config, only its upstream pointed at the
/// local [`upstream`]. Each open checks the config's list is `block`'s (or empty, with none),
/// and keeps the session's sink for [`RealProxy::barrier`].
struct RealProxy {
    open: Mutex<BTreeMap<SessionKey, ProxyHandle>>,
    sinks: Mutex<Vec<Arc<dyn DenialSink>>>,
    upstream: url::Url,
    block: Option<Arc<Current>>,
}

impl EgressProxy for RealProxy {
    fn open(
        &self,
        session: SessionKey,
        mut config: ProxyConfig,
    ) -> Result<(), clauseo_sandbox::Error> {
        match &self.block {
            Some(block) => assert!(Arc::ptr_eq(&config.block, block), "not the seams' list"),
            None => assert!(config.block.get().is_unloaded(), "a list without seams"),
        }
        config.upstream = Upstream::Fixed(self.upstream.clone());
        self.sinks.lock().unwrap().push(Arc::clone(&config.denials));
        let handle = start_proxy(config)?;
        self.open.lock().unwrap().insert(session, handle);
        Ok(())
    }

    fn port_for(&self, session: SessionKey) -> Option<u16> {
        self.open
            .lock()
            .unwrap()
            .get(&session)
            .and_then(ProxyHandle::browser_port)
    }

    fn close(&self, session: SessionKey) {
        drop(self.open.lock().unwrap().remove(&session));
    }
}

impl RealProxy {
    /// Reports a refusal of `host` straight to the one session's sink, as the proxy would: a
    /// positive barrier. Its notice comes after any the proxy's sink posted before it, since
    /// both go down the same actor channel in order.
    fn barrier(&self, host: &str) {
        let sinks = self.sinks.lock().unwrap();
        assert_eq!(sinks.len(), 1, "one session");
        sinks[0].denied(Denial::Blocked {
            host: host.to_owned(),
            port: 443,
            listed: host.to_owned(),
            tag: clauseo_blocklist::Tag::Phishing,
            generated: Generated::parse(FIRST).unwrap(),
        });
    }

    /// The one open session's browser port.
    fn port(&self) -> u16 {
        let open = self.open.lock().unwrap();
        assert_eq!(open.len(), 1, "one session");
        open.values().next().unwrap().browser_port().unwrap()
    }

    /// The status of a `CONNECT` to `host:443` through the session's browser listener.
    fn connect(&self, host: &str) -> u16 {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port())).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .unwrap();
        let request = format!("CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let head = read_head(&mut stream);
        head.split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status: {head:?}"))
    }
}

/// Update settings that never reach a list (a closed local port), so only the test swaps it.
fn no_fetch() -> BlockListUpdate {
    let mut update = BlockListUpdate::release();
    update.list_url = "http://127.0.0.1:9/list.gz".to_owned();
    update.signature_url = "http://127.0.0.1:9/list.gz.minisig".to_owned();
    update.allowed_hosts = vec!["127.0.0.1".to_owned()];
    update.https_only = false;
    update.upstream = Upstream::Direct;
    update
}

/// A host with a running Claude session that has the browser and `proxy`, with the block list's
/// seams on `proxy.block` when it has one, after one turn; and its app subscription.
fn session(dir: &Dir, proxy: &Arc<RealProxy>) -> (Running, AppSubscription) {
    claude_then(dir, &json!([{"name": "browser"}]), &[]);
    let wired: Arc<dyn EgressProxy> = Arc::<RealProxy>::clone(proxy);
    let block = proxy.block.clone();
    let host = dir.start_launching(0, Wired::new(dir), move |config| {
        let config = config
            .with_profile_hook(Box::new(Hook {
                servers: vec![browser_server()],
                ..Hook::default()
            }))
            .with_egress_proxy(wired)
            .with_browser_executable(Some(PathBuf::from("/chrome")));
        match block {
            Some(current) => BlockListSeams::new(current, None, no_fetch()).apply(config),
            None => config,
        }
    });
    let sub = wait(
        "subscribe_app",
        host.handle.subscribe_app(Principal::LocalDesktop),
    )
    .unwrap();
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    (host, sub)
}

/// The next `SiteBlocked` notice on `sub` (id, host, count), waited for up to the watchdog;
/// any other event is skipped.
fn next_notice(sub: &mut AppSubscription) -> (BlockedSiteId, String, u32) {
    loop {
        let item = wait("a SiteBlocked notice", sub.next()).expect("the app stream ended");
        if let HostEvent::SiteBlocked {
            block_id,
            thread,
            host,
            tag,
            count,
            ..
        } = decode(&item)
        {
            assert!(thread.is_some());
            assert_eq!(tag, SiteTag::Phishing);
            return (block_id, host, count);
        }
    }
}

fn real_proxy(block: Option<Arc<Current>>) -> Arc<RealProxy> {
    Arc::new(RealProxy {
        open: Mutex::new(BTreeMap::new()),
        sinks: Mutex::new(Vec::new()),
        upstream: upstream(),
        block,
    })
}

/// A listed site is refused through the session's proxy, and each refusal is told once: the
/// same notice re-sent with its count one higher. A site not on the list passes, with no notice:
/// the next notice after it is a later refusal's.
#[test]
fn a_listed_site_is_refused_and_told_once_per_refusal_and_others_pass() {
    let dir = Dir::new("egress_block_listed", &[]);
    let proxy = real_proxy(Some(Arc::new(Current::new(list(FIRST, &[LISTED])))));
    let (host, mut sub) = session(&dir, &proxy);
    assert_eq!(proxy.connect(LISTED), 403);
    let (id, site, count) = next_notice(&mut sub);
    assert_eq!((site.as_str(), count), (LISTED, 1));
    assert_eq!(proxy.connect(LISTED), 403);
    assert_eq!(next_notice(&mut sub), (id, LISTED.to_owned(), 2));
    assert_eq!(proxy.connect(LATER), 200, "not on the list");
    assert_eq!(proxy.connect(LISTED), 403, "the barrier");
    assert_eq!(
        next_notice(&mut sub),
        (id, LISTED.to_owned(), 3),
        "no notice for the unlisted site came first"
    );
    host.shutdown();
}

/// A swap of the shared list (the updater's `offer`) reaches the session's live proxy at its
/// next connection, with no restart.
#[test]
fn a_list_swap_reaches_the_next_connection() {
    let dir = Dir::new("egress_block_swap", &[]);
    let current = Arc::new(Current::new(list(FIRST, &[LISTED])));
    let proxy = real_proxy(Some(Arc::clone(&current)));
    let (host, mut sub) = session(&dir, &proxy);
    assert_eq!(proxy.connect(LATER), 200);
    assert!(current.offer(list(SECOND, &[LISTED, LATER]), None));
    assert_eq!(proxy.connect(LATER), 403);
    let (_id, site, count) = next_notice(&mut sub);
    assert_eq!(
        (site.as_str(), count),
        (LATER, 1),
        "the first notice is the swap's"
    );
    host.shutdown();
}

/// With no block-list seams the proxy gets an empty list (checked at open) and refuses nothing:
/// the first notice is the barrier's, reported to the session's sink after both connections.
#[test]
fn without_seams_nothing_is_refused() {
    let dir = Dir::new("egress_block_none", &[]);
    let proxy = real_proxy(None);
    let (host, mut sub) = session(&dir, &proxy);
    assert_eq!(proxy.connect(LISTED), 200);
    assert_eq!(proxy.connect(LATER), 200);
    proxy.barrier("barrier.example");
    let (_id, site, count) = next_notice(&mut sub);
    assert_eq!((site.as_str(), count), ("barrier.example", 1));
    host.shutdown();
}
