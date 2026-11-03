//! Rust's half of an install: each file fetched by itself, resumed with `Range`, hashed as it is
//! written, checked against the lock's SHA-512 (or Node's SHA-256) and renamed into place. Also
//! the firm's two ways around the network: a mirror base for tarball URLs, and an offline bundle.
//!
//! ureq is blocking, so a fetch runs on the install's own thread. A pause is seen between chunks.

mod error;
pub(crate) mod firm;

use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_protocol::AgentId;
use ureq::config::{AutoHeaderValue, Config};
use ureq::tls::{PemItem, RootCerts, TlsConfig};

pub(crate) use error::{
    FetchError, FetchKind, classify, error_status, host_of, status_kind, write_kind,
};

use crate::cancel::Cancel;
use crate::config::InstallConfig;
use crate::integrity::Expected;
use crate::write;

/// The most redirects one fetch follows.
const MAX_REDIRECTS: u32 = 3;
/// The longest a connect may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest the response's headers, or any one read of the body, may take: a stall timer.
/// The body as a whole has no limit (a slow link may take minutes); the size cap bounds it.
const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// The size of each read and write.
const CHUNK: usize = 64 * 1024;

/// What the first bytes of a file must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Magic {
    /// gzip, `1f 8b`: npm tarballs and Node's `.tar.gz` archives.
    Gzip,
    /// zip, `PK`: Node's Windows archives.
    Zip,
}

impl Magic {
    /// The leading bytes.
    fn bytes(self) -> &'static [u8] {
        match self {
            Magic::Gzip => &[0x1f, 0x8b],
            Magic::Zip => b"PK",
        }
    }

    /// The word for a detail line.
    fn name(self) -> &'static str {
        match self {
            Magic::Gzip => "gzip",
            Magic::Zip => "zip",
        }
    }
}

/// One file to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FetchJob {
    /// Where it comes from: the lock's `resolved` (after [`mirror_url`]) or Node's archive URL.
    pub(crate) url: String,
    /// The digest it must have.
    pub(crate) expected: Expected,
    /// Where it goes; `<dest>.part` holds it while partial.
    pub(crate) dest: PathBuf,
    /// What its first bytes must be.
    pub(crate) magic: Magic,
    /// The most bytes it may have. A larger `Content-Length` (with the resumed bytes) is refused
    /// before anything is written, and a body that runs past it is cut off; either way the
    /// `.part` is deleted and the fetch is `Failed`.
    pub(crate) max_bytes: u64,
}

/// The HTTP client of one install: its proxy, its TLS roots and its timeouts.
#[derive(Debug)]
pub(crate) struct Fetcher {
    agent: ureq::Agent,
    data_dir: PathBuf,
}

impl Fetcher {
    /// A client for `agent`'s install. The proxy is `install.proxy`, else `system_proxy` (the
    /// OS's static setting, `host:port`, when `retry` asked for it), else none: ureq's own read
    /// of `HTTPS_PROXY` is turned off, since the caller has already chosen. The TLS roots are
    /// `install.ca_certs[agent]`'s certificates when it is set, else the OS's trust store. Every
    /// request and redirect must be https (ureq's `https_only`), and a mirror that is not https
    /// is refused here.
    pub(crate) fn new(
        install: &InstallConfig,
        agent: &AgentId,
        system_proxy: Option<&str>,
        data_dir: &Path,
    ) -> Result<Fetcher, FetchError> {
        let fail = |detail: &str| FetchError::new(FetchKind::Failed, None, None, detail, data_dir);
        if !data_dir.is_absolute() {
            return Err(fail("the data folder is not an absolute path"));
        }
        #[cfg(feature = "test-support")]
        let https_only = !install.allow_http_for_tests;
        #[cfg(not(feature = "test-support"))]
        let https_only = true;
        if https_only
            && let Some(mirror) = &install.registry
            && !is_https(mirror)
        {
            return Err(fail("the mirror setting is not an https:// URL"));
        }
        let proxy = match (install.proxy.as_deref(), system_proxy) {
            (Some(proxy), _) => Some(proxy.to_owned()),
            (None, Some(system)) if system.contains("://") => Some(system.to_owned()),
            (None, Some(system)) => Some(format!("http://{system}")),
            (None, None) => None,
        };
        let proxy = match proxy {
            Some(proxy) => Some(
                ureq::Proxy::new(&proxy)
                    .map_err(|err| fail(&format!("the proxy setting is not usable: {err}")))?,
            ),
            None => None,
        };
        let roots = match install.ca_certs.get(agent) {
            Some(file) => read_roots(file, agent).map_err(|detail| fail(&detail))?,
            None => RootCerts::PlatformVerifier,
        };
        let config = Config::builder()
            .http_status_as_error(false)
            .https_only(https_only)
            .max_redirects(MAX_REDIRECTS)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(READ_TIMEOUT))
            .timeout_per_read(Some(READ_TIMEOUT))
            .accept_encoding(AutoHeaderValue::None)
            .proxy(proxy)
            .tls_config(TlsConfig::builder().root_certs(roots).build())
            .build();
        #[cfg(feature = "test-support")]
        if let Some(resolver) = install.resolver.clone() {
            let agent = ureq::Agent::with_parts(
                config,
                ureq::unversioned::transport::DefaultConnector::new(),
                TestNames(resolver),
            );
            return Ok(Fetcher {
                agent,
                data_dir: data_dir.to_owned(),
            });
        }
        Ok(Fetcher {
            agent: ureq::Agent::new_with_config(config),
            data_dir: data_dir.to_owned(),
        })
    }

    /// Fetches `job`, returning the file's size. A destination already there with the right
    /// digest is kept with no request; one with the wrong digest is deleted. A `.part` is
    /// resumed with `Range` (a 206 appends after its bytes are hashed, a 200 starts over, a 416
    /// deletes it and starts over once). Each chunk is hashed and written; `cancel` is checked
    /// between chunks, and a pause keeps the `.part`. `progress(received, total)` follows each
    /// chunk, both counting the resumed bytes; `total` is 0 when the size is not known.
    pub(crate) fn fetch(
        &self,
        job: &FetchJob,
        cancel: &Cancel,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<u64, FetchError> {
        let host = host_of(&job.url);
        let at = Place {
            host: host.as_deref(),
            data_dir: &self.data_dir,
        };
        if let Some(size) = complete_size(&job.dest, &job.expected).map_err(|err| at.io(&err))? {
            progress(size, size);
            return Ok(size);
        }
        if let Some(parent) = job.dest.parent() {
            write::create_dir_all(parent).map_err(|err| at.io(&err))?;
        }
        let part = part_path(&job.dest);
        // A link where the `.part` goes is removed (the link, not its target); the opens below
        // refuse one that appears later.
        if std::fs::symlink_metadata(&part).is_ok_and(|meta| meta.file_type().is_symlink()) {
            write::remove_file(&part).map_err(|err| at.io(&err))?;
        }
        let mut restarted = false;
        loop {
            if cancel.is_cancelled() {
                return Err(at.cancelled());
            }
            let offset = std::fs::symlink_metadata(&part).map_or(0, |meta| meta.len());
            let mut request = self.agent.get(&job.url);
            if offset > 0 {
                request = request.header("Range", format!("bytes={offset}-"));
            }
            let response = request.call().map_err(|err| at.ureq(&err))?;
            let status = response.status().as_u16();
            match status {
                416 if offset > 0 && !restarted => {
                    restarted = true;
                    write::remove_file(&part).map_err(|err| at.io(&err))?;
                }
                200 => return receive(job, &part, response, 0, cancel, progress, &at),
                206 if offset > 0 => {
                    if !range_starts_at(&response, offset) {
                        write::remove_file(&part).map_err(|err| at.io(&err))?;
                        return Err(at.fail(
                            FetchKind::Failed,
                            Some(status),
                            "the server resumed the download at the wrong place",
                        ));
                    }
                    return receive(job, &part, response, offset, cancel, progress, &at);
                }
                _ => {
                    return Err(at.fail(
                        status_kind(status),
                        Some(status),
                        &format!("the server answered HTTP {status} instead of the file"),
                    ));
                }
            }
        }
    }
}

/// The host and data folder a fetch's errors are made with.
struct Place<'a> {
    host: Option<&'a str>,
    data_dir: &'a Path,
}

impl Place<'_> {
    fn fail(&self, kind: FetchKind, status: Option<u16>, detail: &str) -> FetchError {
        FetchError::new(kind, self.host, status, detail, self.data_dir)
    }

    fn ureq(&self, err: &ureq::Error) -> FetchError {
        let detail = format!("the download failed: {err}");
        self.fail(classify(err), error_status(err), &detail)
    }

    fn io(&self, err: &io::Error) -> FetchError {
        let detail = format!("the download could not be saved: {err}");
        self.fail(write_kind(err), None, &detail)
    }

    fn cancelled(&self) -> FetchError {
        self.fail(FetchKind::Cancelled, None, "the download was paused")
    }
}

/// Reads the body of a 200 (`offset` 0) or a 206 (after `offset` bytes already in `part`) into
/// `part`, then checks the digest and renames it to the destination.
fn receive(
    job: &FetchJob,
    part: &Path,
    response: ureq::http::Response<ureq::Body>,
    offset: u64,
    cancel: &Cancel,
    progress: &mut dyn FnMut(u64, u64),
    at: &Place<'_>,
) -> Result<u64, FetchError> {
    let status = Some(response.status().as_u16());
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    if header("content-type").is_some_and(|kind| is_html_type(&kind)) {
        return Err(at.fail(FetchKind::Blocked, status, HTML));
    }
    let total = header("content-length")
        .and_then(|length| length.trim().parse::<u64>().ok())
        .map_or(0, |length| length.saturating_add(offset));
    if total > job.max_bytes {
        write::remove_file(part).map_err(|err| at.io(&err))?;
        return Err(at.fail(FetchKind::Failed, status, &too_big(job, Some(total))));
    }
    let reader = response.into_body().into_reader();
    let source = Source {
        status,
        offset,
        total,
        read_error: &|err| at.ureq(&ureq::Error::from(err)),
    };
    write_checked(job, part, reader, &source, cancel, progress, at)
}

/// Where [`write_checked`]'s bytes come from: the HTTP status (none for a local file), the bytes
/// already in the `.part` that they follow, the size once known (0 when not), and how a read
/// error is reported.
struct Source<'a> {
    status: Option<u16>,
    offset: u64,
    total: u64,
    read_error: &'a dyn Fn(io::Error) -> FetchError,
}

/// Writes `reader`'s bytes into `part` (after the `source.offset` bytes already there, which
/// are hashed first), hashing each chunk as it is written, then checks the digest and renames
/// `part` to the destination. The magic is checked on a fresh file's first bytes, `cancel`
/// between chunks, and [`FetchJob::max_bytes`] before each write.
fn write_checked(
    job: &FetchJob,
    part: &Path,
    mut reader: impl io::Read,
    source: &Source<'_>,
    cancel: &Cancel,
    progress: &mut dyn FnMut(u64, u64),
    at: &Place<'_>,
) -> Result<u64, FetchError> {
    let (status, offset) = (source.status, source.offset);
    let mut hasher = job.expected.hasher();
    let mut file = if offset > 0 {
        hash_file(part, &mut hasher).map_err(|err| at.io(&err))?;
        write::open_append(part)
    } else {
        write::open_truncate(part)
    }
    .map_err(|err| at.io(&err))?;
    let mut buf = vec![0; CHUNK];
    // The first bytes of a fresh file, held until there are enough to check the magic.
    let mut head: Option<Vec<u8>> = (offset == 0).then(Vec::new);
    let mut received = offset;
    loop {
        if cancel.is_cancelled() {
            return Err(at.cancelled());
        }
        let n = match reader.read(&mut buf) {
            Ok(n) => n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err((source.read_error)(err)),
        };
        let chunk = buf.get(..n).unwrap_or_default();
        let chunk: Vec<u8> = match head.as_mut() {
            Some(held) => {
                held.extend_from_slice(chunk);
                if held.len() < 2 && n > 0 {
                    continue;
                }
                if let Err(kind) = check_head(held, job.magic) {
                    drop(file);
                    write::remove_file(part).map_err(|err| at.io(&err))?;
                    let detail = match kind {
                        FetchKind::Blocked => HTML.to_owned(),
                        _ => format!("the file is not a {} archive", job.magic.name()),
                    };
                    return Err(at.fail(kind, status, &detail));
                }
                head.take().unwrap_or_default()
            }
            None => chunk.to_vec(),
        };
        if chunk.is_empty() && n == 0 {
            break;
        }
        // A pause seen while the read waited: the chunk is dropped, the part stays as it was.
        if cancel.is_cancelled() {
            return Err(at.cancelled());
        }
        received = received.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if received > job.max_bytes {
            drop(file);
            write::remove_file(part).map_err(|err| at.io(&err))?;
            return Err(at.fail(FetchKind::Failed, status, &too_big(job, None)));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(|err| at.io(&err))?;
        progress(received, source.total);
        if n == 0 {
            break;
        }
    }
    file.flush().map_err(|err| at.io(&err))?;
    file.sync_all().map_err(|err| at.io(&err))?;
    drop(file);
    if !hasher.matches(&job.expected) {
        write::remove_file(part).map_err(|err| at.io(&err))?;
        return Err(at.fail(
            FetchKind::Integrity,
            status,
            "the file's checksum does not match the pinned one; it was deleted",
        ));
    }
    write::rename(part, &job.dest).map_err(|err| at.io(&err))?;
    Ok(received)
}

/// The detail for a file over its cap: the size offered when the server said it.
fn too_big(job: &FetchJob, offered: Option<u64>) -> String {
    let cap = job.max_bytes;
    match offered {
        Some(size) => format!(
            "the file would be {size} bytes, more than the {cap} it may have; the partial file \
             was deleted"
        ),
        None => {
            format!("the file ran past the {cap} bytes it may have; the partial file was deleted")
        }
    }
}

/// Whether `url` starts with `https://`, in any case.
fn is_https(url: &str) -> bool {
    url.trim()
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

/// The detail for a web page where a file should be.
const HTML: &str = "a web page came instead of the file (a proxy or firewall page?)";

/// `Err(Blocked)` for a page starting with `<` (after white space), `Err(Failed)` for any
/// other start that is not `magic`.
fn check_head(head: &[u8], magic: Magic) -> Result<(), FetchKind> {
    let text = head.trim_ascii_start();
    if text.first() == Some(&b'<') {
        Err(FetchKind::Blocked)
    } else if head.starts_with(magic.bytes()) {
        Ok(())
    } else {
        Err(FetchKind::Failed)
    }
}

/// Whether a `Content-Type` is HTML.
fn is_html_type(content_type: &str) -> bool {
    let essence = content_type.split(';').next().unwrap_or_default().trim();
    essence.eq_ignore_ascii_case("text/html")
        || essence.eq_ignore_ascii_case("application/xhtml+xml")
}

/// Whether a 206's `Content-Range` starts at `offset` (a 206 without one is taken at its word).
fn range_starts_at(response: &ureq::http::Response<ureq::Body>, offset: u64) -> bool {
    let Some(range) = response
        .headers()
        .get("content-range")
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    let start = range
        .trim()
        .strip_prefix("bytes ")
        .and_then(|rest| rest.split('-').next())
        .and_then(|start| start.trim().parse::<u64>().ok());
    start == Some(offset)
}

/// `<dest>.part`.
pub(crate) fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// The size of `dest` when it is there with the right digest; a wrong one is deleted, and so is
/// a link found there (the link itself, never what it points to).
fn complete_size(dest: &Path, expected: &Expected) -> io::Result<Option<u64>> {
    match std::fs::symlink_metadata(dest) {
        Ok(meta) if meta.file_type().is_symlink() => {
            write::remove_file(dest)?;
            return Ok(None);
        }
        Ok(meta) if meta.is_file() => {}
        _ => return Ok(None),
    }
    let mut hasher = expected.hasher();
    let size = hash_file(dest, &mut hasher)?;
    if hasher.matches(expected) {
        Ok(Some(size))
    } else {
        write::remove_file(dest)?;
        Ok(None)
    }
}

/// Adds the bytes of `path` to `hasher`, returning how many there were. A link is refused.
pub(crate) fn hash_file(path: &Path, hasher: &mut crate::integrity::Hasher) -> io::Result<u64> {
    hash_read(write::open_read(path)?, hasher)
}

/// Adds the bytes `file` has left to `hasher`, returning how many there were.
pub(crate) fn hash_read(mut file: File, hasher: &mut crate::integrity::Hasher) -> io::Result<u64> {
    let mut buf = vec![0; CHUNK];
    let mut size = 0u64;
    loop {
        match file.read(&mut buf) {
            Ok(0) => return Ok(size),
            Ok(n) => {
                hasher.update(buf.get(..n).unwrap_or_default());
                size = size.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
}

/// The certificates of `agent`'s PEM file, as the only TLS roots; an error line when it holds
/// none. The line names the agent, never the file's path.
fn read_roots(file: &Path, agent: &AgentId) -> Result<RootCerts, String> {
    let pem = std::fs::read(file)
        .map_err(|err| format!("the CA file for {agent} could not be read: {err}"))?;
    let mut certs = Vec::new();
    for item in ureq::tls::parse_pem(&pem) {
        match item {
            Ok(PemItem::Certificate(cert)) => certs.push(cert),
            Ok(_) => {}
            Err(err) => return Err(format!("the CA file for {agent} is not PEM: {err}")),
        }
    }
    if certs.is_empty() {
        return Err(format!("the CA file for {agent} holds no certificate"));
    }
    Ok(RootCerts::new_with_certs(&certs))
}

/// Tests only: [`crate::TestResolver`] as ureq's resolver. Its `Err` is a failed name lookup.
/// An address with port 0 takes the URL's port; any other port is kept, so a test can send a
/// real host name to a loopback port.
#[cfg(feature = "test-support")]
#[derive(Debug)]
struct TestNames(crate::TestResolver);

#[cfg(feature = "test-support")]
impl ureq::unversioned::resolver::Resolver for TestNames {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let (_, _) = (config, timeout);
        let host = uri.host().ok_or(ureq::Error::HostNotFound)?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let port = uri
            .port_u16()
            .unwrap_or(if uri.scheme_str() == Some("https") {
                443
            } else {
                80
            });
        let addrs = (self.0.0)(host).map_err(|_offline| ureq::Error::HostNotFound)?;
        let mut out = self.empty();
        for addr in addrs.into_iter().take(16) {
            let port = if addr.port() == 0 { port } else { addr.port() };
            out.push(std::net::SocketAddr::new(addr.ip(), port));
        }
        if out.is_empty() {
            Err(ureq::Error::HostNotFound)
        } else {
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests;
