//! The block list's check (ADR 0053), with the lawyer's allowed sites asked first (PR 2).

use clauseo_blocklist::{BlockList, Hit};

use crate::proxy::allowlist::Name;
use crate::proxy::head::Request;
use crate::proxy::target::{Kind, Target, host_headers};
use crate::proxy::{Exchange, Verdict};
use crate::{Denial, DenialSink as _};

use super::Ctx;

/// The block list. Asks it for the target's host and, for plain http, each `Host` header's
/// host, as `Name::parse` read them (case folded, one trailing dot dropped, an IDN as
/// punycode); an IP literal is matched exactly. A host the lawyer allowed (the launch's
/// `Allowed`, compared exactly with `Name::text`) is skipped, so it goes on to the port,
/// private-address and allowlist checks like any host; another name in the same request
/// is still asked. No DNS, so a listed name is refused before it is resolved. `Some` is the
/// 403's extra header line (none for a CONNECT), with the denial sent.
pub(super) fn blocked(
    ctx: &Ctx,
    request: &Request,
    target: &Target,
    exchange: &mut Exchange,
) -> Option<String> {
    // Taken once: the request keeps this list and this set whatever an update swaps in.
    let list = ctx.block.get();
    // Nothing loaded: nothing blocked.
    let generated = list.generated()?;
    let allowed = ctx.allowed.get();
    // A `Host` header naming a listed host is refused too. A value that names no host is
    // skipped, which is harmless: the proxy rewrites `Host` from the URL before it forwards.
    let headers = match target.kind {
        Kind::Connect => Vec::new(),
        Kind::Http { .. } => host_headers(request),
    };
    let (name, hit) = std::iter::once(&target.name)
        .chain(&headers)
        .filter(|name| allowed.is_empty() || !allowed.contains(&name.text()))
        .find_map(|name| listed(&list, name).map(|hit| (name, hit)))?;
    exchange.verdict = Verdict::Blocked;
    exchange.why = None;
    let host = name.text();
    tracing::info!(%host, port = exchange.port, "egress: refused (block list)");
    ctx.sink.denied(Denial::Blocked {
        host,
        port: exchange.port,
        listed: hit.listed.to_owned(),
        tag: hit.tag,
        generated,
    });
    Some(match target.kind {
        Kind::Connect => String::new(),
        Kind::Http { .. } => format!("Clauseo-Blocked: {}\r\n", hit.tag.word()),
    })
}

/// The list's entry for `name`: a domain by name and its parents, an IP literal exactly.
pub(crate) fn listed<'a>(list: &'a BlockList, name: &Name) -> Option<Hit<'a>> {
    match name {
        Name::Domain(domain) => list.lookup(domain),
        Name::Ip(ip) => list.lookup_ip(*ip),
    }
}

/// Whether a request that passed the list when the allowed set was at `generation` may still
/// open its tunnel to `name`: yes when the set has not changed since, when it still allows
/// the host, or when the list no longer names it. Asked under the tunnel table's lock, so a
/// revoke's watcher, which takes that lock after the swap, cannot miss the tunnel.
pub(super) fn still_open(ctx: &Ctx, name: &Name, generation: u64) -> bool {
    ctx.allowed.generation() == generation
        || ctx.allowed.contains(&name.text())
        || listed(&ctx.block.get(), name).is_none()
}
