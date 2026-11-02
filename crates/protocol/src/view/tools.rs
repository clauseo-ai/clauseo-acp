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
}

open_unit_enum! {
    /// One of Clauseo's own tools.
    pub enum ClauseoTool {
        /// Web search and reading web pages.
        WebSearch,
        /// Clauseo's browser.
        Browser,
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
        /// The browser (and other MCP servers): the firm manages the agent's MCP servers (Claude
        /// Code's managed MCP), so the host sends none.
        ManagedByFirm,
        /// The browser only: no egress proxy for this session.
        NoProxy,
        /// The browser only: no browser is installed.
        BrowserNotInstalled,
        /// The browser only: the machine already runs as many browser sessions as its memory
        /// allows.
        TooManyBrowsers,
    }
    test mod tools_unavailable_reason;
}
