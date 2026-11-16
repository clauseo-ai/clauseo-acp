//! Open mode behind a firm's proxy on a network whose local DNS cannot answer public names (a
//! firm network that resolves them only through its proxy). Only open mode's browser listener,
//! and only on a route through a proxy.
//!
//! Local DNS is always asked first, and any answer is checked as before (one that is not public
//! is refused). Whether a failed lookup may go on by name is never decided by the name itself,
//! which the page picks: a [`Gate`] asks local DNS for [`PROBE_NAMES`], fixed public names, when
//! the proxy starts and again at most once every [`PROBE_EVERY`] after a failed lookup. While
//! either probe name answers, local DNS works, the name-only path is off, and a failed or
//! timed-out lookup is refused as `DnsUnavailable`, as on any other route. Only while neither
//! answers does a failed lookup send the name to the firm's proxy unresolved, and then only a
//! CONNECT (the proxy checks no certificate, the browser does) to a name with a dot, under none
//! of [`INTERNAL`], none of [`NAME_TO_ADDRESS`] and none of the firm's own suffixes
//! ([`FirmNames`]), and never the firm proxy's own host. The firm's suffixes are read when the
//! path first comes on and again each time the probe's answer changes; if any source of them
//! cannot be read, the path stays off. IP literals never come here: the proxy checks them
//! itself, in every form the URL standard reads.
//!
//! What stays open on a network that really is name-only: the firm's proxy, not Clauseo,
//! resolves the name, and a CONNECT's reply names no address, so a public name whose DNS points
//! at a private address reaches whatever the firm's proxy lets it reach on port 443.

use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt as _;
use futures::future::{LocalBoxFuture, Shared, join_all};

use crate::proxy::allowlist::Name;
use crate::proxy::head::within;
use crate::proxy::net::Network;

/// The target of the path's lifecycle lines: the host's log file keeps it at info
/// (`clauseo_host::log_file`).
pub(crate) const LIFECYCLE: &str = "clauseo_sandbox::proxy::lifecycle";

/// The longest local DNS is waited for while the name-only path is on: the lawyer waits this
/// long for each new host on a network whose DNS does not answer.
pub(crate) const LOCAL_DNS_WAIT: Duration = Duration::from_secs(2);

/// The longest a probe waits for local DNS (capped by the connect limit). No answer in time
/// counts as none: a resolver that does not answer at all is the case the path is for.
pub(crate) const PROBE_WAIT: Duration = Duration::from_secs(3);

/// The shortest time between two probes. A probe runs at the proxy's start, then only after a
/// failed lookup.
pub(crate) const PROBE_EVERY: Duration = Duration::from_mins(1);

/// The names a probe asks local DNS for, both at once; either answering means local DNS works.
/// Hosts Clauseo already contacts, so a firm's DNS log shows no new name: `github.com` is where
/// the app's update check (`tauri.conf.json`) and the Python runtime come from, and
/// `registry.npmjs.org` is where every agent install fetches its packages
/// (`clauseo_agents::NPM_REGISTRY`). Both are long-lived public names no firm resolves to an
/// address of its own, and neither is chosen by a page.
pub(crate) const PROBE_NAMES: [&str; 2] = ["github.com", "registry.npmjs.org"];

/// Names under these (or these themselves) are never sent on by name alone: names for the
/// machine, the home network or an intranet, which public DNS does not answer.
const INTERNAL: [&str; 9] = [
    "localhost",
    "local",
    "internal",
    "lan",
    "corp",
    "home",
    "home.arpa",
    "localdomain",
    "intranet",
];

/// Public services whose names spell an address (`10-0-0-5.sslip.io`, `127.0.0.1.nip.io`):
/// their names under these (or these themselves) are never sent on by name alone, since the
/// firm's proxy would reach the address the name spells.
const NAME_TO_ADDRESS: [&str; 8] = [
    "nip.io",
    "sslip.io",
    "xip.io",
    "localtest.me",
    "lvh.me",
    "traefik.me",
    "local.gd",
    "vcap.me",
];

/// Reads the firm's own suffixes from every source this OS has (`upstream::firm_suffixes`), or
/// `None` when any source cannot be read. Runs off the proxy's thread.
pub(crate) type FirmRead = dyn Fn() -> Option<Vec<String>> + Send + Sync;

/// The firm's own DNS suffixes: names under them are the firm's own, which its proxy may reach
/// inside its network.
#[derive(Debug, Clone, Default)]
pub(crate) struct FirmNames {
    suffixes: Vec<String>,
}

impl FirmNames {
    /// `suffixes`, already normalised (lower case, ASCII, no leading or trailing dot).
    pub(crate) fn new(suffixes: Vec<String>) -> FirmNames {
        FirmNames { suffixes }
    }

    /// Whether `name` may go to the firm's proxy by name alone: a domain with a dot, under
    /// none of [`INTERNAL`], [`NAME_TO_ADDRESS`] and the firm's suffixes. Never an IP literal.
    pub(crate) fn admits(&self, name: &Name) -> bool {
        match name {
            Name::Ip(_) => false,
            Name::Domain(domain) => {
                let fixed = INTERNAL.iter().chain(&NAME_TO_ADDRESS);
                let refused = fixed
                    .copied()
                    .chain(self.suffixes.iter().map(String::as_str))
                    .any(|suffix| name.is_at_or_under(suffix));
                domain.contains('.') && !refused
            }
        }
    }
}

/// What a probe found.
#[derive(Debug, Clone)]
pub(crate) enum Outcome {
    /// Local DNS answered a probe name: the name-only path is off.
    DnsWorks,
    /// Neither probe name answered: the firm's own names, or `None` when a source of them could
    /// not be read (the path is off then too).
    NameOnly(Option<Arc<FirmNames>>),
}

/// One probe, with the read of the firm's names it may need; awaited by every lookup that
/// fails while it runs.
pub(crate) type Run = Shared<LocalBoxFuture<'static, Outcome>>;

/// One launch's probe state, on the proxy's clock.
pub(crate) struct Gate {
    net: Arc<dyn Network>,
    read: Arc<FirmRead>,
    /// How long a probe waits for local DNS.
    wait: Duration,
    /// The last probe and when it started; none before the first.
    run: Option<(Duration, Run)>,
}

impl Gate {
    pub(crate) fn new(net: Arc<dyn Network>, read: Arc<FirmRead>, wait: Duration) -> Gate {
        Gate {
            net,
            read,
            wait,
            run: None,
        }
    }

    /// Starts a probe at `now` and gives it back to be driven.
    pub(crate) fn start(&mut self, now: Duration) -> Run {
        let previous = self.finished();
        let net = Arc::clone(&self.net);
        let read = Arc::clone(&self.read);
        let run = probe_and_read(net, read, self.wait, previous)
            .boxed_local()
            .shared();
        self.run = Some((now, run.clone()));
        run
    }

    /// Whether the last finished probe found the name-only path on.
    pub(crate) fn name_only(&self) -> bool {
        matches!(self.finished(), Some(Outcome::NameOnly(_)))
    }

    /// After a lookup failed at `now`: the last probe, or a new one when the last started
    /// [`PROBE_EVERY`] ago or more.
    pub(crate) fn after_failure(&mut self, now: Duration) -> Run {
        match &self.run {
            Some((started, run)) if now < started.saturating_add(PROBE_EVERY) => run.clone(),
            _ => self.start(now),
        }
    }

    /// The last finished probe's outcome.
    fn finished(&self) -> Option<Outcome> {
        self.run.as_ref().and_then(|(_, run)| run.peek().cloned())
    }
}

/// Probes; when local DNS does not answer, the firm's names: the last probe's when it found the
/// path on too and read every source, else read again (the path just came on, or a source
/// could not be read last time). A change of state is logged, on [`LIFECYCLE`].
async fn probe_and_read(
    net: Arc<dyn Network>,
    read: Arc<FirmRead>,
    wait: Duration,
    previous: Option<Outcome>,
) -> Outcome {
    let was_on = matches!(previous, Some(Outcome::NameOnly(_)));
    if probe(&*net, wait).await {
        if was_on {
            tracing::info!(
                target: LIFECYCLE,
                "egress: local DNS answers public names again; the name-only path is off"
            );
        }
        return Outcome::DnsWorks;
    }
    if let Some(Outcome::NameOnly(Some(names))) = previous {
        return Outcome::NameOnly(Some(names));
    }
    if !was_on {
        tracing::info!(
            target: LIFECYCLE,
            "egress: local DNS does not answer public names; the name-only path is on"
        );
    }
    let names = blocking::unblock(move || read()).await;
    Outcome::NameOnly(names.map(|names| Arc::new(FirmNames::new(names))))
}

/// Whether local DNS answers either of [`PROBE_NAMES`] within `wait`.
async fn probe(net: &dyn Network, wait: Duration) -> bool {
    let asks = PROBE_NAMES.map(|name| within(wait, net.resolve_alone(name, 443)));
    let answers = join_all(asks).await;
    answers
        .iter()
        .any(|answer| matches!(answer, Some(Ok(found)) if !found.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::FirmNames;
    use crate::proxy::allowlist::Name;

    fn admits(firm: &FirmNames, host: &str) -> bool {
        firm.admits(&Name::parse(host).unwrap())
    }

    #[test]
    fn names_with_a_dot_outside_every_refused_suffix_pass() {
        let firm = FirmNames::new(vec!["firm.example".to_owned()]);
        for host in [
            "example.com",
            "www.sci.gov.in",
            "corporate.com",
            "notfirm.example",
            "notsslip.io",
            "github.io",
        ] {
            assert!(admits(&firm, host), "{host}");
        }
        for host in [
            "intranet",
            "localhost",
            "a.localhost",
            "printer.local",
            "wiki.corp",
            "x.home.arpa",
            "nas.home",
            "box.lan",
            "a.b.internal",
            "pc.localdomain",
            "portal.intranet",
            "firm.example",
            "wiki.firm.example",
            "127.0.0.1",
        ] {
            assert!(!admits(&firm, host), "{host}");
        }
    }

    #[test]
    fn names_that_spell_an_address_are_refused() {
        let firm = FirmNames::default();
        for host in [
            "10-0-0-5.sslip.io",
            "127.0.0.1.nip.io",
            "nip.io",
            "a.10.0.0.5.xip.io",
            "localtest.me",
            "app.lvh.me",
            "10-0-0-5.traefik.me",
            "x.local.gd",
            "api.vcap.me",
        ] {
            assert!(!admits(&firm, host), "{host}");
        }
    }
}
