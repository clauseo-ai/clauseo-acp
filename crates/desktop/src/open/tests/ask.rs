//! Which links `open_external` asks about: a plain web link opens with no confirm; a mailto
//! link, a user name before the host, an international host and link text naming another site
//! ask. The gate still holds both paths.

use super::*;

/// `url` opened as the command opens it with visible text `shown`: how many confirms it drew, and
/// whether it opened. The confirm accepts.
fn confirms(url: &str, shown: Option<&str>) -> usize {
    let confirm = FakeConfirm::new(true);
    let opener = FakeOpener::default();
    external(&confirm, &opener, &Clock::new(), Language::En, url, shown).unwrap();
    assert_eq!(opener.opened(), [Opened::Url(check_link(url).unwrap().url)]);
    confirm.shown().len()
}

#[test]
fn a_plain_web_link_opens_with_no_confirm() {
    assert_eq!(confirms("https://example.com/a", None), 0);
    assert_eq!(confirms("http://example.com/a", None), 0);
    assert_eq!(
        confirms("https://example.com/a", Some("https://example.com/a")),
        0
    );
}

#[test]
fn a_mailto_link_asks() {
    assert_eq!(confirms("mailto:clerk@court.example", None), 1);
    assert_eq!(
        confirms("mailto:clerk@court.example", Some("clerk@court.example")),
        1
    );
}

/// A user name before the host is refused by the link check before anything is drawn; were it
/// let through, the decision would still ask.
#[test]
fn a_user_name_before_the_host_asks() {
    let link = Link {
        url: "https://bank.example@evil.example/".into(),
        destination: Destination::Web("evil.example".into()),
    };
    assert!(needs_confirm(&link, None));
    let link = Link {
        url: "https://user:pw@example.com/".into(),
        destination: Destination::Web("example.com".into()),
    };
    assert!(needs_confirm(&link, Some("example.com")));

    let confirm = FakeConfirm::new(true);
    let opener = FakeOpener::default();
    let refused = external(
        &confirm,
        &opener,
        &Clock::new(),
        Language::En,
        "https://bank.example@evil.example/",
        None,
    );
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    assert_eq!(confirm.shown(), []);
    assert_eq!(opener.opened(), []);
}

#[test]
fn an_international_host_asks() {
    // Cyrillic а in "apple": the url crate writes it xn--.
    assert_eq!(confirms("https://аpple.com/", None), 1);
    assert_eq!(confirms("https://xn--pple-43d.com/", None), 1);
    assert_eq!(confirms("https://docs.XN--pple-43d.com/", None), 1);
    // A non-ASCII host as given to the decision directly.
    let link = Link {
        url: "https://exämple.com/".into(),
        destination: Destination::Web("exämple.com".into()),
    };
    assert!(needs_confirm(&link, None));
}

#[test]
fn text_naming_the_same_site_opens_with_no_confirm() {
    assert_eq!(
        confirms("https://example.com/x", Some("www.example.com")),
        0
    );
    assert_eq!(
        confirms("https://www.example.com/x", Some("Example.COM")),
        0
    );
    assert_eq!(
        confirms(
            "https://supremecourt.gov.in/y",
            Some("https://supremecourt.gov.in/x")
        ),
        0
    );
    assert_eq!(
        confirms("https://example.com/", Some("docs.example.com")),
        0
    );
    assert_eq!(
        confirms("https://docs.example.com/", Some("example.com")),
        0
    );
    assert_eq!(
        confirms("https://example.com/", Some("See example.com, page 3.")),
        0
    );
}

#[test]
fn text_naming_no_site_opens_with_no_confirm() {
    for shown in [
        "the judgment",
        "here",
        "",
        "Section 3.2",
        "e.g. the order",
        "Rs. 500",
        "U.S.A.",
        "page 12.",
    ] {
        assert_eq!(confirms("https://example.com/", Some(shown)), 0, "{shown}");
    }
}

#[test]
fn text_naming_another_site_asks() {
    assert_eq!(confirms("https://evil.com/", Some("example.com")), 1);
    assert_eq!(
        confirms("https://evil.com/", Some("https://example.com/x")),
        1
    );
    assert_eq!(
        confirms("https://evil.com/", Some("Visit bank.example.com now")),
        1
    );
    // A suffix that is not a subdomain.
    assert_eq!(confirms("https://notexample.com/", Some("example.com")), 1);
    // One of two names is another site.
    assert_eq!(
        confirms("https://example.com/", Some("example.com or evil.com")),
        1
    );
    // A look-alike in another script, full-width letters and dots, and hidden characters.
    assert_eq!(confirms("https://evil.com/", Some("ехаmple.com")), 1);
    assert_eq!(
        confirms("https://evil.com/", Some("ｅｘａｍｐｌｅ。ｃｏｍ")),
        1
    );
    assert_eq!(
        confirms("https://evil.com/", Some("exam\u{200B}ple.com")),
        1
    );
    // Full-width text that names the real site still matches it.
    assert_eq!(
        confirms("https://example.com/", Some("ｅｘａｍｐｌｅ。ｃｏｍ")),
        0
    );
    // Text too long to read asks.
    let long = "a ".repeat(ask_limit());
    assert_eq!(confirms("https://example.com/", Some(&long)), 1);
}

fn ask_limit() -> usize {
    crate::open::ask::MAX_SHOWN_BYTES
}

#[test]
fn shown_hosts_finds_each_host_in_ascii() {
    let hosts = crate::open::ask::shown_hosts("See https://SupremeCourt.gov.in/x and ехаmple.com.");
    assert_eq!(hosts.len(), 2, "{hosts:?}");
    assert_eq!(hosts[0], "supremecourt.gov.in");
    assert!(
        hosts[1].starts_with("xn--") && hosts[1].strip_suffix(".com").is_some(),
        "{hosts:?}"
    );
    assert_eq!(
        crate::open::ask::shown_hosts("the judgment of 12.03.2024"),
        Vec::<String>::new()
    );
}

/// A declined confirm still opens nothing, fails with `transport.unauthorized` and quiets the
/// gate, for a plain link too; a link opened with no confirm holds the next for `BETWEEN_OPENS`.
#[test]
fn the_gate_holds_with_and_without_a_confirm() {
    let clock = Clock::new();
    let opener = FakeOpener::default();
    let gate = gate::Gate::default();
    let plain = check_link("https://example.com/").unwrap();
    let mail = check_link("mailto:clerk@court.example").unwrap();
    let run = |confirm: &FakeConfirm, link: &Link| {
        block_on(open_external(
            confirm,
            &opener,
            &gate,
            &|| clock.now(),
            Language::En,
            link,
            None,
        ))
    };
    let declining = FakeConfirm::new(false);
    let accepting = FakeConfirm::new(true);

    let declined = run(&declining, &mail).unwrap_err();
    assert_eq!(declined.code(), ErrorCode::TransportUnauthorized);
    assert_eq!(declining.shown().len(), 1);
    assert_eq!(opener.opened(), []);
    // Quiet after the decline, a plain link included.
    assert_eq!(code(run(&accepting, &plain)), ErrorCode::CommandConflict);
    assert_eq!(code(run(&accepting, &mail)), ErrorCode::CommandConflict);
    clock.advance(AFTER_DECLINE);

    // A plain link opens with no confirm, and holds the next for BETWEEN_OPENS.
    run(&accepting, &plain).unwrap();
    assert_eq!(accepting.shown(), []);
    assert_eq!(code(run(&accepting, &plain)), ErrorCode::CommandConflict);
    clock.advance(BETWEEN_OPENS);
    run(&accepting, &plain).unwrap();
    assert_eq!(opener.opened().len(), 2);

    // While a call is in progress, a plain link waits too.
    clock.advance(BETWEEN_OPENS);
    let pass = gate.enter(clock.now()).unwrap();
    assert_eq!(code(run(&accepting, &plain)), ErrorCode::CommandConflict);
    drop(pass);
    run(&accepting, &plain).unwrap();
    assert_eq!(accepting.shown(), []);
}
