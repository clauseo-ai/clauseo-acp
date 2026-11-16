//! The re-read of the firm's browser policy that `HostCommand::RecheckBrowserPolicy` asks for
//! (decision record 0029, the 2026-11-08 amendment): where it stands, as
//! `HostEvent::BrowserPolicy` carries it to every app subscriber. App-wide and never logged,
//! like `BrowserInstall`.

use serde::{Deserialize, Serialize};

/// What the last read of the firm's browser policy (Chrome's, Edge's and Chromium's
/// `RemoteDebuggingAllowed`) found, once a lawyer asked for one. Sessions already running keep
/// the browser they started with: after `Clear` the next session started gets the browser, as
/// after a Chrome download's `Done`. Closed: the host and the app ship together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum BrowserPolicyState {
    /// A read is under way.
    Checking,
    /// A place of the policy still could not be read, and none says false: the browser stays
    /// off (`ToolsUnavailableReason::BrowserPolicyUnreadable`).
    Unreadable,
    /// The policy turns remote debugging off: the browser stays off, and every header that
    /// showed `BrowserPolicyUnreadable` now shows `BrowserBlockedByPolicy`.
    Blocked,
    /// The policy does not keep the browser off: the next session started may have it.
    Clear,
}

#[cfg(test)]
mod tests;
