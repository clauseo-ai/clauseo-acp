use super::*;

#[test]
fn a_token_answer_parses_with_any_case_of_bearer() {
    let body = br#"{"access_token":"at","token_type":"bEaReR","expires_in":3600,"refresh_token":"rt","x":1}"#;
    let tokens = parse_token_answer(body, 100, None).unwrap();
    assert_eq!(tokens.access_token.expose(), "at");
    assert_eq!(tokens.refresh_token.expose(), "rt");
    assert_eq!(tokens.access_expires_at, 3700);
}

#[test]
fn a_refresh_answer_without_a_refresh_token_keeps_the_old_one() {
    let body = br#"{"access_token":"at2","token_type":"Bearer","expires_in":"60"}"#;
    let old = Secret::from("rt1");
    let tokens = parse_token_answer(body, 0, Some(&old)).unwrap();
    assert_eq!(tokens.refresh_token.expose(), "rt1");
    assert_eq!(tokens.access_expires_at, 60);
    assert!(parse_token_answer(body, 0, None).is_err());
}

#[test]
fn a_token_answer_without_its_parts_is_refused_without_quoting_it() {
    for body in [
        &br#"{"access_token":"secret-at","token_type":"mac","expires_in":1,"refresh_token":"r"}"#[..],
        br#"{"token_type":"Bearer","expires_in":1,"refresh_token":"secret-rt"}"#,
        br#"{"access_token":"secret-at","token_type":"Bearer","refresh_token":"r"}"#,
        b"secret-at",
    ] {
        let err = parse_token_answer(body, 0, None).unwrap_err();
        assert!(!err.contains("secret"), "{err}");
    }
}

#[test]
fn an_error_code_is_kept_only_when_it_looks_like_one() {
    assert_eq!(
        oauth_error_code(br#"{"error":"invalid_grant"}"#),
        "invalid_grant"
    );
    assert_eq!(
        oauth_error_code(br#"{"error":"fake_rt_abc DEF","error_description":"x"}"#),
        "no error code"
    );
    assert_eq!(oauth_error_code(b"<html>"), "no error code");
}

#[test]
fn a_form_is_url_encoded_in_order() {
    let form = encode_form(&[("a", "b c"), ("resource", "https://x/y?z"), ("a", "&")]);
    assert_eq!(
        std::str::from_utf8(&form).unwrap(),
        "a=b+c&resource=https%3A%2F%2Fx%2Fy%3Fz&a=%26"
    );
}

#[test]
fn the_success_page_has_no_script_and_no_external_resource() {
    for page in [SUCCESS_PAGE, FAILURE_PAGE] {
        let page = page.to_ascii_lowercase();
        for banned in [
            "<script", "src=", "href=", "url(", "@import", "http:", "https:",
        ] {
            assert!(!page.contains(banned), "{banned}");
        }
    }
    // One style block and no style attribute: the policy allows that block by its hash only.
    for page in [SUCCESS_PAGE, FAILURE_PAGE] {
        assert_eq!(page.matches("<style>").count(), 1);
        assert!(!page.contains("style="));
    }
    // Each approved line is split at its sentence break into a heading and a body line, English
    // first, then the Hindi marked lang="hi".
    for (page, lines) in [
        (
            SUCCESS_PAGE,
            [
                "<h1>Signed in.</h1>",
                "<p>You can close this tab and go back to Clauseo.</p>",
                "<div lang=\"hi\"><p class=\"lead\">साइन इन हो गया।</p>",
                "<p>आप यह टैब बंद करके Clauseo पर लौट सकते हैं।</p>",
            ],
        ),
        (
            FAILURE_PAGE,
            [
                "<h1>Sign-in didn't finish.</h1>",
                "<p>Go back to Clauseo and try again.</p>",
                "<div lang=\"hi\"><p class=\"lead\">साइन इन पूरा नहीं हुआ।</p>",
                "<p>Clauseo पर लौटें और फिर से कोशिश करें।</p>",
            ],
        ),
    ] {
        let at = lines.map(|line| page.find(line).unwrap_or_else(|| panic!("{line}")));
        assert!(at.is_sorted(), "{lines:?}");
    }
}

#[test]
fn the_page_policy_names_the_style_hash() {
    use base64::engine::general_purpose::STANDARD;
    use sha2::{Digest as _, Sha256};
    // CSP hashes are standard base64 with padding, not the URL-safe form PKCE uses.
    for page in [SUCCESS_PAGE, FAILURE_PAGE] {
        let (_, rest) = page.split_once("<style>").unwrap();
        let (css, _) = rest.split_once("</style>").unwrap();
        let hash = STANDARD.encode(Sha256::digest(css.as_bytes()));
        assert!(
            listener::PAGE_CSP.contains(&format!("style-src 'sha256-{hash}';")),
            "the policy should name 'sha256-{hash}'"
        );
    }
    assert!(!listener::PAGE_CSP.contains("unsafe"));
    assert!(listener::PAGE_CSP.starts_with("default-src 'none'; "));
}
