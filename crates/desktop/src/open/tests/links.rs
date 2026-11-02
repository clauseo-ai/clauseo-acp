//! `open_external`'s link check and confirm, its words against their review home, and the Rust-only
//! opener with no confirm.

use super::*;

#[test]
fn web_links_name_their_host_in_ascii() {
    let link = check_link("https://www.sci.gov.in/judgments?q=1#top").unwrap();
    assert_eq!(link.destination, Destination::Web("www.sci.gov.in".into()));
    assert_eq!(link.url, "https://www.sci.gov.in/judgments?q=1#top");

    let link = check_link("http://www.example.com:8080/x").unwrap();
    assert_eq!(
        link.destination,
        Destination::Web("www.example.com:8080".into())
    );

    // Public addresses show as written back.
    let link = check_link("https://93.184.215.14/").unwrap();
    assert_eq!(link.destination, Destination::Web("93.184.215.14".into()));
    let link = check_link("https://[2606:4700::1111]/").unwrap();
    assert_eq!(
        link.destination,
        Destination::Web("[2606:4700::1111]".into())
    );

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
    // The two fields it may carry, in any case.
    check_link("mailto:a@x.example?SUBJECT=s&Body=b").unwrap();
}

/// A mailto link opens only to the addresses the confirm names: no header that adds a
/// recipient or an attachment, no address that is not local@domain, nothing a mail app's command
/// line could take for more.
#[test]
fn a_mailto_link_that_hides_anything_is_refused() {
    for bad in [
        // Recipients and attachments the confirm would not name.
        "mailto:@?to=x@y.example",
        "mailto:a@b.example?bcc=c@d.example",
        "mailto:a@b.example?cc=c@d.example",
        "mailto:a@b.example?to=c@d.example",
        "mailto:a@b.example?attach=/etc/passwd",
        "mailto:a@b.example?attachment=/etc/passwd",
        "mailto:a@b.example?from=boss@b.example",
        "mailto:a@b.example?reply-to=c@d.example",
        "mailto:a@b.example?subject=a&subject=b",
        "mailto:a@b.example#x",
        // Addresses that are not local@domain.
        "mailto:@",
        "mailto:a@",
        "mailto:@b.example",
        "mailto:a@b.example,",
        "mailto:a@b.example,@c.example",
        "mailto:a@b@c.example",
        // Quotes, angle brackets and backslashes.
        "mailto:a@b.example%22/x%3Cy%3E",
        "mailto:a%5Cb@c.example",
    ] {
        assert_eq!(code(check_link(bad)), ErrorCode::CommandInvalid, "{bad}");
    }
}

/// A link to this computer or its local network, and an overlong host name, are refused.
#[test]
fn local_and_overlong_hosts_are_refused() {
    let long_label = format!("https://{}.example/", "a".repeat(64));
    let long_host = format!("https://{}example/", "abcdefghi.".repeat(25));
    for bad in [
        "http://localhost:8080/x",
        "http://LOCALHOST./",
        "http://app.localhost/",
        "http://127.0.0.1/",
        "http://2130706433/",
        "http://10.0.0.1/",
        "http://172.16.5.4/",
        "http://192.168.1.1/admin",
        "http://169.254.169.254/latest/meta-data/",
        "http://0.0.0.0/",
        "http://255.255.255.255/",
        "http://[::1]/",
        "http://[::]/",
        "http://[fe80::1]/",
        "http://[fd00::1]/",
        "http://[::ffff:127.0.0.1]/",
        long_label.as_str(),
        long_host.as_str(),
    ] {
        assert_eq!(code(check_link(bad)), ErrorCode::CommandInvalid, "{bad}");
    }
}

/// The limit holds on the link that would open, after the url crate encodes it.
#[test]
fn the_length_limit_holds_after_encoding() {
    let grows = format!("https://example.com/{}", "é".repeat(600));
    assert!(grows.len() <= MAX_LINK_BYTES);
    assert_eq!(code(check_link(&grows)), ErrorCode::CommandInvalid);
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
    let refused = external(
        &confirm,
        &opener,
        &Clock::new(),
        Language::En,
        "file:///Applications/Calculator.app",
    );
    assert_eq!(code(refused), ErrorCode::CommandInvalid);
    assert_eq!(confirm.shown(), []);
    assert_eq!(opener.opened(), []);
}

#[test]
fn a_declined_confirm_opens_nothing_and_is_unauthorized() {
    let confirm = FakeConfirm::new(false);
    let opener = FakeOpener::default();
    let declined = external(
        &confirm,
        &opener,
        &Clock::new(),
        Language::En,
        "https://example.com/a",
    );
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
    external(
        &confirm,
        &opener,
        &Clock::new(),
        Language::En,
        "https://example.com/a",
    )
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
    external(
        &confirm,
        &opener,
        &Clock::new(),
        Language::Hi,
        "mailto:clerk@court.example",
    )
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

/// One confirm at a time, and none for a moment after a decline: a page cannot stack confirms
/// or put one back the moment it is cancelled.
#[test]
fn confirms_come_one_at_a_time_and_pause_after_a_decline() {
    let clock = Clock::new();
    let opener = FakeOpener::default();
    let gate = gate::Gate::default();
    let link = check_link("https://example.com/").unwrap();
    // While one is open, a second is refused before anything is drawn.
    let pass = gate.enter(clock.now()).unwrap();
    let confirm = FakeConfirm::new(true);
    let busy = block_on(open_external(
        &confirm,
        &opener,
        &gate,
        &|| clock.now(),
        Language::En,
        &link,
    ));
    assert_eq!(code(busy), ErrorCode::CommandConflict);
    assert_eq!(confirm.shown(), []);
    drop(pass);
    // Declined, the next is refused until AFTER_DECLINE has passed.
    let declining = FakeConfirm::new(false);
    let run = |confirm: &FakeConfirm| {
        block_on(open_external(
            confirm,
            &opener,
            &gate,
            &|| clock.now(),
            Language::En,
            &link,
        ))
    };
    assert_eq!(code(run(&declining)), ErrorCode::TransportUnauthorized);
    clock.advance(
        AFTER_DECLINE
            .checked_sub(std::time::Duration::from_millis(1))
            .unwrap(),
    );
    assert_eq!(code(run(&confirm)), ErrorCode::CommandConflict);
    assert_eq!(confirm.shown(), []);
    clock.advance(std::time::Duration::from_millis(1));
    run(&confirm).unwrap();
    // Accepted, the next may follow at once.
    run(&confirm).unwrap();
    assert_eq!(opener.opened().len(), 2);
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
