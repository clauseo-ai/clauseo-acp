//! `PayUrl::parse` takes only the connector's own `https://data.clauseo.chat/pay/<id>`.

use clauseo_connectors::{CASE_LAW, Origin, PayUrl};

#[test]
fn our_pay_link_parses() {
    for text in [
        "https://data.clauseo.chat/pay/rch_1",
        "https://data.clauseo.chat/pay/RCH-1_a",
    ] {
        let url = PayUrl::parse(&CASE_LAW, text).unwrap();
        assert_eq!(url.as_str(), text);
    }
    let longest = format!("https://data.clauseo.chat/pay/{}", "a".repeat(128));
    assert!(PayUrl::parse(&CASE_LAW, &longest).is_some());
    let too_long = format!("https://data.clauseo.chat/pay/{}", "a".repeat(129));
    assert_eq!(PayUrl::parse(&CASE_LAW, &too_long), None);
}

#[test]
fn an_uppercase_host_is_taken_only_as_the_exact_lowercase_host() {
    // The url crate lowercases an ASCII host, so this is the exact host and is accepted, in
    // its lowercase form.
    let url = PayUrl::parse(&CASE_LAW, "https://DATA.Clauseo.CHAT/pay/rch_1").unwrap();
    assert_eq!(url.as_str(), "https://data.clauseo.chat/pay/rch_1");
}

#[test]
fn anything_the_parser_would_rewrite_is_refused() {
    let refused = [
        ("a backslash path", "https://data.clauseo.chat\\pay\\rch_1"),
        (
            "a backslash inside the path",
            "https://data.clauseo.chat/pay\\rch_1",
        ),
        ("/pay/../x", "https://data.clauseo.chat/pay/../x"),
        ("%2e%2e", "https://data.clauseo.chat/pay/%2e%2e/x"),
        ("an encoded slash", "https://data.clauseo.chat/pay/a%2Fb"),
        ("an encoded letter", "https://data.clauseo.chat/pay/rch%5F1"),
        (
            "an IDN look-alike (Cyrillic o)",
            "https://data.clause\u{43e}.chat/pay/rch_1",
        ),
        (
            "a trailing dot host",
            "https://data.clauseo.chat./pay/rch_1",
        ),
        ("a query", "https://data.clauseo.chat/pay/rch_1?lang=hi"),
        ("an empty query", "https://data.clauseo.chat/pay/rch_1?"),
        ("a fragment", "https://data.clauseo.chat/pay/rch_1#top"),
        (
            "port 443 written out",
            "https://data.clauseo.chat:443/pay/rch_1",
        ),
        ("a deeper path", "https://data.clauseo.chat/pay/rch_1/x"),
        ("a trailing slash", "https://data.clauseo.chat/pay/rch_1/"),
        ("a dot in the id", "https://data.clauseo.chat/pay/rch.1"),
    ];
    for (why, text) in refused {
        assert_eq!(PayUrl::parse(&CASE_LAW, text), None, "{why}: {text}");
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
    let url = PayUrl::parse_for(&CASE_LAW, &origin, "http://127.0.0.1:4321/pay/rch_1").unwrap();
    assert_eq!(url.as_str(), "https://data.clauseo.chat/pay/rch_1");
    for text in [
        "http://127.0.0.1:4321/pay/rch_1?x=1",
        "http://127.0.0.1:4322/pay/rch_1",
        "http://127.0.0.2:4321/pay/rch_1",
        "https://127.0.0.1:4321/pay/rch_1",
        "http://127.0.0.1:4321/other/rch_1",
        "https://data.clauseo.chat/pay/rch_1",
    ] {
        assert_eq!(PayUrl::parse_for(&CASE_LAW, &origin, text), None, "{text}");
    }
}
