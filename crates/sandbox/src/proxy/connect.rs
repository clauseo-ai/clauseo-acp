//! Connecting: to the checked addresses of an origin or to the upstream proxy, and the CONNECT
//! tunnel, through the upstream when there is one. Tunnel bytes are copied, never parsed.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::pin;
use std::time::Duration;

use futures::AsyncWriteExt;
use futures::future::{Either, select, try_join};

use crate::proxy::Verdict;
use crate::proxy::head::{self, read_head, within};
use crate::proxy::relay::{Stream, pump};
use crate::proxy::server::{Counts, Ctx, SocketSlot, Way};
use crate::proxy::target::Target;

/// Opens the connection `way` names, under the connect timeout, and counts it for
/// reachability. The socket counts against the proxy's and the process's caps while its slot
/// lives. `Err` is the status to answer: 502, 503 when the caps are full, or 504 on the
/// timeout.
pub(crate) async fn dial(ctx: &Ctx, way: &Way) -> Result<(Stream, SocketSlot), u16> {
    let Some(slot) = ctx.socket_slot() else {
        tracing::info!("egress: too many open sockets; no origin connection");
        return Err(503);
    };
    let attempt = async {
        let addresses = match way {
            Way::Direct(addresses) => addresses.clone(),
            Way::Upstream { host, port } => upstream_addresses(ctx, host, *port).await?,
        };
        let mut last = io::Error::new(io::ErrorKind::NotFound, "no address");
        for address in addresses {
            match ctx.net.connect(address).await {
                Ok(stream) => return Ok(stream),
                Err(error) => last = error,
            }
        }
        Err(last)
    };
    match within(ctx.limits.connect, attempt).await {
        Some(Ok(stream)) => {
            ctx.reached(true);
            Ok((stream, slot))
        }
        Some(Err(error)) => {
            tracing::info!(%error, "egress: connect failed");
            ctx.reached(false);
            Err(502)
        }
        None => {
            tracing::info!("egress: connect timed out");
            ctx.reached(false);
            Err(504)
        }
    }
}

/// The upstream proxy's addresses. The firm's proxy is trusted configuration, so it is not
/// address-checked; only the origin behind it is (an IP literal, before this).
async fn upstream_addresses(ctx: &Ctx, host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    ctx.net.resolve(bare, port).await
}

/// The verdict for a failed dial's status.
pub(crate) fn failed(status: u16) -> Verdict {
    if status == 503 {
        Verdict::Busy
    } else {
        Verdict::Failed
    }
}

/// Writes the proxy's own answer; a client that already left is fine.
pub(crate) async fn send(client: &Stream, status: u16) {
    if let Err(error) = (&*client).write_all(&head::answer(status, "")).await {
        tracing::debug!(%error, "egress: the client left before the answer");
    }
}

/// A CONNECT whose checks passed: connect (or ask the upstream for a tunnel), answer 200, pass
/// on the bytes the client sent early, then copy both ways until both sides end.
pub(crate) async fn tunnel(
    ctx: &Ctx,
    client: &Stream,
    early: &[u8],
    target: &Target,
    way: Way,
    counts: &Counts,
) -> Verdict {
    let (origin, _slot) = match dial(ctx, &way).await {
        Ok(dialled) => dialled,
        Err(status) => {
            send(client, status).await;
            return failed(status);
        }
    };
    if let Way::Upstream { host, port } = &way {
        match ask_upstream(ctx, &origin, target).await {
            Asked::Open(late) => {
                send(client, 200).await;
                if relay_bytes(client, &late, &counts.down).await.is_err() {
                    return Verdict::Tunnel;
                }
            }
            Asked::Auth => {
                ctx.upstream_auth(format!("{host}:{port}"));
                send(client, 502).await;
                return Verdict::UpstreamAuth;
            }
            Asked::Failed => {
                send(client, 502).await;
                return Verdict::Failed;
            }
        }
    } else {
        send(client, 200).await;
    }
    if relay_bytes(&origin, early, &counts.up).await.is_err() {
        return Verdict::Tunnel;
    }
    splice(client, &origin, counts, ctx.limits.idle).await;
    Verdict::Tunnel
}

/// Copies both ways until both ends close, either fails, or `idle` passes with no byte moved
/// either way. One side ending its writes is passed on, and the other way keeps flowing.
pub(crate) async fn splice(client: &Stream, origin: &Stream, counts: &Counts, idle: Duration) {
    let both = try_join(
        pump(client, origin, &counts.up, &counts.idle),
        pump(origin, client, &counts.down, &counts.idle),
    );
    match select(pin!(both), pin!(counts.idle.expired(idle))).await {
        Either::Left((Ok(_), _)) => {}
        Either::Left((Err(error), _)) => {
            tracing::debug!(%error, "egress: tunnel ended with an error");
        }
        Either::Right(((), _)) => tracing::info!("egress: tunnel closed after its idle timeout"),
    }
}

/// Writes bytes already read to `to`, counting them.
async fn relay_bytes(to: &Stream, bytes: &[u8], count: &std::cell::Cell<u64>) -> io::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    (&*to).write_all(bytes).await?;
    count.set(count.get().saturating_add(bytes.len() as u64));
    Ok(())
}

/// The upstream's answer to a CONNECT.
enum Asked {
    /// 200, with any bytes it sent after its head.
    Open(Vec<u8>),
    /// 407: it wants credentials Clauseo does not have.
    Auth,
    /// Anything else, a head it could not read, or no head in time.
    Failed,
}

/// Sends `CONNECT host:443` with `Host` and no credentials, and reads the answer's head.
async fn ask_upstream(ctx: &Ctx, upstream: &Stream, target: &Target) -> Asked {
    let authority = target.name.authority(target.port);
    let ask = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n");
    if (&*upstream).write_all(ask.as_bytes()).await.is_err() {
        return Asked::Failed;
    }
    let mut reader = upstream;
    let Some(Ok(answer)) = within(ctx.limits.connect, read_head(&mut reader)).await else {
        return Asked::Failed;
    };
    match head::parse_status(&answer.bytes) {
        Some(200) => Asked::Open(answer.rest),
        Some(407) => Asked::Auth,
        Some(status) => {
            tracing::info!(status, "egress: the upstream refused the tunnel");
            Asked::Failed
        }
        None => Asked::Failed,
    }
}
