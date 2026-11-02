//! `open_external`'s link check and confirm, its words against their review home, and the Rust-only
//! opener with no confirm.

use super::*;

#[test]
fn web_links_name_their_host_in_ascii() {
    let link = check_link("https://www.sci.gov.in/judgments?q=1#top").unwrap();
    assert_eq!(link.destination, Destination::Web("www.sci.gov.in".into()));
    assert_eq!(link.url, "https://www.sci.gov.in/judgments?q=1#top");

    let link = check_link("http://localhost:8080/x").unwrap();
    assert_eq!(link.destination, Destination::Web("localhost:8080".into()));

    // An international name shows in its xn-- form, so a look-alike cannot pass as another.
    let link = check_link("https://аpple.com/").unwrap();
    assert_eq!(
        link.destination,
        Destination::Web("xn--pple-43d.com".into())
    );
    assert_eq!(link.url, "https://xn--pple-43d.com/");

    // Upper case and spaces around are the url crate's to tidy; the host is what shows.
    let link = check_link("HTTPS://Example.COM").unwrap();
    assert_eq!(link.destination, Destination::Web("example.com".into()));
}

#[test]
fn a_mailto_link_names_its_addresses() {
    let link = check_link("mailto:clerk@court.example?subject=Filing&body=Hi").unwrap();
    assert_eq!(
        link.destination,
        Destination::Mail("clerk@court.example".into())
    );
    let link = check_link("mailto:a@x.example,b@y.example").unwrap();
    assert_eq!(
        link.destination,
        Destination::Mail("a@x.example,b@y.example".into())
    );
}

#[test]
fn other_links_are_refused() {
    let long = format!("https://example.com/{}", "a".repeat(MAX_LINK_BYTES));
    for bad in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "data:text/html,<script>1</script>",
        "ftp://example.com/x",
        "tauri://localhost/index.html",
        "smb://server/share",
        "x-apple.systempreferences:com.apple.preference.security",
        "ms-settings:privacy",
        "about:blank",
        "not a link",
        "",
        "/relative/path",
        "https://user:secret@example.com/",
        "https://example.com@evil.example/",
        "mailto:",
        "mailto:nobody",
        // A right-to-left override, and a space, in the address.
        "mailto:%E2%80%AEmoc.live@a",
        "mailto:a%20b@x.example",
        "mailto:%ZZ@x.example",
        long.as_str(),
    ] {
        let refused = check_link(bad);
        assert_eq!(code(refused), ErrorCode::CommandInvalid, "{bad}");
    }
}

#[test]
fn a_bad_scheme_is_refused_before_any_confirm() {
    let confirm = FakeConfirm::new(true);
    let opener = FakeOpener::default();
    let refused = block_on(open_external(
        &confirm,
        &opener,
        Language::En,
        "file:///Applications/Calculator.app",
    ));
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    assert_eq!(confirm.shown(), []);
    assert_eq!(opener.opened(), []);
}

#[test]
fn a_declined_confirm_opens_nothing_and_is_unauthorized() {
    let confirm = FakeConfirm::new(false);
    let opener = FakeOpener::default();
    let declined = block_on(open_external(
        &confirm,
        &opener,
        Language::En,
        "https://example.com/a",
    ));
    let error = declined.unwrap_err();
    assert_eq!(error.code(), ErrorCode::TransportUnauthorized);
    assert!(!error.retryable());
    assert_eq!(confirm.shown().len(), 1);
    assert_eq!(opener.opened(), []);
}

#[test]
fn an_accepted_confirm_opens_the_link_it_named() {
    let confirm = FakeConfirm::new(true);
    let opener = FakeOpener::default();
    block_on(open_external(
        &confirm,
        &opener,
        Language::En,
        "https://example.com/a",
    ))
    .unwrap();
    assert_eq!(
        confirm.shown(),
        [LinkWords {
            title: "Open this link in your browser?".into(),
            message: "The link goes to example.com.".into(),
            ok: "Open link".into(),
            cancel: "Cancel".into(),
        }]
    );
    assert_eq!(
        opener.opened(),
        [Opened::Url("https://example.com/a".into())]
    );
}

#[test]
fn the_confirm_speaks_the_apps_language() {
    let confirm = FakeConfirm::new(true);
    let opener = FakeOpener::default();
    block_on(open_external(
        &confirm,
        &opener,
        Language::Hi,
        "mailto:clerk@court.example",
    ))
    .unwrap();
    let shown = confirm.shown();
    assert_eq!(shown[0].title, words::HI.mail_title);
    assert_eq!(
        shown[0].message,
        "आपका ईमेल ऐप clerk@court.example के लिए नया संदेश खोलेगा।"
    );
    assert_eq!(shown[0].ok, words::HI.mail_ok);
    assert_eq!(shown[0].cancel, words::HI.cancel);
}

/// Every word of the confirm, in both languages, appears verbatim in its review home,
/// `ui/app/src/review/registry-h11.tsx`, so the copy the reviewers approve is what Rust draws.
#[test]
fn the_confirms_words_are_the_reviewed_copy() {
    let registry = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/app/src/review/registry-h11.tsx");
    let registry = std::fs::read_to_string(&registry).unwrap();
    for table in [&words::EN, &words::HI] {
        for (id, text) in [
            ("H11-01", table.web_title),
            ("H11-02", table.web_message),
            ("H11-03", table.web_ok),
            ("H11-04", table.cancel),
            ("H11-05", table.mail_title),
            ("H11-06", table.mail_message),
            ("H11-07", table.mail_ok),
        ] {
            assert!(registry.contains(id), "registry-h11.tsx has no {id}");
            let quoted = format!("\"{text}\"");
            assert!(
                registry.contains(&quoted),
                "registry-h11.tsx lacks {id}: {quoted}"
            );
        }
    }
}

#[test]
fn the_unconfirmed_opener_opens_https_only_and_asks_nobody() {
    let opener = FakeOpener::default();
    open_url_unconfirmed(&opener, "https://claude.ai/oauth/authorize?code=x").unwrap();
    for bad in [
        "http://claude.ai/",
        "mailto:a@b.example",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://user:pw@claude.ai/",
    ] {
        assert_eq!(
            code(open_url_unconfirmed(&opener, bad)),
            ErrorCode::CommandInvalid,
            "{bad}"
        );
    }
    assert_eq!(
        opener.opened(),
        [Opened::Url(
            "https://claude.ai/oauth/authorize?code=x".into()
        )]
    );
}
