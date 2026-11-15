//! Plain `http://`: one request per client connection, rewritten for the origin (or sent
//! absolute-form to the upstream), its body copied unchanged up to its end, and the response
//! piped back raw. Through an upstream the response's status is read first, so the upstream's
//! own 407 becomes a 502 and `Denial::UpstreamAuth`, as for CONNECT. Straight to an origin the
//! status line is read first too, and an origin's 407 becomes a 502: a proxy challenge from a
//! site must never reach the browser, which would answer it with the proxy's credential. Either
//! way a response whose status line is not strictly `HTTP/1.[01] NNN` becomes a 502 too, since
//! Chrome reads looser forms and could take one for a challenge ([`status_code`]).

use std::io;
use std::pin::pin;

use futures::future::{Either, pending, select};
use futures::{AsyncReadExt, AsyncWriteExt};

use crate::proxy::Verdict;
use crate::proxy::connect::{dial, failed};
use crate::proxy::head::{RESPONSE_HEAD_CAP, Request, read_head_capped};
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
            ctx.fail(client, status, "").await;
            return failed(status);
        }
    };
    let head = rewrite(request, target, url, through_upstream);
    if (&origin).write_all(&head).await.is_err() {
        ctx.fail(client, 502, "").await;
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
        if matches!(way, Way::Direct(_)) {
            let start = match status_start(&origin).await {
                Ok(start) => start,
                Err(error) => {
                    tracing::debug!(%error, "egress: response ended with an error");
                    return Answered::Relayed;
                }
            };
            match status_code(&start) {
                Some(407) => return Answered::OriginAuth,
                None => return Answered::BadStatus,
                Some(_) => {}
            }
            if (&*client).write_all(&start).await.is_err() {
                return Answered::Relayed;
            }
            counts
                .down
                .set(counts.down.get().saturating_add(start.len() as u64));
            counts.idle.touch();
        }
        if let Way::Upstream { host, port } = &way {
            let mut reader = &origin;
            let Ok(head) = read_head_capped(&mut reader, RESPONSE_HEAD_CAP).await else {
                return Answered::Failed;
            };
            match status_code(&head.bytes) {
                Some(407) => return Answered::Auth(format!("{host}:{port}")),
                None => return Answered::BadStatus,
                Some(_) => {}
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
                ctx.fail(client, 400, "").await;
                return Verdict::BadRequest;
            }
        }
        Either::Left((Either::Right((Answered::Relayed, _)), _)) => {}
        Either::Left((Either::Right((Answered::Auth(upstream), _)), _)) => {
            ctx.upstream_auth(upstream);
            ctx.fail(client, 502, "").await;
            return Verdict::UpstreamAuth;
        }
        Either::Left((Either::Right((Answered::OriginAuth, _)), _)) => {
            tracing::info!("egress: the origin asked for proxy credentials; answered 502");
            ctx.fail(client, 502, "").await;
            return Verdict::Failed;
        }
        Either::Left((Either::Right((Answered::BadStatus, _)), _)) => {
            tracing::info!("egress: the response's status line was not strict; answered 502");
            ctx.fail(client, 502, "").await;
            return Verdict::Failed;
        }
        Either::Left((Either::Right((Answered::Failed, _)), _)) => {
            tracing::info!("egress: the upstream sent no response head");
            ctx.fail(client, 502, "").await;
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
    /// The origin answered 407, a proxy challenge it has no business sending.
    OriginAuth,
    /// The response did not start with a strict status line ([`status_code`]).
    BadStatus,
}

/// How many bytes hold an origin's status code and the byte after it: `HTTP/1.x NNN `.
const STATUS_START: usize = 13;

/// The start of an origin's response: read until there are at least [`STATUS_START`] bytes, a
/// line end, or the origin's end, whichever comes first. The caller passes all of it on unless
/// the status is 407 or the status line is not strict ([`status_code`]).
async fn status_start(origin: &Stream) -> io::Result<Vec<u8>> {
    let mut start = Vec::new();
    let mut chunk = [0u8; 2048];
    while start.len() < STATUS_START && !start.contains(&b'\n') {
        let n = (&*origin).read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        start.extend_from_slice(chunk.get(..n).unwrap_or_default());
    }
    Ok(start)
}

/// The status code of a response that starts with a strict status line: `HTTP/1.1` or
/// `HTTP/1.0`, one space, three digits, then a space, a line end or the response's end.
/// `None` for anything else, which the proxy refuses with its own 502: Chrome reads looser
/// forms (another version, more spaces, lower case, junk before `HTTP`, no status line) and
/// could take one for this proxy's challenge, which it would answer with the credential.
pub(super) fn status_code(start: &[u8]) -> Option<u16> {
    let rest = start
        .strip_prefix(b"HTTP/1.1 ")
        .or_else(|| start.strip_prefix(b"HTTP/1.0 "))?;
    let digits = rest.get(..3)?;
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if !matches!(rest.get(3), None | Some(b' ' | b'\r' | b'\n')) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
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

#[cfg(test)]
mod tests {
    use super::status_code;

    #[test]
    fn only_a_strict_status_line_has_a_code() {
        let strict: [(&[u8], u16); 5] = [
            (b"HTTP/1.1 200 OK\r\n", 200),
            (b"HTTP/1.0 407 Proxy Authentication Required\r\n", 407),
            (b"HTTP/1.1 204\r\n", 204),
            (b"HTTP/1.1 302\n", 302),
            (b"HTTP/1.1 500", 500),
        ];
        for (start, code) in strict {
            assert_eq!(status_code(start), Some(code), "{start:?}");
        }
        let loose: [&[u8]; 13] = [
            b"HTTP/1.1  407 Two spaces\r\n",
            b"http/1.1 407 Lower case\r\n",
            b"HTTP/1.2 407 Other version\r\n",
            b"HTTP/2 407\r\n",
            b"HTTP/1.1\t407 Tab\r\n",
            b" HTTP/1.1 407 Leading space\r\n",
            b"xHTTP/1.1 407 Junk first\r\n",
            b"HTTP/1.1 4071 Four digits\r\n",
            b"HTTP/1.1 40 Two digits\r\n",
            b"HTTP/1.1 4o7 Letter\r\n",
            b"HTTP/1.1 407x\r\n",
            b"<html>no status line</html>",
            b"",
        ];
        for start in loose {
            assert_eq!(status_code(start), None, "{start:?}");
        }
    }
}
