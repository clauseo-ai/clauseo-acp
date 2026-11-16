//! Open mode behind a firm's proxy when local DNS cannot answer (a firm network that resolves
//! public names only through its proxy). Only open mode's browser listener, and only when the
//! route goes through a proxy: local DNS is asked first, for at most [`LOCAL_DNS_WAIT`], and
//! any answer is checked as before (one that is not public is refused). Only a lookup that
//! fails, times out or finds nothing sends the name to the firm's proxy unresolved, and then
//! only a CONNECT (the proxy checks no certificate, the browser does) to a name with a dot,
//! under none of [`INTERNAL`] and none of the firm's own suffixes ([`FirmNames`]); anything else
//! is refused as `DnsUnavailable`. IP literals never come here: the proxy checks them itself, in
//! every form the URL standard reads. After [`FAILURES_SKIP`] failed lookups in a row, the
//! launch skips local DNS for [`SKIP_FOR`] ([`Streak`]), so a network with no DNS pays the wait
//! three times a minute at most. The firm's proxy's own answer cannot be checked: a CONNECT's
//! reply names no address.

use std::time::Duration;

use crate::proxy::allowlist::Name;

/// The longest local DNS is waited for before the name goes on alone: the lawyer waits this
/// long for each new host on a network with no DNS, [`FAILURES_SKIP`] times a minute at most.
pub(crate) const LOCAL_DNS_WAIT: Duration = Duration::from_secs(2);

/// Failed local lookups in a row after which the launch stops asking for [`SKIP_FOR`].
const FAILURES_SKIP: u32 = 3;

/// How long the launch skips local DNS after [`FAILURES_SKIP`] failures in a row.
const SKIP_FOR: Duration = Duration::from_mins(1);

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

/// The firm's own DNS suffixes, learned when the proxy starts (`upstream::firm_suffixes`):
/// names under them are the firm's own, which its proxy may reach inside its network.
#[derive(Debug, Clone, Default)]
pub(crate) struct FirmNames {
    suffixes: Vec<String>,
}

impl FirmNames {
    /// `suffixes`, already normalised (lower case, ASCII, no leading or trailing dot).
    pub(crate) fn new(suffixes: Vec<String>) -> FirmNames {
        FirmNames { suffixes }
    }

    /// How many suffixes were learned.
    pub(crate) fn count(&self) -> usize {
        self.suffixes.len()
    }

    /// Whether `name` may go to the firm's proxy by name alone: a domain with a dot, under
    /// none of [`INTERNAL`] and none of the firm's suffixes. Never an IP literal.
    pub(crate) fn admits(&self, name: &Name) -> bool {
        match name {
            Name::Ip(_) => false,
            Name::Domain(domain) => {
                let internal = INTERNAL.iter().any(|suffix| name.is_at_or_under(suffix));
                let firm = self
                    .suffixes
                    .iter()
                    .any(|suffix| name.is_at_or_under(suffix));
                domain.contains('.') && !internal && !firm
            }
        }
    }
}

/// One launch's failed local lookups in a row, on the proxy's clock.
#[derive(Debug, Default)]
pub(crate) struct Streak {
    failures: u32,
    /// Until when local DNS is skipped.
    skip_until: Option<Duration>,
}

impl Streak {
    /// Whether local DNS is skipped at `now`.
    pub(crate) fn skipping(&self, now: Duration) -> bool {
        self.skip_until.is_some_and(|until| now < until)
    }

    /// A lookup answered: the count starts again.
    pub(crate) fn answered(&mut self) {
        self.failures = 0;
    }

    /// A lookup failed at `now`. True when this failure starts a skip.
    pub(crate) fn failed(&mut self, now: Duration) -> bool {
        self.failures = self.failures.saturating_add(1);
        if self.failures < FAILURES_SKIP {
            return false;
        }
        self.failures = 0;
        self.skip_until = Some(now.saturating_add(SKIP_FOR));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{FirmNames, Streak};
    use crate::proxy::allowlist::Name;
    use std::time::Duration;

    fn admits(firm: &FirmNames, host: &str) -> bool {
        firm.admits(&Name::parse(host).unwrap())
    }

    #[test]
    fn names_with_a_dot_outside_every_internal_suffix_pass() {
        let firm = FirmNames::new(vec!["firm.example".to_owned()]);
        for host in [
            "example.com",
            "www.sci.gov.in",
            "corporate.com",
            "notfirm.example",
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
    fn three_failures_in_a_row_skip_for_a_minute() {
        let mut streak = Streak::default();
        let at = Duration::from_secs;
        assert!(!streak.failed(at(1)));
        assert!(!streak.failed(at(2)));
        streak.answered();
        assert!(!streak.failed(at(3)));
        assert!(!streak.failed(at(4)));
        assert!(!streak.skipping(at(4)));
        assert!(streak.failed(at(5)));
        assert!(streak.skipping(at(5)));
        assert!(streak.skipping(at(64)));
        assert!(!streak.skipping(at(65)));
    }
}
