//! The address check before an upstream proxy, which gets the name unresolved, and open mode's
//! name-only path behind it (`proxy::name_only`).

use std::net::SocketAddr;
use std::time::Duration;

use crate::proxy::Exchange;
use crate::proxy::allowlist::Name;
use crate::proxy::head::within;
use crate::proxy::name_only::LOCAL_DNS_WAIT;
use crate::proxy::target::{Kind, Target};
use crate::{Clock as _, HostRefusal};

use super::Ctx;

/// The address check before an upstream: an IP literal must be public, and a name is resolved
/// here too and refused if any local answer is not public (`PrivateAddress`). When local DNS
/// fails, times out or answers nothing, a request that met the allowlist still goes upstream (a
/// desktop that resolves only through the proxy), unchecked: a known gap. One let in without the
/// list (open mode's browser listener) takes the name-only path instead ([`by_name`]). `Err`
/// means refused, with the denial sent.
pub(super) async fn screen(ctx: &Ctx, target: &Target, exchange: &mut Exchange) -> Result<(), ()> {
    let listless = ctx.shared.listless(exchange.listener);
    let refusal = match &target.name {
        Name::Ip(ip) => (!ctx.reachable(*ip)).then_some(HostRefusal::PrivateAddress),
        Name::Domain(domain) if listless => screen_open(ctx, target, domain).await,
        Name::Domain(domain) => {
            let found = local_dns(ctx, target, domain, ctx.limits.connect).await;
            private(ctx, &found)
        }
    };
    match refusal {
        None => Ok(()),
        Some(why) => {
            ctx.refuse(exchange, why, false);
            Err(())
        }
    }
}

/// Open mode's browser listener: local DNS first, for at most [`LOCAL_DNS_WAIT`], its answers
/// checked as on any route; no answer, the name-only path. While the launch's
/// [`Streak`](crate::proxy::name_only::Streak) skips local DNS, the name-only path at once.
async fn screen_open(ctx: &Ctx, target: &Target, domain: &str) -> Option<HostRefusal> {
    let skipping = ctx.streak.borrow().skipping(ctx.dns_clock.now());
    if !skipping {
        let wait = ctx.limits.connect.min(LOCAL_DNS_WAIT);
        let found = local_dns(ctx, target, domain, wait).await;
        if !found.is_empty() {
            ctx.streak.borrow_mut().answered();
            return private(ctx, &found);
        }
        let now = ctx.dns_clock.now();
        if ctx.streak.borrow_mut().failed(now) {
            tracing::info!("egress: local DNS failed 3 times in a row; skipped for 60 s");
        }
    }
    by_name(ctx, target)
}

/// The name-only path: the name goes to the firm's proxy unresolved, so only a CONNECT (TLS,
/// whose certificate the browser checks against the name) to a name the firm's names admit
/// ([`FirmNames::admits`](crate::proxy::name_only::FirmNames::admits)); anything else is
/// refused as `DnsUnavailable`: its address could not be checked.
fn by_name(ctx: &Ctx, target: &Target) -> Option<HostRefusal> {
    let connect = matches!(target.kind, Kind::Connect);
    if connect && ctx.firm.admits(&target.name) {
        tracing::debug!("egress: local DNS did not answer; the name went to the proxy alone");
        None
    } else {
        Some(HostRefusal::DnsUnavailable)
    }
}

/// `PrivateAddress` when any of `found` may not be reached.
fn private(ctx: &Ctx, found: &[SocketAddr]) -> Option<HostRefusal> {
    found
        .iter()
        .any(|address| !ctx.reachable(address.ip()))
        .then_some(HostRefusal::PrivateAddress)
}

/// Local DNS's answers for `domain`, under `wait`; none when it fails or times out.
async fn local_dns(ctx: &Ctx, target: &Target, domain: &str, wait: Duration) -> Vec<SocketAddr> {
    let host = &target.name;
    match within(wait, ctx.net.resolve(domain, target.port)).await {
        Some(Ok(found)) => found,
        Some(Err(error)) => {
            tracing::debug!(%host, %error, "egress: local DNS failed");
            Vec::new()
        }
        None => {
            tracing::debug!(%host, "egress: local DNS timed out");
            Vec::new()
        }
    }
}
