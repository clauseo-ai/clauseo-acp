//! The lawyer's "Allow this site" override of the unsafe-site block list (decision record 0053,
//! PR 2): the scope an allow has, and one allowed site as `HostQuery::AllowedSites` answers it.
//! Never logged and in no stream: commands, a query and its answer carry these types.
//!
//! - [`AllowScope`] is closed: a command must name a scope this host keeps, and the desktop app
//!   ships with the host (0002's freeze rules), so a new scope is additive.
//! - An allowed site's host is the exact host the block's notice named
//!   (`HostEvent::SiteBlocked::host`), as the egress proxy printed it; an allow covers that host
//!   and nothing under it.

use serde::{Deserialize, Serialize};

use crate::{MatterId, Timestamp};

/// How far an allow reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum AllowScope {
    /// The blocked request's matter only: its agents' later launches and the running one.
    Matter,
    /// Every matter, now and later.
    Always,
}

/// One site the lawyer allowed, as Settings lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AllowedSite {
    /// The host name or IP address, exactly as the notice named it.
    pub host: String,
    /// How far the allow reaches.
    pub scope: AllowScope,
    /// The matter of a `Matter` allow; left out for `Always`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matter: Option<MatterId>,
    /// When the lawyer allowed it.
    pub allowed_at: Timestamp,
}
