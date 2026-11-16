use super::*;
use crate::known::{CASE_LAW, CLIENT_ID, PRICING_URL};

#[test]
fn production_keeps_every_compiled_in_url() {
    let urls = Origin::Production.urls(&CASE_LAW);
    assert_eq!(urls.issuer, CASE_LAW.issuer);
    assert_eq!(urls.mcp_url, CASE_LAW.mcp_url);
    assert_eq!(urls.api_base, CASE_LAW.api_base);
    assert_eq!(urls.resources, CASE_LAW.resources);
    assert_eq!(urls.client_id, CLIENT_ID);
    assert_eq!(urls.pricing_url, PRICING_URL);
    assert_eq!(urls.pay_prefix, CASE_LAW.pay_prefix);
    assert_eq!(
        urls.protected_resource_metadata,
        "https://data.clauseo.chat/.well-known/oauth-protected-resource/mcp/case-law"
    );
    assert_eq!(
        urls.authorization_server_metadata,
        "https://data.clauseo.chat/.well-known/oauth-authorization-server"
    );
}

#[cfg(feature = "staging")]
#[test]
fn staging_maps_every_url_onto_its_host() {
    let urls = Origin::Staging.urls(&CASE_LAW);
    assert_eq!(urls.issuer, "https://data-staging.clauseo.chat");
    assert_eq!(
        urls.mcp_url,
        "https://data-staging.clauseo.chat/mcp/case-law"
    );
    assert_eq!(
        urls.resources,
        [
            "https://data-staging.clauseo.chat/mcp/case-law",
            "https://data-staging.clauseo.chat/v1/case-law"
        ]
    );
    assert_eq!(
        urls.client_id,
        "https://data-staging.clauseo.chat/oauth/clients/clauseo-desktop.json"
    );
    assert_eq!(
        urls.pricing_url,
        "https://data-staging.clauseo.chat/pricing.json"
    );
    assert_eq!(urls.pay_prefix, "https://data-staging.clauseo.chat/pay/");
}

/// The local test server: every path of production on `http://127.0.0.1:8793`, the paths the
/// connector's owner listed.
#[cfg(feature = "local")]
#[test]
fn local_maps_every_url_onto_the_local_server() {
    let base = "http://127.0.0.1:8793";
    assert!(Origin::Local.allowed());
    let urls = Origin::Local.urls(&CASE_LAW);
    assert_eq!(urls.issuer, base);
    assert_eq!(urls.mcp_url, format!("{base}/mcp/case-law"));
    assert_eq!(urls.api_base, format!("{base}/v1/case-law"));
    assert_eq!(
        urls.resources,
        [
            format!("{base}/mcp/case-law"),
            format!("{base}/v1/case-law")
        ]
    );
    assert_eq!(
        urls.client_id,
        format!("{base}/oauth/clients/clauseo-desktop.json")
    );
    assert_eq!(urls.pricing_url, format!("{base}/pricing.json"));
    assert_eq!(urls.pay_prefix, format!("{base}/pay/"));
    assert_eq!(
        urls.protected_resource_metadata,
        format!("{base}/.well-known/oauth-protected-resource/mcp/case-law")
    );
    assert_eq!(
        urls.authorization_server_metadata,
        format!("{base}/.well-known/oauth-authorization-server")
    );
}

#[cfg(feature = "staging")]
#[test]
fn map_url_leaves_a_lookalike_host_alone() {
    let origin = Origin::Staging;
    assert_eq!(
        origin.map_url("https://data.clauseo.chat.evil.example/x"),
        "https://data.clauseo.chat.evil.example/x"
    );
}

#[cfg(feature = "test-support")]
#[test]
fn fake_maps_onto_loopback_and_refuses_anything_else() {
    let fake = Origin::Fake(url::Url::parse("http://127.0.0.1:4321").unwrap());
    assert!(fake.allowed());
    assert_eq!(fake.urls(&CASE_LAW).issuer, "http://127.0.0.1:4321");
    assert_eq!(
        fake.urls(&CASE_LAW).api_base,
        "http://127.0.0.1:4321/v1/case-law"
    );
    for bad in [
        "https://127.0.0.1:4321",
        "http://localhost:4321",
        "http://127.0.0.2:4321",
        "http://example.com:4321",
        "http://user@127.0.0.1:4321",
        "http://127.0.0.1:4321/sub",
        "http://127.0.0.1:4321/?q=1",
    ] {
        let origin = Origin::Fake(url::Url::parse(bad).unwrap());
        assert!(!origin.allowed(), "{bad}");
    }
}

#[test]
fn production_config_has_the_brief_s_defaults() {
    let config = Config::production("Clauseo/1.2.3".to_owned(), PathBuf::from("state.json"));
    assert_eq!(config.origin, Origin::Production);
    assert_eq!(config.proxy, ProxyChoice::Env);
    assert_eq!(config.signin_timeout, Duration::from_mins(5));
    assert_eq!(config.recharge_deadline, Duration::from_mins(15));
    assert_eq!(config.recharge_interval, Duration::from_secs(3));
    assert!(format!("{config:?}").contains("Clauseo/1.2.3"));
}
