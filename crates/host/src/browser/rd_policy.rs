//! Whether the firm's own browser policy lets Clauseo's browser start: Chrome's, Edge's and
//! Chromium's `RemoteDebuggingAllowed`, read by `clauseo_settings::chromium_policy`. Every
//! browser Clauseo drives needs remote debugging, and Chrome for Testing and
//! chrome-headless-shell do not read the firm's policy themselves, so the launch prep asks
//! [`RdPolicy::check`] before it picks any browser (`launch_checks/browser.rs`), at every
//! launch: nothing is kept between launches.
//!
//! A false anywhere starts none (`BrowserBlockedByPolicy`). Otherwise a place that could not be
//! read starts none either (`BrowserPolicyUnreadable`): nothing is known there, and the lawyer is
//! not told the firm turned the browser off. Either way the firm's Clauseo policy can allow
//! Clauseo's browser anyway (`browser.allow_when_remote_debugging_disabled`). Nothing set, or a
//! value Chrome would not take, does not block. Blocking reads: off the actor (the launch prep,
//! and the lawyer's "Try again", `RecheckBrowserPolicy`, on a thread of its own).

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_protocol::ToolsUnavailableReason;
use clauseo_settings::chromium_policy::{Place, RemoteDebugging, RemoteDebuggingPolicy};

/// The wiring's reader of the firm's browser policy, with its log-once guards: each place it
/// could not read, and each bad value, is logged the first time this reader sees one (once per
/// host), with no path.
#[derive(Debug)]
pub(crate) struct RdPolicy {
    reader: Arc<dyn RemoteDebuggingPolicy>,
    unreadable_logged: AtomicBool,
    bad_logged: AtomicBool,
}

impl RdPolicy {
    /// The rule over `reader`.
    pub(crate) fn new(reader: Arc<dyn RemoteDebuggingPolicy>) -> Self {
        Self {
            reader,
            unreadable_logged: AtomicBool::new(false),
            bad_logged: AtomicBool::new(false),
        }
    }

    /// The reader the host uses unless a test passes one: this machine's stores, or in this
    /// crate's tests and its dependents' (`test-support`) one that finds nothing set, so no
    /// test reads the machine it runs on.
    pub(crate) fn wired() -> Self {
        #[cfg(any(test, feature = "test-support"))]
        let reader = Arc::new(clauseo_settings::chromium_policy::NothingSet);
        #[cfg(not(any(test, feature = "test-support")))]
        let reader = Arc::new(clauseo_settings::chromium_policy::SystemPolicy);
        Self::new(reader)
    }

    /// Why the session gets no browser, read now; `None` when it may have one.
    /// `allowed_anyway` is the firm's `browser.allow_when_remote_debugging_disabled`.
    pub(crate) fn check(&self, allowed_anyway: bool) -> Option<ToolsUnavailableReason> {
        let read = self.reader.read();
        self.log_once(&read);
        decide(&read, allowed_anyway)
    }

    /// Why no browser would start, read now for the lawyer's "Try again"
    /// (`actor/browser_policy.rs`): [`Self::check`]'s rule and its log-once guards, without the
    /// line each launch logs, since no session is left out by it.
    pub(crate) fn recheck(&self, allowed_anyway: bool) -> Option<ToolsUnavailableReason> {
        let read = self.reader.read();
        self.log_once(&read);
        rule(&read)
            .filter(|_| !allowed_anyway)
            .map(|(_, reason)| reason)
    }

    /// The first unreadable place and the first bad value this reader sees, each logged once.
    fn log_once(&self, read: &RemoteDebugging) {
        if read.unreadable().next().is_some()
            && !self.unreadable_logged.swap(true, Ordering::Relaxed)
        {
            for (place, why) in read.unreadable() {
                tracing::warn!(
                    %place,
                    why,
                    "the firm's browser policy was not read there; until it is, the session has \
                     no browser"
                );
            }
        }
        if read.bad_values().next().is_some() && !self.bad_logged.swap(true, Ordering::Relaxed) {
            for (place, why) in read.bad_values() {
                tracing::warn!(
                    %place,
                    why,
                    "the firm's browser policy holds a value Chrome ignores; Clauseo ignores it too"
                );
            }
        }
    }
}

/// [`RdPolicy::check`]'s rule over what was read, with its log line: a false wins over a read
/// error, and the firm's Clauseo opt-in overrides both.
fn decide(read: &RemoteDebugging, allowed_anyway: bool) -> Option<ToolsUnavailableReason> {
    let (place, reason) = rule(read)?;
    let off = reason == ToolsUnavailableReason::BrowserBlockedByPolicy;
    if allowed_anyway {
        tracing::info!(
            %place,
            turned_off = off,
            "the firm's browser policy turns remote debugging off or was not read; its Clauseo \
             policy allows Clauseo's browser anyway"
        );
        return None;
    }
    if off {
        tracing::warn!(
            %place,
            "the session has no browser: the firm's browser policy turns remote debugging off"
        );
    } else {
        tracing::warn!(
            %place,
            "the session has no browser: the firm's browser policy was not read"
        );
    }
    Some(reason)
}

/// The rule alone, before the firm's Clauseo opt-in: the place that blocks and why, a false
/// before a read error; `None` when nothing blocks.
fn rule(read: &RemoteDebugging) -> Option<(Place, ToolsUnavailableReason)> {
    if let Some(place) = read.turned_off_by() {
        Some((place, ToolsUnavailableReason::BrowserBlockedByPolicy))
    } else {
        read.unreadable()
            .next()
            .map(|(place, _)| (place, ToolsUnavailableReason::BrowserPolicyUnreadable))
    }
}
