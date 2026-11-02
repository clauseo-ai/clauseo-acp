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
    let page = SUCCESS_PAGE.to_ascii_lowercase();
    for banned in [
        "<script", "src=", "href=", "url(", "@import", "http:", "https:",
    ] {
        assert!(!page.contains(banned), "{banned}");
    }
    assert!(SUCCESS_PAGE.contains("Signed in. You can close this tab and go back to Clauseo."));
}
