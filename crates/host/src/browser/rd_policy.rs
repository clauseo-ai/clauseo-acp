//! Whether the firm's own browser policy lets Clauseo's browser start: Chrome's and Edge's
//! `RemoteDebuggingAllowed`, read by `clauseo_settings::chromium_policy`. Every browser Clauseo
//! drives needs remote debugging, and Chrome for Testing and chrome-headless-shell do not read
//! the firm's Chrome or Edge policy themselves, so the launch prep asks [`blocked`] before it
//! picks any browser (`launch_checks/browser.rs`). A false anywhere starts none, unless the
//! firm's Clauseo policy allows Clauseo's browser anyway
//! (`browser.allow_when_remote_debugging_disabled`); the entry then drops with
//! `BrowserBlockedByPolicy`. A missing value or an unreadable store does not block. Blocking
//! reads: off the actor (the launch prep).

#[cfg(test)]
mod tests;

use std::sync::Once;

use clauseo_settings::chromium_policy::{Found, RemoteDebugging, RemoteDebuggingPolicy};

/// Whether the session gets no browser: some place says `RemoteDebuggingAllowed` false and
/// `allowed_anyway` (the firm's `browser.allow_when_remote_debugging_disabled`) is false.
/// Logs each place it could not read, once per host; never a path.
pub(crate) fn blocked(policy: &dyn RemoteDebuggingPolicy, allowed_anyway: bool) -> bool {
    static UNREADABLE_LOGGED: Once = Once::new();
    let read = policy.read();
    UNREADABLE_LOGGED.call_once(|| log_unreadable(&read));
    decide(&read, allowed_anyway)
}

/// [`blocked`]'s rule over what was read, with its log line.
fn decide(read: &RemoteDebugging, allowed_anyway: bool) -> bool {
    match read.turned_off_by() {
        None => false,
        Some(place) if allowed_anyway => {
            tracing::info!(
                %place,
                "the firm's browser policy turns remote debugging off; its Clauseo policy allows \
                 Clauseo's browser anyway"
            );
            false
        }
        Some(place) => {
            tracing::warn!(
                %place,
                "the session has no browser: the firm's browser policy turns remote debugging off"
            );
            true
        }
    }
}

/// One warning for each place that could not be read; it does not block.
fn log_unreadable(read: &RemoteDebugging) {
    for (place, found) in &read.found {
        if let Found::Unreadable(why) = found {
            tracing::warn!(
                %place,
                why = %why,
                "the firm's browser policy was not read there; it does not turn the browser off"
            );
        }
    }
}
