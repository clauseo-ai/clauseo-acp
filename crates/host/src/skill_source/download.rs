//! The download of a skill source, and its preview.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use clauseo_profile::packs::source::remote::CODELOAD;
use clauseo_profile::packs::source::{
    DOWNLOAD_MAX, Preview, Remote, SourceError, Tree, stage_tree,
};
use clauseo_sandbox::Upstream;
use url::{Host, Url};

use crate::blocklist::fetch::{Clients, Failure, upstream};
use crate::clock::{Clock, SystemClock};

/// The longest one download takes, every hop together (one deadline), under the preview job's
/// own 120 s.
pub(crate) const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(90);

/// The most redirects one download follows.
const MAX_REDIRECTS: usize = 5;

/// GitHub, whose `/<owner>.png` is the owner's picture (`icon.rs`).
const GITHUB: &str = "https://github.com";

/// The skills.sh catalogue's search.
const SEARCH_URL: &str = "https://skills.sh/api/search";

/// What a download or a search goes through: the clients by route, the route's source, whether
/// plain `http` is refused, the one local origin allowed (the tests' fake server), GitHub's
/// archive host, the search's address and the clock a download's deadline is read on.
pub(crate) struct Net {
    clients: Clients,
    upstream: Upstream,
    https_only: bool,
    local_origin: Option<url::Origin>,
    clock: Arc<dyn Clock>,
    archive_host: String,
    avatar_host: String,
    pub(super) search_url: String,
}

impl Net {
    /// This net, reading a download's deadline on `clock` (the actor's).
    pub(crate) fn with_clock(self, clock: Arc<dyn Clock>) -> Net {
        Net { clock, ..self }
    }

    /// Where `/<owner>.png` is fetched from: GitHub, or the fake server in tests.
    pub(crate) fn avatar_host(&self) -> &str {
        &self.avatar_host
    }

    /// The release's: `https` only, through `upstream_proxy` (`network.upstream_proxy`) when
    /// set, else `fallback` (the system's proxy). `DownloadFailed` when the setting is refused.
    pub(crate) fn new(
        upstream_proxy: Option<&str>,
        fallback: &Upstream,
    ) -> Result<Net, SourceError> {
        let upstream = upstream(upstream_proxy, fallback).map_err(|_refused| {
            tracing::warn!("a skill download: the proxy setting is refused");
            SourceError::DownloadFailed
        })?;
        Ok(Net {
            clients: Clients::new(true),
            upstream,
            https_only: true,
            local_origin: None,
            clock: Arc::new(SystemClock),
            archive_host: CODELOAD.to_owned(),
            avatar_host: GITHUB.to_owned(),
            search_url: SEARCH_URL.to_owned(),
        })
    }

    /// The tests': plain `http` to the fake server at `origin` (`http://127.0.0.1:<port>`)
    /// allowed, direct; GitHub's archives and the search are on it. Only
    /// `HostConfig::with_skill_server` (test support) leads here outside this crate's tests.
    pub(crate) fn plain(origin: &str) -> Net {
        Net {
            clients: Clients::new(false),
            upstream: Upstream::Direct,
            https_only: false,
            local_origin: Url::parse(origin).ok().map(|url| url.origin()),
            clock: Arc::new(SystemClock),
            archive_host: origin.to_owned(),
            avatar_host: origin.to_owned(),
            search_url: format!("{origin}/api/search"),
        }
    }
}

/// GETs `start` through `net`, at most `cap` bytes, every hop together within `timeout`. The
/// log lines name the kind of failure, never the link.
pub(crate) fn download(
    net: &Net,
    start: &str,
    cap: u64,
    timeout: Duration,
) -> Result<Vec<u8>, SourceError> {
    let mut url = Url::parse(start).map_err(|_bad| SourceError::BadSource)?;
    let deadline = net.clock.now().mono.saturating_add(timeout);
    for hop in 0..=MAX_REDIRECTS {
        check(net, &url)?;
        if is_local(net, &url) {
            tracing::warn!(hop, "a skill download was sent to a local address");
            return Err(if hop == 0 {
                SourceError::BadSource
            } else {
                SourceError::NotFound
            });
        }
        let left = deadline.saturating_sub(net.clock.now().mono);
        if left.is_zero() {
            tracing::warn!("a skill download timed out");
            return Err(SourceError::DownloadFailed);
        }
        let client = net
            .clients
            .for_url(&net.upstream, &url)
            .map_err(|failure| {
                if let Failure::Refused(why) = failure {
                    tracing::warn!(why = %why, "a skill download found no route");
                }
                SourceError::DownloadFailed
            })?;
        let got = client.get(url.as_str(), cap, left).map_err(|why| {
            tracing::warn!(why = %why, "a skill download failed");
            if why == "answer too large" {
                SourceError::TooBig
            } else {
                SourceError::DownloadFailed
            }
        })?;
        match got.status {
            200 => return Ok(got.body),
            404 | 410 => return Err(SourceError::NotFound),
            301 | 302 | 303 | 307 | 308 => {
                let location = got.location.ok_or(SourceError::DownloadFailed)?;
                url = url
                    .join(&location)
                    .map_err(|_bad| SourceError::DownloadFailed)?;
            }
            status => {
                tracing::warn!(status, "a skill download was answered with an error");
                return Err(SourceError::DownloadFailed);
            }
        }
    }
    tracing::warn!("a skill download was redirected too many times");
    Err(SourceError::DownloadFailed)
}

/// Whether `url` may be fetched: `https` (or plain `http` when `net` allows it), no explicit
/// port when `https` only, no user info. A redirect to a link that is not is a failed download.
fn check(net: &Net, url: &Url) -> Result<(), SourceError> {
    let scheme = url.scheme();
    let scheme_ok = scheme == "https" || (!net.https_only && scheme == "http");
    let port_ok = !net.https_only || url.port().is_none();
    let plain = url.username().is_empty() && url.password().is_none() && url.host().is_some();
    if scheme_ok && port_ok && plain {
        Ok(())
    } else {
        tracing::warn!("a skill download was sent to a link Clauseo does not fetch");
        Err(SourceError::DownloadFailed)
    }
}

/// Whether `url` names this computer or a private network (on every hop, the first link too):
/// `localhost`, or a loopback, private, link-local, shared, unspecified, reserved, multicast or
/// unique-local IP literal (see [`is_local_v4`] and [`is_local_v6`]). A local source is a
/// picked folder. Only the tests' fake server is let through.
fn is_local(net: &Net, url: &Url) -> bool {
    if net.local_origin.as_ref() == Some(&url.origin()) {
        return false;
    }
    match url.host() {
        Some(Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost")
        }
        Some(Host::Ipv4(ip)) => is_local_v4(ip),
        Some(Host::Ipv6(ip)) => is_local_v6(ip),
        None => true,
    }
}

/// Loopback 127/8, private 10/8, 172.16/12 and 192.168/16, link-local 169.254/16, shared
/// 100.64/10, benchmarking 198.18/15, unspecified 0/8, multicast 224/4, and reserved 240/4
/// (broadcast among them).
fn is_local_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || a == 0
        || a >= 240
        || (a == 100 && (64..128).contains(&b))
        || (a == 198 && (b & 0xfe) == 18)
}

/// Loopback `::1`, unspecified `::`, unique-local `fc00::/7`, link-local `fe80::/10`,
/// multicast `ff00::/8`, the whole NAT64 prefix `64:ff9b::/96` (it only ever wraps an IPv4
/// address, and a public one is reachable as itself), and an IPv4-mapped, -compatible or 6to4
/// (`2002::/16`) address whose IPv4 is local.
fn is_local_v6(ip: Ipv6Addr) -> bool {
    let [first, second, third, fourth, fifth, sixth, ..] = ip.segments();
    let nat64 = [first, second, third, fourth, fifth, sixth] == [0x64, 0xff9b, 0, 0, 0, 0];
    let six_to_four = first == 0x2002 && {
        let [a, b] = second.to_be_bytes();
        let [c, d] = third.to_be_bytes();
        is_local_v4(Ipv4Addr::new(a, b, c, d))
    };
    let mapped = ip
        .to_ipv4_mapped()
        .or_else(|| ip.to_ipv4())
        .is_some_and(is_local_v4);
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        || nat64
        || six_to_four
        || mapped
}

/// Downloads `remote` through `net` and stages it under `adding`: GitHub's archive with its
/// top folder skipped and narrowed to the link's folder and the skills.sh skill; a `.zip` link
/// as it is. Off the actor.
pub(crate) fn preview_remote(
    net: &Net,
    remote: &Remote,
    adding: &Path,
    now_ms: i64,
) -> Result<Preview, SourceError> {
    let link = match remote {
        Remote::GitHub(github) => format!("{}{}", net.archive_host, github.archive_path()),
        Remote::Zip { url } => url.clone(),
    };
    let bytes = download(net, &link, DOWNLOAD_MAX, DOWNLOAD_TIMEOUT)?;
    let (strip_top, path, asked) = match remote {
        Remote::GitHub(github) => (true, github.path.as_deref(), github.skill.as_deref()),
        Remote::Zip { .. } => (false, None, None),
    };
    let tree = Tree::Zip {
        bytes: &bytes,
        strip_top,
        path,
    };
    stage_tree(adding, tree, remote.origin(), asked, now_ms)
}
