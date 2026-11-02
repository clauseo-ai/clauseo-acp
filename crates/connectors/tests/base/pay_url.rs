//! `PayUrl::parse` takes only the connector's own `https://data.clauseo.chat/pay/...`.

use clauseo_connectors::{CASE_LAW, Origin, PayUrl};

#[test]
fn our_pay_link_parses() {
    for text in [
        "https://data.clauseo.chat/pay/rch_1",
        "https://data.clauseo.chat/pay/rch_1?lang=hi",
        "https://data.clauseo.chat:443/pay/rch_1",
    ] {
        let url = PayUrl::parse(&CASE_LAW, text).unwrap();
        assert!(
            url.as_str()
                .starts_with("https://data.clauseo.chat/pay/rch_1"),
            "{text}"
        );
    }
}

#[test]
fn every_refusal_in_the_brief_gives_none() {
    let refused = [
        ("http", "http://data.clauseo.chat/pay/rch_1"),
        ("another host", "https://evil.example/pay/rch_1"),
        (
            "a lookalike host",
            "https://data.clauseo.chat.evil.example/pay/rch_1",
        ),
        ("a subdomain", "https://pay.data.clauseo.chat/pay/rch_1"),
        (
            "staging under production",
            "https://data-staging.clauseo.chat/pay/rch_1",
        ),
        ("a port", "https://data.clauseo.chat:8443/pay/rch_1"),
        ("user info", "https://user@data.clauseo.chat/pay/rch_1"),
        (
            "user and password",
            "https://user:pw@data.clauseo.chat/pay/rch_1",
        ),
        ("a path outside /pay/", "https://data.clauseo.chat/account"),
        ("/pay without a slash", "https://data.clauseo.chat/pay"),
        ("/payment/", "https://data.clauseo.chat/payment/rch_1"),
        ("nothing after /pay/", "https://data.clauseo.chat/pay/"),
        (
            "a dot-dot escape",
            "https://data.clauseo.chat/pay/../account",
        ),
        (
            "an encoded dot-dot",
            "https://data.clauseo.chat/pay/%2e%2e/account",
        ),
        ("another scheme", "javascript:alert(1)"),
        ("not a URL", "/pay/rch_1"),
        ("empty", ""),
    ];
    for (why, text) in refused {
        assert_eq!(PayUrl::parse(&CASE_LAW, text), None, "{why}: {text}");
    }
}

#[test]
fn staging_keeps_its_own_link_and_refuses_production() {
    let staged = PayUrl::parse_for(
        &CASE_LAW,
        &Origin::Staging,
        "https://data-staging.clauseo.chat/pay/rch_1",
    );
    assert_eq!(
        staged.unwrap().as_str(),
        "https://data-staging.clauseo.chat/pay/rch_1"
    );
    assert_eq!(
        PayUrl::parse_for(
            &CASE_LAW,
            &Origin::Staging,
            "https://data.clauseo.chat/pay/rch_1"
        ),
        None
    );
    assert_eq!(
        PayUrl::parse_for(
            &CASE_LAW,
            &Origin::Staging,
            "http://data-staging.clauseo.chat/pay/rch_1"
        ),
        None
    );
}

#[test]
fn the_fake_s_link_comes_back_in_the_production_form() {
    let origin = Origin::Fake(url::Url::parse("http://127.0.0.1:4321").unwrap());
    let url = PayUrl::parse_for(&CASE_LAW, &origin, "http://127.0.0.1:4321/pay/rch_1?x=1").unwrap();
    assert_eq!(url.as_str(), "https://data.clauseo.chat/pay/rch_1?x=1");
    for text in [
        "http://127.0.0.1:4322/pay/rch_1",
        "http://127.0.0.2:4321/pay/rch_1",
        "https://127.0.0.1:4321/pay/rch_1",
        "http://127.0.0.1:4321/other/rch_1",
        "https://data.clauseo.chat/pay/rch_1",
    ] {
        assert_eq!(PayUrl::parse_for(&CASE_LAW, &origin, text), None, "{text}");
    }
}
