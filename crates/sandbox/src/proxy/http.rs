//! Plain `http://`: one request per client connection, rewritten for the origin (or sent
//! absolute-form to the upstream), its body copied unchanged up to its end, and the response
//! piped back raw.

use std::pin::pin;

use futures::AsyncWriteExt;
use futures::future::{Either, pending, select};

use crate::proxy::Verdict;
use crate::proxy::connect::{dial, send};
use crate::proxy::head::Request;
use crate::proxy::relay::{Framing, Stream, copy_body, pump};
use crate::proxy::server::{Counts, Ctx, Way};
use crate::proxy::target::Target;

/// Headers never forwarded: hop-by-hop ones, the proxy's own credential, and `Host`, which the
/// proxy writes from the URL so the origin serves the site that was checked.
const DROPPED: &[&str] = &[
    "host",
    "proxy-authorization",
    "proxy-authenticate",
    "proxy-connection",
    "connection",
    "keep-alive",
    "te",
    "trailer",
    "upgrade",
];

/// Forwards one request whose checks passed and pipes the response back.
pub(crate) async fn forward(
    ctx: &Ctx,
    client: &Stream,
    request: &Request,
    early: &[u8],
    target: &Target,
    url: &url::Url,
    framing: Framing,
    way: Way,
    counts: &Counts,
) -> Verdict {
    let through_upstream = matches!(way, Way::Upstream { .. });
    let origin = match dial(ctx, &way).await {
        Ok(stream) => stream,
        Err(status) => {
            send(client, status).await;
            return Verdict::Failed;
        }
    };
    let head = rewrite(request, target, url, through_upstream);
    if (&origin).write_all(&head).await.is_err() {
        send(client, 502).await;
        return Verdict::Failed;
    }
    let body = async {
        if let Err(error) = copy_body(framing, early, client, &origin, &counts.up).await {
            tracing::debug!(%error, "egress: request body ended early");
            return;
        }
        // The body is complete: wait for the response, reading nothing more from the client.
        pending::<()>().await;
    };
    let response = pump(&origin, client, &counts.down);
    match select(pin!(body), pin!(response)).await {
        Either::Left(((), _)) | Either::Right((Ok(()), _)) => {}
        Either::Right((Err(error), _)) => {
            tracing::debug!(%error, "egress: response ended with an error");
        }
    }
    Verdict::Forward
}

/// The head the origin (or the upstream) gets: origin-form (absolute-form through an
/// upstream), `Host` from the URL, the client's other end-to-end headers in order, and
/// `Connection: close`.
fn rewrite(request: &Request, target: &Target, url: &url::Url, absolute: bool) -> Vec<u8> {
    let mut path = url.path().to_owned();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    let host = target.name.host_header();
    let line_target = if absolute {
        format!("http://{host}{path}")
    } else {
        path
    };
    let named = connection_names(request);
    let mut head = format!(
        "{} {line_target} HTTP/1.1\r\nHost: {host}\r\n",
        request.method
    )
    .into_bytes();
    for (name, value) in &request.headers {
        let lower = name.to_ascii_lowercase();
        if DROPPED.contains(&lower.as_str()) || named.contains(&lower) {
            continue;
        }
        head.extend_from_slice(name.as_bytes());
        head.extend_from_slice(b": ");
        head.extend_from_slice(value);
        head.extend_from_slice(b"\r\n");
    }
    head.extend_from_slice(b"Connection: close\r\n\r\n");
    head
}

/// The header names the client's `Connection` headers list, lowercase.
fn connection_names(request: &Request) -> Vec<String> {
    request
        .values("connection")
        .iter()
        .filter_map(|value| std::str::from_utf8(value).ok())
        .flat_map(|value| value.split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
}
