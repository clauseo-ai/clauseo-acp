//! The download of a skill source, and its preview.

use std::path::Path;
use std::time::Duration;

use clauseo_profile::packs::source::remote::CODELOAD;
use clauseo_profile::packs::source::{
    DOWNLOAD_MAX, Preview, Remote, SourceError, Tree, stage_tree,
};
use clauseo_sandbox::Upstream;
use url::Url;

use crate::blocklist::fetch::{Clients, Failure, upstream};

/// The longest one download takes, every hop together.
pub(crate) const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(90);

/// The most redirects one download follows.
const MAX_REDIRECTS: usize = 5;

/// GitHub, whose `/<owner>.png` is the owner's picture (`icon.rs`).
const GITHUB: &str = "https://github.com";

/// The skills.sh catalogue's search.
const SEARCH_URL: &str = "https://skills.sh/api/search";

/// What a download or a search goes through: the clients by route, the route's source, whether
/// plain `http` is refused, GitHub's archive host and the search's address.
pub(crate) struct Net {
    clients: Clients,
    upstream: Upstream,
    https_only: bool,
    archive_host: String,
    avatar_host: String,
    pub(super) search_url: String,
}

impl Net {
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
            archive_host: origin.to_owned(),
            avatar_host: origin.to_owned(),
            search_url: format!("{origin}/api/search"),
        }
    }
}

/// GETs `start` through `net`, at most `cap` bytes, every hop within `timeout`. The log
/// lines name the kind of failure, never the link.
pub(crate) fn download(
    net: &Net,
    start: &str,
    cap: u64,
    timeout: Duration,
) -> Result<Vec<u8>, SourceError> {
    let mut url = Url::parse(start).map_err(|_bad| SourceError::BadSource)?;
    for _hop in 0..=MAX_REDIRECTS {
        check(net, &url)?;
        let client = net
            .clients
            .for_url(&net.upstream, &url)
            .map_err(|failure| {
                if let Failure::Refused(why) = failure {
                    tracing::warn!(why = %why, "a skill download found no route");
                }
                SourceError::DownloadFailed
            })?;
        let got = client.get(url.as_str(), cap, timeout).map_err(|why| {
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
