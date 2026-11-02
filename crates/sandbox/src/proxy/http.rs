//! Plain `http://`: one request per client connection, rewritten for the origin (or sent
//! absolute-form to the upstream), its body copied unchanged up to its end, and the response
//! piped back raw. Through an upstream the response's status is read first, so the upstream's
//! own 407 becomes a 502 and `Denial::UpstreamAuth`, as for CONNECT.

use std::io;
use std::pin::pin;

use futures::AsyncWriteExt;
use futures::future::{Either, pending, select};

use crate::proxy::Verdict;
use crate::proxy::connect::{dial, failed, send};
use crate::proxy::head::{RESPONSE_HEAD_CAP, Request, parse_status, read_head_capped};
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
    let (origin, _slot) = match dial(ctx, &way).await {
        Ok(dialled) => dialled,
        Err(status) => {
            send(client, status).await;
            return failed(status);
        }
    };
    let head = rewrite(request, target, url, through_upstream);
    if (&origin).write_all(&head).await.is_err() {
        send(client, 502).await;
        return Verdict::Failed;
    }
    // Ends only when the body fails: true when its framing was malformed.
    let body = async {
        if let Err(error) =
            copy_body(framing, early, client, &origin, &counts.up, &counts.idle).await
        {
            tracing::debug!(%error, "egress: request body ended early");
            return error.kind() == io::ErrorKind::InvalidData;
        }
        // The body is complete: wait for the response, reading nothing more from the client.
        pending::<bool>().await
    };
    let response = async {
        if let Way::Upstream { host, port } = &way {
            let mut reader = &origin;
            let Ok(head) = read_head_capped(&mut reader, RESPONSE_HEAD_CAP).await else {
                return Answered::Failed;
            };
            if parse_status(&head.bytes) == Some(407) {
                return Answered::Auth(format!("{host}:{port}"));
            }
            for bytes in [&head.bytes, &head.rest] {
                if (&*client).write_all(bytes).await.is_err() {
                    return Answered::Relayed;
                }
                counts
                    .down
                    .set(counts.down.get().saturating_add(bytes.len() as u64));
                counts.idle.touch();
            }
        }
        if let Err(error) = pump(&origin, client, &counts.down, &counts.idle).await {
            tracing::debug!(%error, "egress: response ended with an error");
        }
        Answered::Relayed
    };
    let (body, response) = (pin!(body), pin!(response));
    let relay = select(body, response);
    match select(relay, pin!(counts.idle.expired(ctx.limits.idle))).await {
        Either::Left((Either::Left((malformed, _)), _)) => {
            // A malformed chunked body gets 400 if no byte of a response has gone back yet.
            if malformed && counts.down.get() == 0 {
                send(client, 400).await;
                return Verdict::BadRequest;
            }
        }
        Either::Left((Either::Right((Answered::Relayed, _)), _)) => {}
        Either::Left((Either::Right((Answered::Auth(upstream), _)), _)) => {
            ctx.upstream_auth(upstream);
            send(client, 502).await;
            return Verdict::UpstreamAuth;
        }
        Either::Left((Either::Right((Answered::Failed, _)), _)) => {
            tracing::info!("egress: the upstream sent no response head");
            send(client, 502).await;
            return Verdict::Failed;
        }
        Either::Right(((), _)) => {
            tracing::info!("egress: forwarded request closed after its idle timeout");
        }
    }
    Verdict::Forward
}

/// How the response side of a forwarded request ended.
enum Answered {
    /// Relayed to the client, to its end or until a side failed.
    Relayed,
    /// The upstream proxy answered 407: it wants credentials Clauseo does not have.
    Auth(String),
    /// The upstream proxy sent no head the proxy could read.
    Failed,
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
