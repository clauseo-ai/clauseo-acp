//! The proxy thread: the executor, the two accept loops, the rate cap's tick, and each request's
//! checks up to the connect.

use std::cell::{Cell, RefCell};
use std::net::{SocketAddr, TcpListener};
use std::pin::pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use async_executor::LocalExecutor;
use async_io::{Async, Timer};
use futures::channel::oneshot;
use futures::future::{Abortable, join3, select};
use futures::{AsyncWriteExt, StreamExt};

use crate::proxy::address::is_public;
use crate::proxy::allowlist::Name;
use crate::proxy::head::{self, Head, HeadError, ParseError, Request, within};
use crate::proxy::load::{Load, PendingSlot, SocketSlot};
use crate::proxy::net::Network;
use crate::proxy::relay::{Idle, Stream, linger};
use crate::proxy::target::{Kind, Target, classify};
use crate::proxy::token::Token;
use crate::proxy::upstream::{Lookup, Route, Scheme};
use crate::proxy::{
    Allowlist, Exchange, Limits, Listener, Record, Shared, SharedClock, Unopened, Verdict, connect,
    http,
};
use crate::{CappedSink, Denial, DenialSink, HostRefusal, Reachability};

/// Failures in a row before the proxy reports the network unreachable.
const FAILURES_UNREACHABLE: u32 = 3;
/// The pause after a failed accept (out of file descriptors), so the loop does not spin.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// What the proxy thread gets from `start_with`.
pub(crate) struct Parts {
    pub(crate) shared: Arc<Shared>,
    pub(crate) silent: Allowlist,
    pub(crate) token: Token,
    pub(crate) denials: Arc<dyn DenialSink>,
    pub(crate) clock: SharedClock,
    pub(crate) net: Arc<dyn Network>,
    pub(crate) lookup: Arc<Lookup>,
    pub(crate) record: Option<Record>,
    pub(crate) limits: Limits,
}

/// One proxy's state on its thread.
pub(crate) struct Ctx {
    pub(crate) shared: Arc<Shared>,
    silent: Allowlist,
    token: Token,
    sink: CappedSink<Arc<dyn DenialSink>, SharedClock>,
    pub(crate) net: Arc<dyn Network>,
    lookup: Arc<Lookup>,
    record: Option<Record>,
    pub(crate) limits: Limits,
    reach: RefCell<Reach>,
    pub(crate) load: Rc<Load>,
}

/// The connectivity count: no state until the first connect.
#[derive(Debug, Default)]
struct Reach {
    state: Option<Reachability>,
    failures: u32,
}

/// Runs the proxy until `stopped` fires (the handle dropped). Dropping the executor then drops
/// every task, which closes every client and origin socket; the listeners close with it.
pub(crate) fn run(
    parts: Parts,
    agent: &Async<TcpListener>,
    browser: &Async<TcpListener>,
    stopped: oneshot::Receiver<()>,
) {
    let ctx = Rc::new(Ctx {
        shared: parts.shared,
        silent: parts.silent,
        token: parts.token,
        sink: CappedSink::with_clock(parts.denials, parts.clock),
        net: parts.net,
        lookup: parts.lookup,
        record: parts.record,
        limits: parts.limits,
        reach: RefCell::new(Reach::default()),
        load: Rc::new(Load::default()),
    });
    let executor = LocalExecutor::new();
    async_io::block_on(executor.run(async {
        let work = join3(
            accept(&executor, &ctx, agent, Listener::Agent),
            accept(&executor, &ctx, browser, Listener::Browser),
            ticks(&ctx),
        );
        select(pin!(work), stopped).await;
    }));
}

#[expect(
    clippy::infinite_loop,
    reason = "accepts until the proxy stops, which drops it"
)]
async fn accept(
    executor: &LocalExecutor<'_>,
    ctx: &Rc<Ctx>,
    listener: &Async<TcpListener>,
    which: Listener,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                // Counted here, before the task runs, so a burst of queued connections cannot
                // pass the caps; the slots drop when the connection's task ends.
                let Some(pending) = ctx.load.pending(&ctx.limits, which) else {
                    tracing::debug!("egress: too many connections reading a head; closed one");
                    continue;
                };
                let Some(socket) = ctx.load.socket(&ctx.limits) else {
                    tracing::debug!("egress: too many open sockets; closed a client connection");
                    continue;
                };
                executor
                    .spawn(serve(Rc::clone(ctx), stream, which, socket, pending))
                    .detach();
            }
            Err(error) => {
                tracing::warn!(%error, "egress: accept failed");
                Timer::after(ACCEPT_BACKOFF).await;
            }
        }
    }
}

/// Delivers the rate cap's held count about once a second.
async fn ticks(ctx: &Ctx) {
    let mut timer = Timer::interval(crate::DENIAL_WINDOW);
    while timer.next().await.is_some() {
        ctx.sink.tick();
    }
}

/// One client connection: one request, then the tunnel or the forwarded exchange.
async fn serve(
    ctx: Rc<Ctx>,
    client: Stream,
    which: Listener,
    socket: SocketSlot,
    pending: PendingSlot,
) {
    let mut exchange = Exchange {
        listener: which,
        host: String::new(),
        port: 0,
        verdict: Verdict::BadRequest,
        why: None,
        bytes_up: 0,
        bytes_down: 0,
    };
    let answered = handle(&ctx, &client, &mut exchange, pending).await;
    drop(client);
    drop(socket);
    if answered && let Some(record) = &ctx.record {
        record(&exchange);
    }
}

/// Where the connect goes once the address check passed.
pub(crate) enum Way {
    /// To the origin, at one of these checked addresses, in order.
    Direct(Vec<SocketAddr>),
    /// To the upstream proxy at `host:port`, which gets the name unresolved.
    Upstream { host: String, port: u16 },
}

/// Bytes moved, readable after the relay ends however it ends.
#[derive(Default)]
pub(crate) struct Counts {
    pub(crate) up: Cell<u64>,
    pub(crate) down: Cell<u64>,
    /// When a byte last moved either way.
    pub(crate) idle: Idle,
}

/// Runs one request. False when the client went away before a head: nothing to record.
async fn handle(ctx: &Ctx, client: &Stream, exchange: &mut Exchange, pending: PendingSlot) -> bool {
    let mut reader = client;
    let read = within(ctx.limits.head, head::read_head(&mut reader)).await;
    drop(pending);
    let head = match read {
        Some(Ok(head)) => head,
        Some(Err(HeadError::TooLarge)) => {
            ctx.answer(client, exchange, 431, Verdict::BadRequest, "")
                .await;
            return true;
        }
        Some(Err(HeadError::Closed)) => return false,
        Some(Err(HeadError::Io(error))) => {
            tracing::debug!(%error, "egress: client read failed");
            return false;
        }
        None => {
            ctx.answer(client, exchange, 408, Verdict::BadRequest, "")
                .await;
            return true;
        }
    };
    let request = match head::parse_request(&head.bytes) {
        Ok(request) => request,
        Err(ParseError::TooLarge) => {
            ctx.answer(client, exchange, 431, Verdict::BadRequest, "")
                .await;
            return true;
        }
        Err(ParseError::Malformed) => {
            ctx.answer(client, exchange, 400, Verdict::BadRequest, "")
                .await;
            return true;
        }
    };
    if exchange.listener == Listener::Agent
        && !ctx.token.admits(&request.values("proxy-authorization"))
    {
        // The log names the host asked for; nothing is checked or reported for it.
        match classify(&request) {
            Ok(target) => {
                exchange.host = target.name.text();
                exchange.port = target.port;
            }
            Err(refusal) => {
                exchange.host = refusal.host;
                exchange.port = refusal.port;
            }
        }
        let challenge = "Proxy-Authenticate: Basic realm=\"clauseo\"\r\n";
        ctx.answer(client, exchange, 407, Verdict::AuthRequired, challenge)
            .await;
        return true;
    }
    match ctx.check(&request, exchange) {
        Ok(target) => ctx.carry(client, request, head, target, exchange).await,
        Err(status) => {
            ctx.answer(client, exchange, status, exchange.verdict, "")
                .await;
        }
    }
    true
}

impl Ctx {
    /// The checks that need no network: form and scheme, the silent list, port and the
    /// allowlist. Fills the exchange's host, port and verdict; `Err` is the status to answer.
    fn check(&self, request: &Request, exchange: &mut Exchange) -> Result<Target, u16> {
        let target = classify(request).map_err(|refusal| {
            exchange.host.clone_from(&refusal.host);
            exchange.port = refusal.port;
            match refusal.why {
                Some(why) => self.refuse(exchange, why, false),
                None => exchange.verdict = Verdict::BadRequest,
            }
            refusal.status
        })?;
        exchange.host = target.name.text();
        exchange.port = target.port;
        if self.silent.contains_name(&target.name) {
            exchange.verdict = Verdict::Silent;
            tracing::info!(host = %target.name, "egress: refused (silent list)");
            return Err(403);
        }
        let required = if matches!(target.kind, Kind::Connect) {
            443
        } else {
            80
        };
        if target.port != required {
            self.refuse(exchange, HostRefusal::Port, false);
            return Err(403);
        }
        if !self.shared.live().allow.contains_name(&target.name) {
            let shared = target.name.is_shared_hosting();
            self.refuse(exchange, HostRefusal::NotAllowed, shared);
            return Err(403);
        }
        Ok(target)
    }

    /// The route, the address check, the tunnel slot, then the relay.
    async fn carry(
        &self,
        client: &Stream,
        request: Request,
        head: Head,
        target: Target,
        exchange: &mut Exchange,
    ) {
        let route = {
            let lookup = Arc::clone(&self.lookup);
            let host = target.name.text();
            let port = target.port;
            let scheme = if matches!(target.kind, Kind::Connect) {
                Scheme::Https
            } else {
                Scheme::Http
            };
            let lookup = blocking::unblock(move || lookup.route(scheme, &host, port));
            within(self.limits.connect, lookup)
                .await
                .unwrap_or_else(|| {
                    let host = &target.name;
                    tracing::warn!(%host, "egress: upstream lookup too slow; going direct");
                    Route::Direct
                })
        };
        let way = match route {
            Route::Direct => match self.resolve(&target, exchange).await {
                Ok(addresses) => addresses,
                Err(status) => {
                    return self
                        .answer(client, exchange, status, exchange.verdict, "")
                        .await;
                }
            },
            Route::Http { host, port } => {
                if self.screen(&target, exchange).await.is_err() {
                    return self
                        .answer(client, exchange, 403, Verdict::Refused, "")
                        .await;
                }
                Way::Upstream { host, port }
            }
        };
        let (id, registration) = match self.shared.open(&target.name, self.limits.tunnels) {
            Ok(opened) => opened,
            Err(Unopened::NotAllowed) => {
                self.refuse(
                    exchange,
                    HostRefusal::NotAllowed,
                    target.name.is_shared_hosting(),
                );
                return self
                    .answer(client, exchange, 403, Verdict::Refused, "")
                    .await;
            }
            Err(Unopened::Busy) => {
                return self.answer(client, exchange, 503, Verdict::Busy, "").await;
            }
        };
        let counts = Counts::default();
        let relay = async {
            match &target.kind {
                Kind::Connect => {
                    connect::tunnel(self, client, &head.rest, &target, way, &counts).await
                }
                Kind::Http { url, framing } => {
                    http::forward(
                        self, client, &request, &head.rest, &target, url, *framing, way, &counts,
                    )
                    .await
                }
            }
        };
        let outcome = Abortable::new(relay, registration).await;
        self.shared.close(id);
        exchange.bytes_up = counts.up.get();
        exchange.bytes_down = counts.down.get();
        match outcome {
            Ok(verdict) => exchange.verdict = verdict,
            Err(_aborted) => {
                exchange.verdict = if matches!(target.kind, Kind::Connect) {
                    Verdict::Tunnel
                } else {
                    Verdict::Forward
                };
                tracing::info!(host = %target.name, "egress: tunnel closed by revoke");
            }
        }
    }

    /// The addresses for a direct connect, all public; never resolved again. `Err` is the
    /// status to answer, with the exchange's verdict set.
    async fn resolve(&self, target: &Target, exchange: &mut Exchange) -> Result<Way, u16> {
        let addresses = match &target.name {
            Name::Ip(ip) => vec![SocketAddr::new(*ip, target.port)],
            Name::Domain(domain) => {
                match within(self.limits.connect, self.net.resolve(domain, target.port)).await {
                    Some(Ok(found)) if !found.is_empty() => found,
                    Some(Ok(_) | Err(_)) => {
                        tracing::info!(host = %target.name, "egress: DNS found no address");
                        self.reached(false);
                        exchange.verdict = Verdict::Failed;
                        return Err(502);
                    }
                    None => {
                        tracing::info!(host = %target.name, "egress: DNS timed out");
                        self.reached(false);
                        exchange.verdict = Verdict::Failed;
                        return Err(504);
                    }
                }
            }
        };
        if addresses.iter().any(|a| !is_public(a.ip())) {
            self.refuse(exchange, HostRefusal::PrivateAddress, false);
            return Err(403);
        }
        Ok(Way::Direct(addresses))
    }

    /// The address check before an upstream, which gets the name unresolved: an IP literal must
    /// be public, and a name is resolved here too, under the connect timeout, and refused if
    /// any local answer is not public. When local DNS fails or times out the name still goes
    /// upstream (a desktop that resolves only through the proxy), unchecked: a known gap.
    /// `Err` means refused, with the denial sent.
    async fn screen(&self, target: &Target, exchange: &mut Exchange) -> Result<(), ()> {
        let public = match &target.name {
            Name::Ip(ip) => is_public(*ip),
            Name::Domain(domain) => {
                match within(self.limits.connect, self.net.resolve(domain, target.port)).await {
                    Some(Ok(found)) => found.iter().all(|a| is_public(a.ip())),
                    Some(Err(error)) => {
                        let host = &target.name;
                        tracing::debug!(%host, %error, "egress: local DNS failed; sent upstream");
                        true
                    }
                    None => {
                        let host = &target.name;
                        tracing::debug!(%host, "egress: local DNS timed out; upstream resolves it");
                        true
                    }
                }
            }
        };
        if public {
            Ok(())
        } else {
            self.refuse(exchange, HostRefusal::PrivateAddress, false);
            Err(())
        }
    }

    /// Reports a refusal: the denial through the capped sink, and a log line by host only.
    pub(crate) fn refuse(&self, exchange: &mut Exchange, why: HostRefusal, shared_hosting: bool) {
        exchange.verdict = Verdict::Refused;
        exchange.why = Some(why);
        tracing::info!(host = %exchange.host, port = exchange.port, ?why, "egress: refused");
        self.sink.denied(Denial::Host {
            host: exchange.host.clone(),
            port: exchange.port,
            why,
            shared_hosting,
        });
    }

    /// Reports the upstream's demand for credentials.
    pub(crate) fn upstream_auth(&self, upstream: String) {
        tracing::info!(%upstream, "egress: the upstream proxy asked for credentials");
        self.sink.denied(Denial::UpstreamAuth { upstream });
    }

    /// Counts a connect to an origin or an upstream, and reports a change of reachability.
    pub(crate) fn reached(&self, ok: bool) {
        let change = {
            let mut reach = self.reach.borrow_mut();
            let next = if ok {
                reach.failures = 0;
                Some(Reachability::Reachable)
            } else {
                reach.failures = reach.failures.saturating_add(1);
                (reach.failures >= FAILURES_UNREACHABLE).then_some(Reachability::Unreachable)
            };
            match next {
                Some(next) if reach.state != Some(next) => {
                    reach.state = Some(next);
                    Some(next)
                }
                Some(_) | None => None,
            }
        };
        if let Some(change) = change {
            self.sink.upstream(change);
        }
    }

    /// Writes the proxy's own error answer, sets the verdict, and lingers before the close.
    pub(crate) async fn answer(
        &self,
        client: &Stream,
        exchange: &mut Exchange,
        status: u16,
        verdict: Verdict,
        extra: &str,
    ) {
        exchange.verdict = verdict;
        self.fail(client, status, extra).await;
    }

    /// Writes an error answer, then reads and discards what the client still sends (bounded in
    /// bytes and time) so the close does not reset the connection. The socket slot stays held
    /// until the caller drops the client.
    pub(crate) async fn fail(&self, client: &Stream, status: u16, extra: &str) {
        match (&*client).write_all(&head::answer(status, extra)).await {
            Ok(()) => linger(client, self.limits.linger).await,
            Err(error) => tracing::debug!(%error, "egress: the client left before the answer"),
        }
    }
}
