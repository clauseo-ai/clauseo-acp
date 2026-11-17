//! The address check before an upstream proxy, which gets the name unresolved, and open mode's
//! name-only path behind it (`proxy::name_only`).

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use crate::proxy::Exchange;
use crate::proxy::allowlist::Name;
use crate::proxy::head::within;
use crate::proxy::name_only::{FirmNames, LOCAL_DNS_WAIT, Outcome, Run};
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
/// when `Gate::after_failure` finds it due, finds local DNS unable to answer public names: then the
/// name-only path ([`by_name`]). While the last probe found it so, local DNS is waited for
/// [`LOCAL_DNS_WAIT`] at most, else the connect limit, as on any route; while the first probe
/// runs, see [`while_first_probe`]. A lookup whose every answer may be reached tells the probe
/// local DNS may be back (`Gate::answered`).
async fn screen_open(ctx: &Ctx, target: &Target, domain: &str, proxy: &str) -> Option<HostRefusal> {
    let Some(gate) = &ctx.gate else {
        let found = local_dns(ctx, target, domain, ctx.limits.connect).await;
        return Some(private(ctx, &found).unwrap_or(HostRefusal::DnsUnavailable));
    };
    let first = gate.borrow().first_running();
    let found = if let Some(first) = first {
        while_first_probe(ctx, target, domain, first).await
    } else if gate.borrow().name_only() {
        let wait = ctx.limits.connect.min(LOCAL_DNS_WAIT);
        local_dns(ctx, target, domain, wait).await
    } else {
        local_dns(ctx, target, domain, ctx.limits.connect).await
    };
    if !found.is_empty() {
        let refusal = private(ctx, &found);
        if refusal.is_none() {
            gate.borrow_mut().answered();
        }
        return refusal;
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
    let found = within(wait, ctx.net.resolve(domain, target.port)).await;
    answers(target, found)
}

/// Local DNS's answers for `domain` while the first probe (`first`) runs: waited for
/// [`LOCAL_DNS_WAIT`], then, if it has not answered, for the probe. If the probe finds local DNS
/// unable to answer, none; else the lookup is waited for up to the connect limit in all. So the
/// first request on a name-only network waits for the longer of the two, not the connect limit.
async fn while_first_probe(
    ctx: &Ctx,
    target: &Target,
    domain: &str,
    first: Run,
) -> Vec<SocketAddr> {
    let mut asked = ctx.net.resolve(domain, target.port);
    let short = ctx.limits.connect.min(LOCAL_DNS_WAIT);
    if let Some(found) = within(short, &mut asked).await {
        return answers(target, Some(found));
    }
    if matches!(first.await, Outcome::NameOnly(_)) {
        return answers(target, None);
    }
    let rest = ctx.limits.connect.saturating_sub(short);
    answers(target, within(rest, asked).await)
}

/// A lookup's answers; none when it failed (`Some(Err)`) or timed out (`None`).
fn answers(target: &Target, found: Option<io::Result<Vec<SocketAddr>>>) -> Vec<SocketAddr> {
    let host = &target.name;
    match found {
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
