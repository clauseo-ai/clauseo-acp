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
    assert_eq!(
        opener.opened(),
        [Opened::Url(check_link(url).unwrap().url.to_string())]
    );
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
        url: url::Url::parse("https://bank.example@evil.example/").unwrap(),
        destination: Destination::Web("evil.example".into()),
    };
    assert!(needs_confirm(&link, None));
    let link = Link {
        url: url::Url::parse("https://user:pw@example.com/").unwrap(),
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
        url: url::Url::parse("https://exämple.com/").unwrap(),
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
            Ask::IfMisleading(None),
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

/// `open_external` refuses `url` before any confirm, opening nothing.
fn refused(url: &str, shown: Option<&str>) {
    let confirm = FakeConfirm::new(true);
    let opener = FakeOpener::default();
    let refused = external(&confirm, &opener, &Clock::new(), Language::En, url, shown);
    assert_eq!(code(refused), ErrorCode::CommandInvalid, "{url}");
    assert_eq!(confirm.shown(), [], "{url}");
    assert_eq!(opener.opened(), [], "{url}");
}

/// Host names match label by label, never by a prefix or a bare suffix; a trailing `.` on
/// either side is the same name.
#[test]
fn hosts_match_label_by_label() {
    assert_eq!(confirms("https://bank.com.evil.io/", Some("bank.com")), 1);
    assert_eq!(confirms("https://evilbank.com/", Some("bank.com")), 1);
    assert_eq!(confirms("https://bank.co/", Some("bank.com")), 1);
    assert_eq!(confirms("https://bank.com./", Some("bank.com")), 0);
    assert_eq!(confirms("https://bank.com/", Some("bank.com.")), 0);
    assert_eq!(
        confirms("https://login.bank.com./x", Some("www.bank.com.")),
        0
    );
}

/// A host that is an address asks, however the parser that opens it reads it; one on this
/// computer or its network is refused outright.
#[test]
fn an_address_host_asks() {
    for url in [
        "https://93.184.215.14/",
        "http://1572394766/",
        "http://0x5DB8D70E/",
        "http://0135.0270.0327.016/",
        "http://0x5d.184.0327.14/",
        "https://[2606:4700::1111]/",
    ] {
        assert_eq!(confirms(url, None), 1, "{url}");
        assert_eq!(confirms(url, Some("93.184.215.14")), 1, "{url}");
    }
    // 192.168.1.1 as dotted, decimal, hex and octal; loopback IPv6.
    for url in [
        "http://192.168.1.1/",
        "http://3232235777/",
        "http://0xC0A80101/",
        "http://0300.0250.01.01/",
        "http://[::1]/",
    ] {
        refused(url, None);
    }
}

/// What the check reads is what opens: the one parsed URL, written back. A backslash or a
/// percent-encoded dot in the host is read as the parser reads it for the OS, so text naming
/// the site it imitates asks.
#[test]
fn the_url_checked_is_the_url_opened() {
    for (url, opened) in [
        ("https:\\\\evil.com\\x", "https://evil.com/x"),
        ("https://bank.com%2Eevil.io/", "https://bank.com.evil.io/"),
        ("https://BANK.com/a b", "https://bank.com/a%20b"),
    ] {
        let link = check_link(url).unwrap();
        assert_eq!(link.url.as_str(), opened);
        let confirm = FakeConfirm::new(true);
        let opener = FakeOpener::default();
        external(
            &confirm,
            &opener,
            &Clock::new(),
            Language::En,
            url,
            Some("bank.com"),
        )
        .unwrap();
        assert_eq!(opener.opened(), [Opened::Url(opened.into())]);
        let asked = usize::from(!opened.starts_with("https://bank.com/"));
        assert_eq!(confirm.shown().len(), asked, "{url}");
    }
}

/// `javascript:`, `data:` and `file:` links never open, whatever their text.
#[test]
fn script_data_and_file_links_never_open() {
    for url in [
        "javascript:alert(document.cookie)",
        "JavaScript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "file:///etc/passwd",
        "file://server/share/x.docx",
    ] {
        refused(url, None);
        refused(url, Some("example.com"));
    }
    // The refusal names the scheme, never the link.
    let error = check_link("javascript:alert(document.cookie)").unwrap_err();
    assert!(!error.message().contains("cookie"), "{}", error.message());
}

/// Text that holds a direction override or isolate, or a character that shows nothing, asks
/// even when what it seems to name is the link's own site; so does text naming two sites when
/// either is another.
#[test]
fn hidden_or_reordering_text_asks() {
    for mark in [
        '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}',
        '\u{2068}', '\u{2069}', '\u{200B}', '\u{200D}', '\u{FEFF}',
    ] {
        let shown = format!("example.com{mark}");
        assert_eq!(
            confirms("https://example.com/", Some(&shown)),
            1,
            "{mark:?}"
        );
        let shown = format!("the {mark}judgment");
        assert_eq!(
            confirms("https://example.com/", Some(&shown)),
            1,
            "{mark:?}"
        );
    }
    assert_eq!(
        confirms("https://example.com/", Some("example.com, evil.com")),
        1
    );
    assert_eq!(
        confirms("https://example.com/", Some("evil.com, example.com")),
        1
    );
    assert_eq!(
        confirms(
            "https://example.com/",
            Some("example.com, docs.example.com")
        ),
        0
    );
}

/// Iris's pairs on #1007: one character outside ASCII hides `bank.com` from the host finder, so
/// text that seems to name it, on a link to `evil.io`, must ask. Fail closed: no list of them.
#[test]
fn one_hidden_character_cannot_hide_the_site_named() {
    for shown in [
        "bank\u{FE0F}.com",
        "bank.c\u{FE0F}om",
        "bank\u{034F}.com",
        "bank\u{E0020}.com",
        "bank.\u{E0020}com",
        "bank\u{17B4}.com",
        "bank\u{180B}.com",
        "bank\u{2065}.com",
        "bank\u{FFF9}.com",
        "bank\u{200A}.com",
        "bank\u{2009}.com",
        "bank\u{A4F8}com",
        "bank\u{0702}com",
        "bank\u{0701}com",
        "bank\u{00B7}com",
        "bank\u{0387}com",
        "bank\u{2E33}com",
        "bank\u{2219}com",
        "bank\u{22C5}com",
        "bank\u{06D4}com",
        "bank\u{0589}com",
        "bank\u{00A0}.com",
        "bank\u{2024}com",
        "Visit bank\u{00B7}com today",
    ] {
        assert_eq!(confirms("https://evil.io/", Some(shown)), 1, "{shown:?}");
    }
}

/// A look-alike dot in text with no ASCII beside it is read as `.` before hosts are found.
#[test]
fn a_look_alike_dot_reads_as_a_dot() {
    let hosts = crate::open::ask::shown_hosts("ｂａｎｋ\u{A4F8}ｃｏｍ");
    assert_eq!(hosts, ["bank.com"]);
    assert_eq!(
        confirms("https://evil.io/", Some("ｂａｎｋ\u{A4F8}ｃｏｍ")),
        1
    );
    assert_eq!(
        confirms("https://bank.com/", Some("ｂａｎｋ\u{A4F8}ｃｏｍ")),
        0
    );
}

/// An image in the link's text (U+FFFC, from to-react.ts) asks: its pixels could read as a site.
#[test]
fn an_image_in_the_text_asks() {
    assert_eq!(confirms("https://evil.io/", Some("\u{FFFC}")), 1);
    assert_eq!(
        confirms("https://example.com/", Some("the \u{FFFC} order")),
        1
    );
}

/// Words in Hindi, and curly quotes, dashes, the section and rupee signs touching letters or
/// digits, open unasked; any other character outside ASCII touching ASCII asks (`Résumé`).
#[test]
fn plain_words_outside_ascii_open_and_mixed_ones_ask() {
    for shown in [
        "निर्णय दिनांक 12.03.2024",
        "the “judgment”",
        "the court’s order",
        "pages 3–5 and §12",
        "₹500 fee",
        "«the order»",
    ] {
        assert_eq!(confirms("https://example.com/", Some(shown)), 0, "{shown}");
    }
    for shown in ["Résumé", "bank“.com", "bank.–com", "naïve"] {
        assert_eq!(confirms("https://example.com/", Some(shown)), 1, "{shown}");
    }
}

/// `Ask::Always` asks for a link that would open unasked, and can never remove a confirm.
#[test]
fn always_ask_adds_a_confirm() {
    let clock = Clock::new();
    let opener = FakeOpener::default();
    let confirm = FakeConfirm::new(true);
    for (url, ask) in [
        ("https://example.com/", Ask::Always),
        ("mailto:a@b.example", Ask::IfMisleading(None)),
    ] {
        let link = check_link(url).unwrap();
        let gate = gate::Gate::default();
        block_on(open_external(
            &confirm,
            &opener,
            &gate,
            &|| clock.now(),
            Language::En,
            &link,
            ask,
        ))
        .unwrap();
    }
    assert_eq!(confirm.shown().len(), 2);
    assert_eq!(opener.opened().len(), 2);
}
