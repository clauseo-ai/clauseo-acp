//! Hosts Chrome reaches on its own, not for a page the agent opened, which stage C's proxy denies
//! silently: no "Allow this site?" card, since the lawyer could not tell what they are for.
//!
//! **Provisional until G17 item 5**, which captures a session's real traffic through the proxy
//! and settles the list. agent-browser v0.41.1 already starts Chrome with
//! `--disable-background-networking`, `--disable-component-update`, `--disable-sync` and
//! `--disable-default-apps` (`cli/src/native/cdp/chrome.rs:466-480`), but not with
//! `--safebrowsing-disable-auto-update`, and none of its flags turns off the optimization
//! guide or autofill's server predictions; the hosts below are those features' and the
//! component updater's, which the flags are known not to stop entirely.

/// One host Chrome contacts in the background, and why it is denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DenyHost {
    /// The exact host name.
    pub host: &'static str,
    /// What Chrome uses it for.
    pub reason: &'static str,
}

/// The background hosts denied without a card (provisional until G17 item 5).
pub const DENY_HOSTS: &[DenyHost] = &[
    DenyHost {
        host: "update.googleapis.com",
        reason: "component updater checks, which --disable-component-update does not stop for every component",
    },
    DenyHost {
        host: "clients2.google.com",
        reason: "component updater and extension update pings",
    },
    DenyHost {
        host: "redirector.gvt1.com",
        reason: "component updater downloads",
    },
    DenyHost {
        host: "edgedl.me.gvt1.com",
        reason: "component updater downloads",
    },
    DenyHost {
        host: "safebrowsing.googleapis.com",
        reason: "Safe Browsing list updates; agent-browser passes no --safebrowsing-disable-auto-update",
    },
    DenyHost {
        host: "sb-ssl.google.com",
        reason: "Safe Browsing lookups",
    },
    DenyHost {
        host: "optimizationguide-pa.googleapis.com",
        reason: "optimization guide hints for each page, which no flag of agent-browser turns off",
    },
    DenyHost {
        host: "content-autofill.googleapis.com",
        reason: "autofill server predictions for forms on a page",
    },
    DenyHost {
        host: "clientservices.googleapis.com",
        reason: "the variations (field trial) seed",
    },
    DenyHost {
        host: "accounts.google.com",
        reason: "Chrome's sign-in checks; the fresh profile has no account",
    },
];

#[cfg(test)]
mod tests {
    use super::DENY_HOSTS;
    use crate::browser::seed_sites::SEED_SITES;

    #[test]
    fn each_host_once_with_a_reason_and_none_seeded() {
        let mut hosts: Vec<&str> = DENY_HOSTS.iter().map(|deny| deny.host).collect();
        hosts.sort_unstable();
        hosts.dedup();
        assert_eq!(hosts.len(), DENY_HOSTS.len(), "a host listed twice");
        for deny in DENY_HOSTS {
            assert!(!deny.reason.is_empty(), "{} has no reason", deny.host);
            assert!(!deny.reason.contains('\n'), "{}: one line", deny.host);
            assert!(!SEED_SITES.contains(&deny.host), "{} is seeded", deny.host);
        }
    }
}
