//! The browser's host side (S27, decision record 0029): which Chrome a session runs
//! ([`chrome`]), how many run at once and the limits each gets ([`limits`]), the sites every
//! matter's allowlist starts from ([`seed_sites`]) and the background hosts the proxy denies
//! ([`deny_hosts`]), and where Chrome runs ([`BrowserLaunch`]). The `browser` entry itself is
//! filled in `agent/loads/browser.rs`; the pages it opens are recorded in `sources/browser.rs`.

pub mod chrome;
pub mod deny_hosts;
pub mod limits;
pub mod seed_sites;

/// Where the browser runs, a switch for stage C.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BrowserLaunch {
    /// agent-browser starts Chrome itself, inside the session's jail: the only one built.
    #[default]
    InJail,
    /// Chrome runs outside the jail and the entry carries `AGENT_BROWSER_CDP` with this port
    /// instead of starting one. Not built: [`HostConfig::with_browser_launch`] refuses it.
    ///
    /// [`HostConfig::with_browser_launch`]: crate::HostConfig::with_browser_launch
    OutsideJail {
        /// The port of Chrome's `DevTools` endpoint.
        cdp_port: u16,
    },
}

/// A [`BrowserLaunch`] the host does not build yet.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "the browser outside the jail (BrowserLaunch::OutsideJail, CDP port {cdp_port}) is not built \
     yet; only BrowserLaunch::InJail is (decision record 0029)"
)]
pub struct UnsupportedLaunch {
    /// The port asked for.
    pub cdp_port: u16,
}
