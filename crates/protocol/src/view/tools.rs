//! Clauseo's own tools in a thread's header: whether each is available to the agent, and why
//! not. The host fills [`HeaderEntry::tools`](super::HeaderEntry::tools) at each session start.
//! Not logged; every enum here is open, so a newer host's value decodes.

use serde::{Deserialize, Serialize};

use crate::open_enum::{open_data_enum, open_unit_enum};

/// One of Clauseo's own tools and whether the agent has it in this session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ToolState {
    /// The tool.
    pub tool: ClauseoTool,
    /// Whether the agent has it.
    pub availability: ToolAvailability,
    /// Something the lawyer should know about an available tool, if anything; left out when
    /// none, so a header from before it writes back the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<ToolNotice>,
}

open_unit_enum! {
    /// One of Clauseo's own tools.
    pub enum ClauseoTool {
        /// Web search and reading web pages.
        WebSearch,
        /// Clauseo's browser.
        Browser,
        /// The case-law connector (R24), shown only when the connector is added.
        CaseLaw,
    }
    test mod clauseo_tool;
}

open_data_enum! {
    /// Whether the agent has a tool in this session.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ToolAvailability {
        /// The agent has it.
        Available = "available",
        /// The agent does not have it.
        Unavailable = "unavailable" {
            /// Why not.
            reason: ToolsUnavailableReason,
        },
        _ =>
        /// An availability from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod tool_availability with [
        ToolAvailability::Available,
        ToolAvailability::Unavailable {
            reason: ToolsUnavailableReason::LockedOff,
        },
    ];
}

open_unit_enum! {
    /// Why the agent does not have a tool.
    pub enum ToolsUnavailableReason {
        /// The lawyer turned it off.
        Off,
        /// Firm policy turned it off, or a policy source could not be read.
        LockedOff,
        /// Web search only: this login has no web search (Bedrock); reading web pages still
        /// works.
        NoSearchOnThisLogin,
        /// The browser, case law and other MCP servers: the firm manages the agent's MCP servers
        /// (Claude Code's managed MCP), so the host sends none.
        ManagedByFirm,
        /// The browser only: no egress proxy for this session.
        NoProxy,
        /// The browser only: no Chrome for Testing that Clauseo downloaded is there, or the one
        /// at the pinned version no longer matches the record of what the download installed
        /// (a file added, removed or changed); the app offers the download either way. A Chrome
        /// or Edge installed on the machine is never used. (On Linux, development only: no
        /// Google Chrome or Chromium.)
        BrowserNotInstalled,
        /// The browser only: the machine already runs as many browser sessions as its memory
        /// allows.
        TooManyBrowsers,
        /// Case law only: the host could not open the session's loopback relay.
        NoRelay,
        /// The browser only: the firm turned off Chrome's or Edge's remote debugging
        /// (`RemoteDebuggingAllowed` false), which Clauseo's browser needs, and its Clauseo
        /// policy does not allow Clauseo's browser anyway
        /// (`browser.allow_when_remote_debugging_disabled`).
        BrowserBlockedByPolicy,
        /// The browser only: a place the firm's Chrome, Edge or Chromium `RemoteDebuggingAllowed`
        /// can be set could not be read (not missing: denied or failed), so Clauseo does not know
        /// whether remote debugging is allowed; none says false, and the firm's Clauseo policy does
        /// not allow Clauseo's browser anyway. Checked again at the next launch.
        BrowserPolicyUnreadable,
        /// The browser only: no Chrome for Testing Clauseo downloaded is exactly the version
        /// Clauseo's signed browser list pins (each is older, or newer); the app offers the
        /// download, which fetches the pinned version.
        BrowserOutOfDate,
        /// The browser only: a Chrome for Testing Clauseo downloaded is there, and no signed
        /// browser list can be used, so its version cannot be checked: none verifies (neither
        /// the bundled one nor a fetched one), the newest is dated over a day ahead of this
        /// computer's clock, or the stored one stopped verifying and none as new as it has
        /// been fetched again yet.
        BrowserListUnavailable,
        /// The browser only: a Chrome for Testing Clauseo downloaded is the browser, and the
        /// newest signed browser list Clauseo has was published over 45 days ago, so its copy
        /// of Chrome is too old to use, and Clauseo can't update it right now.
        BrowserListExpired,
    }
    test mod tools_unavailable_reason;
}

open_unit_enum! {
    /// Something the lawyer should know about a tool the agent has.
    pub enum ToolNotice {
        /// The browser only: it is a Chrome for Testing Clauseo downloaded, and the newest
        /// signed browser list the app has was published over 14 days ago, so a newer Chrome
        /// may be out; the browser stays available.
        BrowserListOld,
    }
    test mod tool_notice;
}
