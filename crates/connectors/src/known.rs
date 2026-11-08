//! The compiled-in connectors. The app never offers a connector that is not listed here.

/// A connector Clauseo knows, with its production URLs. Under `Origin::Staging` or
/// `Origin::Fake` each URL maps onto that origin (`Config::map_url`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownConnector {
    /// The connector's id, also the keychain account and the state file's key: `case-law`.
    pub id: &'static str,
    /// The product name. The app shows its own words.
    pub name: &'static str,
    /// The MCP endpoint, also the first OAuth `resource`.
    pub mcp_url: &'static str,
    /// The JSON API's base, also the second OAuth `resource`.
    pub api_base: &'static str,
    /// The OAuth issuer.
    pub issuer: &'static str,
    /// The OAuth `resource` values, in the order the authorize URL sends them: MCP, then API.
    pub resources: &'static [&'static str],
    /// The OAuth scope.
    pub scope: &'static str,
    /// Every pay link starts with this.
    pub pay_prefix: &'static str,
}

/// The production origin every compiled-in URL starts with.
pub(crate) const PRODUCTION_ORIGIN: &str = "https://data.clauseo.chat";

/// The staging origin, with the same paths as production (feature `staging` only).
#[cfg(feature = "staging")]
pub(crate) const STAGING_ORIGIN: &str = "https://data-staging.clauseo.chat";

/// The connector's local test server on the Mac mini (feature `local` only): plain http, the
/// same paths as production. A fixed address, so a build can be scanned for it.
#[cfg(feature = "local")]
pub const LOCAL_ORIGIN: &str = "http://127.0.0.1:8793";

/// Indian Kanoon case law, the one connector at launch.
pub const CASE_LAW: KnownConnector = KnownConnector {
    id: "case-law",
    name: "Clauseo Case Law",
    mcp_url: "https://data.clauseo.chat/mcp/case-law",
    api_base: "https://data.clauseo.chat/v1/case-law",
    issuer: "https://data.clauseo.chat",
    resources: &[
        "https://data.clauseo.chat/mcp/case-law",
        "https://data.clauseo.chat/v1/case-law",
    ],
    scope: "case-law offline_access",
    pay_prefix: "https://data.clauseo.chat/pay/",
};

/// Every connector Clauseo knows.
pub const KNOWN: &[&KnownConnector] = &[&CASE_LAW];

/// The known connector with this id, or `None`.
pub fn known(id: &str) -> Option<&'static KnownConnector> {
    KNOWN.iter().copied().find(|connector| connector.id == id)
}

/// The OAuth client ID: a client ID metadata document the connector serves.
pub const CLIENT_ID: &str = "https://data.clauseo.chat/oauth/clients/clauseo-desktop.json";

/// Liveness and price, fetched without auth.
pub const PRICING_URL: &str = "https://data.clauseo.chat/pricing.json";

/// The keychain service of every connector's item; the account is the connector's id.
pub const KEYCHAIN_SERVICE: &str = "ai.clauseo.desktop.connectors";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_finds_case_law_and_nothing_else() {
        assert_eq!(known("case-law"), Some(&CASE_LAW));
        assert_eq!(known("case_law"), None);
        assert_eq!(known(""), None);
    }

    #[test]
    fn every_compiled_in_url_starts_with_the_production_origin() {
        let urls = [
            CASE_LAW.mcp_url,
            CASE_LAW.api_base,
            CASE_LAW.issuer,
            CASE_LAW.pay_prefix,
            CLIENT_ID,
            PRICING_URL,
        ];
        for url in urls.iter().chain(CASE_LAW.resources) {
            assert!(url.starts_with(PRODUCTION_ORIGIN), "{url}");
        }
        assert_eq!(CASE_LAW.resources, [CASE_LAW.mcp_url, CASE_LAW.api_base]);
    }
}
