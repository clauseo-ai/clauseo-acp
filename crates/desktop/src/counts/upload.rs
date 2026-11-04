//! The counts' upload: `clauseo_counts::Upload` over `reqwest` (rustls with ring, and the
//! OS's certificate check through `rustls-platform-verifier`), the one place desktop
//! sends counts off the machine (decision record 0036, "The desktop").
//!
//! - HTTPS only (`https_only`, and an endpoint that is not `https://` is none), no redirect
//!   followed, no cookie store, no `Referer`.
//! - Each POST names `User-Agent: Clauseo`, `Content-Type: application/json` and
//!   `Content-Encoding: gzip`, and nothing else. reqwest adds `Accept: */*` to every request,
//!   and hyper `Host` and `Content-Length`; none carries anything of the lawyer's.
//! - The proxy is the system's: reqwest's `system-proxy` (on by default; `no_proxy` is never
//!   called) reads the `HTTPS_PROXY` family of variables, then macOS's network settings
//!   (`SCDynamicStoreCopyProxies`) or Windows' per-user Internet Settings (`ProxyEnable`,
//!   `ProxyServer`, `ProxyOverride`), through hyper-util's `client-proxy-system`. No PAC file
//!   (Later, G10). Never the agents' egress proxy: that goes to agents in their own environment,
//!   never into this process's.
//! - The endpoint: release builds post to [`RELEASE_ENDPOINT`], which is empty until S23's live
//!   checks pass, so they send nothing. Debug and `e2e` builds read [`ENDPOINT_VAR`] instead,
//!   and have none without it; release builds never read it. With no endpoint every POST fails
//!   without touching the network, and the days wait in the queue until they age out.

use std::time::Duration;

use clauseo_counts::{Upload, UploadError, UploadPath};
use futures::future::BoxFuture;
use reqwest::header::{CONTENT_ENCODING, CONTENT_TYPE, USER_AGENT};

/// The release builds' endpoint. Empty until a follow-up pull request, made after S23's live
/// checks pass, sets it to `https://t.clauseo.chat`.
pub(crate) const RELEASE_ENDPOINT: &str = "";

/// The variable debug and `e2e` builds read their endpoint from.
pub(crate) const ENDPOINT_VAR: &str = "CLAUSEO_COUNTS_URL";

/// How long a POST may take, connecting included. The sender counts one with no answer after
/// 10 minutes as a failed try anyway.
const TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// The upload over HTTPS.
#[derive(Debug)]
pub(crate) struct HttpsUpload {
    /// None when it could not be built (its TLS set-up failed): nothing is sent.
    client: Option<reqwest::Client>,
    /// `https://host[:port]`, with no path; none sends nothing.
    endpoint: Option<String>,
}

impl HttpsUpload {
    /// The upload to this build's endpoint ([`build_endpoint`]).
    pub(crate) fn new() -> HttpsUpload {
        HttpsUpload::to(build_endpoint().as_deref())
    }

    /// The upload to `endpoint`, when it is one ([`endpoint`]).
    pub(crate) fn to(endpoint: Option<&str>) -> HttpsUpload {
        // reqwest's rustls takes the process's provider: ring, as tauri-plugin-updater installs
        // it. Either may be first; the other finds it in place.
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            drop(rustls::crypto::ring::default_provider().install_default());
        }
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .timeout(TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|err| tracing::warn!(%err, "no usage counts upload: the counts are kept and not sent"))
            .ok();
        HttpsUpload {
            client,
            endpoint: endpoint.and_then(self::endpoint),
        }
    }

    /// The POST of `gzip_body` to `path`, as it goes; none with no endpoint.
    fn request(&self, path: UploadPath, gzip_body: Vec<u8>) -> Option<reqwest::Request> {
        let endpoint = self.endpoint.as_ref()?;
        self.client
            .as_ref()?
            .post(format!("{endpoint}{}", path.as_str()))
            .header(USER_AGENT, "Clauseo")
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_ENCODING, "gzip")
            .body(gzip_body)
            .build()
            .ok()
    }
}

impl Upload for HttpsUpload {
    fn post(
        &self,
        path: UploadPath,
        gzip_body: Vec<u8>,
    ) -> BoxFuture<'_, Result<u16, UploadError>> {
        let request = self.request(path, gzip_body);
        Box::pin(async move {
            let (Some(client), Some(request)) = (self.client.as_ref(), request) else {
                return Err(UploadError);
            };
            // The answer's status alone: no header or body of it is read.
            let response = client
                .execute(request)
                .await
                .map_err(|_network| UploadError)?;
            Ok(response.status().as_u16())
        })
    }
}

/// This build's endpoint: [`ENDPOINT_VAR`] in debug and `e2e` builds, [`RELEASE_ENDPOINT`] in
/// release builds.
fn build_endpoint() -> Option<String> {
    if cfg!(any(debug_assertions, feature = "e2e")) {
        std::env::var(ENDPOINT_VAR).ok()
    } else {
        Some(RELEASE_ENDPOINT.to_owned())
    }
}

/// `text` as an endpoint: an `https` URL with a host, no user, password, query or fragment, and
/// no path but `/`; as `https://host[:port]`. Anything else, the empty string included, is none.
pub(crate) fn endpoint(text: &str) -> Option<String> {
    let url = url::Url::parse(text).ok()?;
    let plain = url.scheme() == "https"
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path() == "/";
    plain.then(|| url.origin().ascii_serialization())
}

#[cfg(test)]
mod tests;
