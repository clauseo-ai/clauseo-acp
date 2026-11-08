//! Chrome's own background traffic, turned off: the switches and profile preferences that keep
//! an idle Chrome from contacting any host through the session's proxy (decision record 0029,
//! "No background hosts"). agent-browser's flags (`--disable-background-networking`,
//! `--disable-component-update`, `--disable-sync`) left six Google hosts in G17 item 5's 300 s
//! idle run on iris; with these, none.
//!
//! Nothing else weakens a security control; Safe Browsing's warnings are given up (ADR 0029).
//! No certificate, sandbox, web-security or site isolation change. Safe Browsing is off, so its
//! list updates and lookups never leave; the session's egress proxy and deny list are the
//! control on where the browser goes.

/// Chrome's one `--disable-features` and one `--enable-features` (Chrome reads only the last of
/// each): agent-browser's own entries plus the features below, by Chromium's exact names.
pub(crate) const FEATURES: [&str; 2] = [
    // Translate: page translation (agent-browser's entry). OptimizationHints: hint and model
    // downloads (optimizationguide-pa.googleapis.com). NetworkTimeServiceQuerying: the time
    // query to clients2.google.com. AimEnabled: the AI-mode eligibility fetch, and
    // PreconnectToSearch: the search-engine preconnect, both to www.google.com. MediaRouter:
    // Cast discovery on the local network. AutofillServerCommunication: autofill's form
    // predictions (content-autofill.googleapis.com).
    "--disable-features=Translate,OptimizationHints,NetworkTimeServiceQuerying,AimEnabled,\
     PreconnectToSearch,MediaRouter,AutofillServerCommunication",
    // agent-browser's entries, plus SkipListAccountsWithoutGaiaCookies: no accounts.google.com
    // ListAccounts without a Google cookie (Chrome 155 and later; earlier Chrome ignores it,
    // and `--gaia-url` below covers them).
    "--enable-features=NetworkService,NetworkServiceInProcess,WebMCPTesting,\
     DevToolsWebMCPSupport,SkipListAccountsWithoutGaiaCookies",
];

/// The switches beside agent-browser's, each for traffic no feature or flag of its own stops.
pub(crate) const ARGS: &[&str] = &[
    // Chromium's compiled-in field trial config (variations), so no trial turns a feature on.
    "--disable-field-trial-config",
    // Crash reports.
    "--disable-breakpad",
    // Component and default extensions, and their update checks.
    "--disable-extensions",
    "--disable-component-extensions-with-background-pages",
    // Domain Reliability's error reports to Google.
    "--disable-domain-reliability",
    // Hyperlink auditing (`<a ping>`).
    "--no-pings",
    // The component updater: `--disable-component-update` skips only Chrome's main component
    // list, and components registered elsewhere (the on-device model manifest) still send
    // update checks at start-up to update.googleapis.com (Chromium fixed it after 157; Google
    // Chrome on CI did it within 1 s). Every check goes to the one URL this sets, and port 1 is
    // on Chrome's unsafe-port list, so each fails before any connection.
    "--component-updater=url-source=https://update.invalid:1/",
    // GCM: the enterprise policy invalidation listeners, made for every desktop profile with no
    // switch of their own, check in to android.clients.google.com, register there and hold
    // mtalk.google.com:5228 open. A URL Chrome cannot load fails each step on the spot, with no
    // connection. Web push, which needs GCM, does not work.
    "--gcm-checkin-url=about:blank",
    "--gcm-registration-url=about:blank",
    "--gcm-mcs-endpoint=about:blank",
    // Sign-in: Chrome fetches accounts.google.com/ListAccounts at start up (its sync metrics
    // recorder) whatever `--disable-sync` says. Port 1 is on Chrome's unsafe-port list, so the
    // fetch fails before any connection. The agent's own visits to accounts.google.com are
    // unaffected; Chrome's own sign-in, never used here, cannot work.
    "--gaia-url=https://gaia.invalid:1",
    // Chrome isolates its Gaia origin in a process of its own; with `--gaia-url` moved, this
    // keeps that for accounts.google.com.
    "--isolate-origins=https://accounts.google.com",
];

/// `Default/Preferences` entries, merged into the profile's: services with a preference and no
/// switch.
pub(crate) fn preferences() -> serde_json::Value {
    serde_json::json!({
        // Safe Browsing's list updates, URL lookups and download pings.
        "safebrowsing": {"enabled": false, "scout_reporting_enabled": false},
        "translate": {"enabled": false},
        // No preconnect or prefetch of links and search results.
        "net": {"network_prediction_options": 2},
        // No suggestions from Google for error pages or the omnibox.
        "alternate_error_pages": {"enabled": false},
        "search": {"suggest_enabled": false},
        // No dictionary download from Google.
        "browser": {"enable_spellchecking": false},
        "url_keyed_anonymized_data_collection": {"enabled": false},
        "autofill": {"profile_enabled": false, "credit_card_enabled": false},
    })
}
