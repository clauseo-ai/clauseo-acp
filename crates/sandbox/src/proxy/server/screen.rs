//! The address check before an upstream proxy, which gets the name unresolved, and open mode's
//! name-only path behind it (`proxy::name_only`).

use std::net::SocketAddr;
use std::time::Duration;

use crate::proxy::Exchange;
use crate::proxy::allowlist::Name;
use crate::proxy::head::within;
use crate::proxy::name_only::{FirmNames, LOCAL_DNS_WAIT, Outcome};
use crate::proxy::target::{Kind, Target};
use crate::{Clock as _, HostRefusal};

use super::Ctx;

/// The address check before an upstream at `proxy` (its host): an IP literal must be public,
/// and a name is resolved here too and refused if any local answer is not public
/// (`PrivateAddress`). When local DNS fails, times out or answers nothing, a request that met the
/// allowlist still goes upstream (a desktop that resolves only through the proxy), unchecked: a
/// known gap. One let in without the list (open mode's browser listener) is refused then
/// (`DnsUnavailable`), unless the name-only path is on ([`screen_open`]). `Err` means refused,
/// with the denial sent.
pub(super) async fn screen(
    ctx: &Ctx,
    target: &Target,
    proxy: &str,
    exchange: &mut Exchange,
) -> Result<(), ()> {
    let listless = ctx.shared.listless(exchange.listener);
    let refusal = match &target.name {
        Name::Ip(ip) => (!ctx.reachable(*ip)).then_some(HostRefusal::PrivateAddress),
        Name::Domain(domain) if listless => screen_open(ctx, target, domain, proxy).await,
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

/// Open mode's browser listener: local DNS first, its answers checked as on any route. No
/// answer is `DnsUnavailable` unless the launch's probe (`name_only::Gate`), run again first
/// when the last is a minute old, finds local DNS unable to answer public names: then the
/// name-only path ([`by_name`]). While the last probe found it so, local DNS is waited for
/// [`LOCAL_DNS_WAIT`] at most, else the connect limit, as on any route.
async fn screen_open(ctx: &Ctx, target: &Target, domain: &str, proxy: &str) -> Option<HostRefusal> {
    let Some(gate) = &ctx.gate else {
        let found = local_dns(ctx, target, domain, ctx.limits.connect).await;
        return Some(private(ctx, &found).unwrap_or(HostRefusal::DnsUnavailable));
    };
    let wait = if gate.borrow().name_only() {
        ctx.limits.connect.min(LOCAL_DNS_WAIT)
    } else {
        ctx.limits.connect
    };
    let found = local_dns(ctx, target, domain, wait).await;
    if !found.is_empty() {
        return private(ctx, &found);
    }
    let run = gate.borrow_mut().after_failure(ctx.dns_clock.now());
    match run.await {
        Outcome::NameOnly(Some(firm)) => by_name(target, &firm, proxy),
        Outcome::NameOnly(None) | Outcome::DnsWorks => Some(HostRefusal::DnsUnavailable),
    }
}

/// The name-only path: the name goes to the firm's proxy unresolved, so only a CONNECT (TLS,
/// whose certificate the browser checks against the name) to a name the firm's names admit
/// ([`FirmNames::admits`]) and that is not the firm proxy's own host (`proxy`, which would
/// connect to itself); anything else is refused as `DnsUnavailable`: its address could not be
/// checked.
fn by_name(target: &Target, firm: &FirmNames, proxy: &str) -> Option<HostRefusal> {
    let connect = matches!(target.kind, Kind::Connect);
    let own = Name::parse(proxy).is_some_and(|proxy| proxy == target.name);
    if connect && !own && firm.admits(&target.name) {
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
