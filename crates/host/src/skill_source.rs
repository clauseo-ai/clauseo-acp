//! Adding a skill from the network (decision record 0040, "Adding from a link"): the download
//! of a GitHub archive or a `.zip` link, and the skills.sh catalogue's search. Each runs only on
//! the lawyer's own action (a preview, a search), never in the background, on a thread off the
//! actor; nothing is ever sent to skills.sh beyond the search's own terms (no telemetry).
//!
//! The client is the block list's (`blocklist/fetch.rs`): clauseo-connectors' `HttpsClient`
//! (rustls, the OS trust store, which holds a firm CA installed in the OS), routed through
//! `network.upstream_proxy` when set, else the system's proxy (env, then the OS's settings,
//! PAC included); a refused proxy setting fails the download and never goes around it.
//! `https` only, no explicit port, no user info, redirects followed by hand (at most 5), each
//! hop checked again.
//!
//! - [`download::download`]: at most `DOWNLOAD_MAX` (10 MiB) within
//!   [`download::DOWNLOAD_TIMEOUT`]. A 404 or 410 is `NotFound`; past the cap `TooBig`; anything
//!   else that fails (offline, the proxy, TLS, a timeout, a 5xx, a 429, a 403) `DownloadFailed`,
//!   worth a retry.
//! - [`download::preview_remote`]: the download staged and its skills found
//!   (`profile::packs::source`).
//! - [`search::search`]: `GET https://skills.sh/api/search?q=<terms>`, at most
//!   [`search::SEARCH_MAX`] items; none found, it asks once more with the last term dropped, as
//!   the skills.sh CLI does.

pub(crate) mod download;
pub(crate) mod icon;
pub(crate) mod search;
#[cfg(test)]
mod tests;
