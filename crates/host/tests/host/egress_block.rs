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
use clauseo_host::{
    AppSubscription, BlockListSeams, BlockListUpdate, EgressProxy, ManualClock, SessionKey,
};
use clauseo_protocol::{BlockedSiteId, HostEvent, MatterId, Principal, SiteTag};
use clauseo_sandbox::{Denial, DenialSink, ProxyConfig, ProxyHandle, Upstream, start_proxy};
use futures::StreamExt as _;
use serde_json::json;

use crate::egress::claude_then;
use crate::profile_rig::{BROWSING, Hook, Wired, browser_server, turn};
use crate::settings::decode;
use crate::support::{Dir, Running, wait};

/// The launch's wait for the block list's first load (`blocklist/first_load.rs`): no other
/// host timer has this delay.
const FIRST_LOAD_WAIT: Duration = Duration::from_secs(4);

/// The run whose clock starts at 2026-11-07T06:13:20Z, just after the signed fixtures were
/// made, so the updater does not set them aside as dated ahead.
const FIXTURE_RUN: u16 = 1120;

/// Both are seed sites, so the session's allowlist lets them through when no list names them.
pub(crate) const LISTED: &str = "sci.gov.in";
const LATER: &str = "delhihighcourt.nic.in";

pub(crate) const FIRST: &str = "2026-11-07T06:00:00Z";
const SECOND: &str = "2026-11-07T12:00:00Z";

/// A list made at `made` naming `hosts` as phishing.
pub(crate) fn list(made: &str, hosts: &[&str]) -> BlockList {
    let mut lines: Vec<String> = hosts.iter().map(|host| format!("{host} p")).collect();
    lines.sort_unstable();
    let text = format!("{}\n", lines.join("\n"));
    let header = Header {
        generated: Generated::parse(made).unwrap(),
        entries: u32::try_from(lines.len()).unwrap(),
    };
    parse_plain(text.as_bytes(), header).unwrap()
}

/// An upstream proxy that answers every `CONNECT` with 200, then echoes the tunnel's bytes until
/// the proxy closes it: a site that passed.
fn upstream() -> url::Url {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let _detached = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let _echo = std::thread::spawn(move || {
                let _head = read_head(&mut stream);
                let _sent = stream.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
                let mut back = stream.try_clone().unwrap();
                let _copied = std::io::copy(&mut stream, &mut back);
            });
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
pub(crate) struct RealProxy {
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

    /// The browser port of the one open session in `matter`, or of the one open session.
    fn port(&self, matter: Option<MatterId>) -> u16 {
        let open = self.open.lock().unwrap();
        let mut ports = open
            .iter()
            .filter(|(key, _)| matter.is_none_or(|matter| key.matter == matter))
            .map(|(_, handle)| handle.browser_port().unwrap());
        let port = ports.next().expect("an open session");
        assert!(ports.next().is_none(), "one session");
        port
    }

    /// A `CONNECT` to `host:443` through the browser listener of the session in `matter` (the
    /// one session with `None`): its status, and the stream (the open tunnel on a 200).
    pub(crate) fn tunnel(&self, matter: Option<MatterId>, host: &str) -> (u16, TcpStream) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port(matter))).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .unwrap();
        let request = format!("CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let head = read_head(&mut stream);
        let status = head
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status: {head:?}"));
        (status, stream)
    }

    /// The status of a `CONNECT` to `host:443` through the session in `matter`.
    pub(crate) fn connect_in(&self, matter: MatterId, host: &str) -> u16 {
        self.tunnel(Some(matter), host).0
    }

    /// The status of a `CONNECT` to `host:443` through the one session.
    fn connect(&self, host: &str) -> u16 {
        self.tunnel(None, host).0
    }
}

/// Update settings that never reach a list (a closed local port), so only the test swaps it.
pub(crate) fn no_fetch() -> BlockListUpdate {
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
    let seams = proxy
        .block
        .clone()
        .map(|current| BlockListSeams::new(current, None, no_fetch()));
    session_with(dir, proxy, 0, seams, |_clock| {})
}

/// [`session`] on run `run`'s clock with `seams`, `while_launching` started on its own thread
/// with the host clock before the prompt.
fn session_with(
    dir: &Dir,
    proxy: &Arc<RealProxy>,
    run: u16,
    seams: Option<BlockListSeams>,
    while_launching: impl FnOnce(ManualClock) + Send + 'static,
) -> (Running, AppSubscription) {
    let (host, sub) = start_host(dir, proxy, run, seams, while_launching);
    open_session(dir, &host, "m");
    (host, sub)
}

/// A host on run `run`'s clock whose sessions get the browser and `proxy`, with `seams`, and
/// its app subscription; `while_launching` started on its own thread with the host clock.
pub(crate) fn start_host(
    dir: &Dir,
    proxy: &Arc<RealProxy>,
    run: u16,
    seams: Option<BlockListSeams>,
    while_launching: impl FnOnce(ManualClock) + Send + 'static,
) -> (Running, AppSubscription) {
    claude_then(dir, &json!([{"name": "browser"}]), &[]);
    let wired: Arc<dyn EgressProxy> = Arc::<RealProxy>::clone(proxy);
    let host = dir.start_launching(run, Wired::new(dir), move |config| {
        let config = config
            .with_profile_hook(Box::new(Hook {
                servers: vec![browser_server()],
                ..Hook::default()
            }))
            .with_egress_proxy(wired)
            .with_browser_executable(Some(PathBuf::from("/chrome")));
        // The profile hook offers a guarded browser with a proxy, which no shipped entry has,
        // so the seams say the list is enforced and the host runs its updater
        // (`blocklist::needed`).
        match seams {
            Some(seams) => seams.enforced().apply(config),
            None => config,
        }
    });
    let clock = host.clock.clone();
    let _detached = std::thread::spawn(move || while_launching(clock));
    let sub = wait(
        "subscribe_app",
        host.handle.subscribe_app(Principal::LocalDesktop),
    )
    .unwrap();
    (host, sub)
}

/// A new matter `name` with a Claude session, after one turn, so its proxy is open.
pub(crate) fn open_session(dir: &Dir, host: &Running, name: &str) -> MatterId {
    let matter = host.create_matter(name, &dir.folder(name, &[]));
    let thread = host.create_thread(matter, BROWSING);
    turn(host, thread);
    matter
}

/// The next `SiteBlocked` notice on `sub` (id, host, count), waited for up to the watchdog;
/// any other event is skipped.
pub(crate) fn next_notice(sub: &mut AppSubscription) -> (BlockedSiteId, String, u32) {
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

pub(crate) fn real_proxy(block: Option<Arc<Current>>) -> Arc<RealProxy> {
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

/// Seams on `current` with the signed fixture `list` (key A, `evil.example` on it) as the
/// bundled snapshot in `dir`, the first load held until `release` sends or drops.
fn bundled_seams(
    dir: &Dir,
    current: Arc<Current>,
    release: std::sync::mpsc::Receiver<()>,
) -> BlockListSeams {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../blocklist/tests/data");
    let bundled = dir.folder("bundled", &[]);
    for (fixture, name) in [
        ("list.txt.gz", clauseo_blocklist::LIST_FILE),
        ("list.txt.gz.minisig", clauseo_blocklist::SIGNATURE_FILE),
    ] {
        crate::write::write(
            &bundled.join(name),
            &std::fs::read(data.join(fixture)).unwrap(),
        );
    }
    let key = std::fs::read_to_string(data.join("key-a.pub")).unwrap();
    let mut update = no_fetch();
    update.keys = vec![key.lines().nth(1).unwrap().to_owned()];
    let release = Mutex::new(release);
    BlockListSeams::new(current, Some(bundled), update).before_first_load(move || {
        let _released = release.lock().unwrap().recv();
    })
}

/// Waits until the launch arms its wait for the first load.
fn until_the_launch_waits(clock: &ManualClock) {
    while wait("the launch's block list wait", clock.next_armed()) != FIRST_LOAD_WAIT {}
}

/// A launch that starts while the first load is held waits for it: once the load is let go,
/// its proxy refuses a site on the bundled list and the app is told.
#[test]
fn a_launch_during_a_slow_first_load_gets_the_list() {
    let dir = Dir::new("egress_block_first_load", &[]);
    let current = Arc::new(Current::default());
    let proxy = real_proxy(Some(Arc::clone(&current)));
    let (release, held) = std::sync::mpsc::channel();
    let seams = bundled_seams(&dir, current, held);
    let (host, mut sub) = session_with(&dir, &proxy, FIXTURE_RUN, Some(seams), move |clock| {
        until_the_launch_waits(&clock);
        release.send(()).unwrap();
    });
    assert_eq!(proxy.connect("evil.example"), 403);
    let (_id, site, count) = next_notice(&mut sub);
    assert_eq!((site.as_str(), count), ("evil.example", 1));
    host.shutdown();
}

/// A first load that does not end within the wait: the launch goes on, on the host clock, with
/// the list in use (none), so nothing is blocked; the first notice is the barrier's.
#[test]
fn a_launch_goes_on_after_the_first_load_wait() {
    let dir = Dir::new("egress_block_first_load_late", &[]);
    let current = Arc::new(Current::default());
    let proxy = real_proxy(Some(Arc::clone(&current)));
    let (release, held) = std::sync::mpsc::channel::<()>();
    let seams = bundled_seams(&dir, current, held);
    let (host, mut sub) = session_with(&dir, &proxy, FIXTURE_RUN, Some(seams), |clock| {
        until_the_launch_waits(&clock);
        clock.advance(FIRST_LOAD_WAIT);
    });
    assert_eq!(proxy.connect(LISTED), 200);
    proxy.barrier("barrier.example");
    let (_id, site, _count) = next_notice(&mut sub);
    assert_eq!(site, "barrier.example", "nothing was blocked before it");
    drop(release);
    host.shutdown();
}
