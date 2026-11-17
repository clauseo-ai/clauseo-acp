//! Open mode behind a firm's proxy on a network whose local DNS cannot answer public names (a
//! firm network that resolves them only through its proxy). Only open mode's browser listener,
//! and only on a route through a proxy.
//!
//! Local DNS is always asked first, and any answer is checked as before (one that is not public
//! is refused). Whether a failed lookup may go on by name is never decided by the name itself,
//! which the page picks: a [`Gate`] asks local DNS for fixed public names ([`probe_names`])
//! when the proxy starts and again after a failed lookup: at most once every [`PROBE_EVERY`]
//! while the path is off; while it is on, once [`RECHECK_ON`] has passed, or at once when a
//! lookup has answered since the last probe (local DNS may be back). While any probe name
//! answers, local DNS works, the name-only path is off, and a failed or timed-out lookup is
//! refused as `DnsUnavailable`, as on any other route. Only while none answers does a failed
//! lookup send the name to the firm's proxy unresolved, and then only a
//! CONNECT (the proxy checks no certificate, the browser does) to a name with a dot, under none
//! of [`INTERNAL`], none of [`NAME_TO_ADDRESS`] and none of the firm's own suffixes
//! ([`FirmNames`]), and never the firm proxy's own host. The firm's suffixes are read again on
//! every probe that finds local DNS unable to answer; if any source of them cannot be read, the
//! path stays off. IP literals never come here: the proxy checks them
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

/// The shortest time between two probes while the path is off (or a source of the firm's names
/// could not be read). A probe runs at the proxy's start, then only after a failed lookup.
pub(crate) const PROBE_EVERY: Duration = Duration::from_mins(1);

/// The longest the path stays on, after a failed lookup, without a new probe; sooner when a
/// lookup has answered since the last one.
pub(crate) const RECHECK_ON: Duration = Duration::from_secs(10);

/// Hosts Clauseo already contacts, so a firm's DNS log shows no new name: `github.com` is where
/// the app's update check (`tauri.conf.json`) and the Python runtime come from, and
/// `registry.npmjs.org` is where every agent install fetches its packages
/// (`clauseo_agents::NPM_REGISTRY`). Both are long-lived public names no firm resolves to an
/// address of its own, and neither is chosen by a page.
const APP_NAMES: [&str; 2] = ["github.com", "registry.npmjs.org"];

/// The app's names and Windows's own connectivity check, which the OS looks up all the time:
/// a firm DNS filter that refuses the app's names still answers it.
const WINDOWS_NAMES: [&str; 3] = ["github.com", "registry.npmjs.org", "dns.msftncsi.com"];

/// The app's names and macOS's own captive-portal check, which the OS looks up all the time.
const MACOS_NAMES: [&str; 3] = ["github.com", "registry.npmjs.org", "captive.apple.com"];

/// The names a probe asks local DNS for on this OS, all at once; any answering means local DNS
/// works.
pub(crate) fn probe_names() -> &'static [&'static str] {
    names_for(std::env::consts::OS)
}

/// The probe names on `os` (as `std::env::consts::OS` names it).
fn names_for(os: &str) -> &'static [&'static str] {
    match os {
        "windows" => &WINDOWS_NAMES,
        "macos" => &MACOS_NAMES,
        _ => &APP_NAMES,
    }
}

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
const NAME_TO_ADDRESS: [&str; 11] = [
    "nip.io",
    "sslip.io",
    "xip.io",
    "localtest.me",
    "lvh.me",
    "traefik.me",
    "local.gd",
    "vcap.me",
    "rbndr.us",
    "1u.ms",
    "localhost.direct",
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
    /// No probe name answered: the firm's own names, or `None` when a source of them could
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
    /// Whether a lookup has answered with an address it may reach since the last probe started.
    answered: bool,
    /// The outcome of the probe before the last, for while the last runs.
    before: Option<Outcome>,
}

impl Gate {
    pub(crate) fn new(net: Arc<dyn Network>, read: Arc<FirmRead>, wait: Duration) -> Gate {
        Gate {
            net,
            read,
            wait,
            run: None,
            answered: false,
            before: None,
        }
    }

    /// Starts a probe at `now` and gives it back to be driven.
    pub(crate) fn start(&mut self, now: Duration) -> Run {
        self.before = self.settled();
        let was_on = self.name_only();
        let net = Arc::clone(&self.net);
        let read = Arc::clone(&self.read);
        let run = probe_and_read(net, read, self.wait, was_on)
            .boxed_local()
            .shared();
        self.run = Some((now, run.clone()));
        self.answered = false;
        run
    }

    /// A lookup answered with an address it may reach: local DNS may be back, so the next
    /// failure probes again at once while the path is on.
    pub(crate) fn answered(&mut self) {
        self.answered = true;
    }

    /// Whether the last finished probe found local DNS unable to answer public names.
    pub(crate) fn name_only(&self) -> bool {
        matches!(self.settled(), Some(Outcome::NameOnly(_)))
    }

    /// The first probe while it runs: until it ends, nothing is known of local DNS.
    pub(crate) fn first_running(&self) -> Option<Run> {
        let (_, run) = self.run.as_ref()?;
        self.settled().is_none().then(|| run.clone())
    }

    /// After a lookup failed at `now`: the last probe while it runs, or when it is recent
    /// enough, else a new one. Recent enough while the path is on: started under [`RECHECK_ON`]
    /// ago, with no lookup answered since; otherwise under [`PROBE_EVERY`] ago.
    pub(crate) fn after_failure(&mut self, now: Duration) -> Run {
        match &self.run {
            Some((started, run)) if !self.due(*started, run, now) => run.clone(),
            _ => self.start(now),
        }
    }

    /// Whether `run`, started at `started`, is to be followed by a new probe at `now`.
    fn due(&self, started: Duration, run: &Run, now: Duration) -> bool {
        let every = match run.peek() {
            None => return false,
            Some(Outcome::NameOnly(Some(_))) if self.answered => return true,
            Some(Outcome::NameOnly(Some(_))) => RECHECK_ON,
            Some(Outcome::NameOnly(None) | Outcome::DnsWorks) => PROBE_EVERY,
        };
        now >= started.saturating_add(every)
    }

    /// The last finished probe's outcome.
    fn settled(&self) -> Option<Outcome> {
        let last = self.run.as_ref().and_then(|(_, run)| run.peek().cloned());
        last.or_else(|| self.before.clone())
    }
}

/// Probes; when local DNS does not answer, the firm's names, read again each time (a VPN joined
/// while the path is on is seen at the next probe). A change of state (`was_on`: the last probe
/// found local DNS unable to answer) is logged, on [`LIFECYCLE`].
async fn probe_and_read(
    net: Arc<dyn Network>,
    read: Arc<FirmRead>,
    wait: Duration,
    was_on: bool,
) -> Outcome {
    if probe(&*net, wait, probe_names()).await {
        if was_on {
            tracing::info!(
                target: LIFECYCLE,
                "egress: local DNS answers public names again; the name-only path is off"
            );
        }
        return Outcome::DnsWorks;
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

/// Whether local DNS answers any of `names` within `wait`.
async fn probe(net: &dyn Network, wait: Duration, names: &[&str]) -> bool {
    let asks = names
        .iter()
        .map(|name| within(wait, net.resolve_alone(name, 443)));
    let answers = join_all(asks).await;
    answers
        .iter()
        .any(|answer| matches!(answer, Some(Ok(found)) if !found.is_empty()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{FirmNames, names_for, probe};
    use crate::proxy::allowlist::Name;
    use crate::proxy::tests::test_net::TestNet;

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
            "7f000001.c0a80001.rbndr.us",
            "a.10.0.0.5.1u.ms",
            "1u.ms",
            "app.localhost.direct",
        ] {
            assert!(!admits(&firm, host), "{host}");
        }
    }

    #[test]
    fn any_probe_name_answering_means_local_dns_works() {
        let wait = Duration::from_secs(1);
        let windows = names_for("windows");
        let macos = names_for("macos");
        assert_eq!(
            windows,
            ["github.com", "registry.npmjs.org", "dns.msftncsi.com"]
        );
        assert_eq!(
            macos,
            ["github.com", "registry.npmjs.org", "captive.apple.com"]
        );
        assert_eq!(names_for("linux"), ["github.com", "registry.npmjs.org"]);
        // A firm DNS filter that refuses the app's names but answers the OS's own.
        let net = TestNet::default();
        net.name("dns.msftncsi.com", &["192.0.2.81"]);
        net.name("captive.apple.com", &["192.0.2.82"]);
        assert!(async_io::block_on(probe(&net, wait, windows)));
        assert!(async_io::block_on(probe(&net, wait, macos)));
        assert!(!async_io::block_on(probe(&net, wait, names_for("linux"))));
        let none = TestNet::default();
        assert!(!async_io::block_on(probe(&none, wait, windows)));
    }
}
