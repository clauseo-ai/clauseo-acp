//! The URL rule (0041, `logged_url`): what a logged URL keeps and what it loses. The query stays,
//! because court and government sites put a document's identity there; credentials and tokens go.

use clauseo_protocol::logged_url;

/// `logged_url` of `url`, checked against `expected`.
#[track_caller]
fn logs_as(url: &str, expected: &str) {
    assert_eq!(logged_url(url), expected, "logging {url}");
}

#[test]
fn a_signed_s3_link_loses_every_x_amz_value() {
    let url = "https://bucket.s3.amazonaws.com/doc.pdf?X-Amz-Algorithm=AWS4-HMAC-SHA256\
        &X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20261102%2Fus-east-1%2Fs3%2Faws4_request\
        &X-Amz-Date=20261102T000000Z&X-Amz-Expires=3600&X-Amz-SignedHeaders=host\
        &X-Amz-Signature=5d41402abc4b2a76b9719d911017c592";
    logs_as(
        url,
        "https://bucket.s3.amazonaws.com/doc.pdf?X-Amz-Algorithm=REDACTED\
        &X-Amz-Credential=REDACTED&X-Amz-Date=REDACTED&X-Amz-Expires=REDACTED\
        &X-Amz-SignedHeaders=REDACTED&X-Amz-Signature=REDACTED",
    );
}

#[test]
fn a_google_signed_url_loses_every_x_goog_value() {
    let url = "https://storage.googleapis.com/bucket/obj.pdf?X-Goog-Algorithm=GOOG4-RSA-SHA256\
        &X-Goog-Credential=svc%40proj.iam.gserviceaccount.com%2F20261102%2Fauto%2Fstorage%2Fgoog4_request\
        &X-Goog-Date=20261102T000000Z&X-Goog-Expires=300&X-Goog-SignedHeaders=host\
        &X-Goog-Signature=0f1e2d3c4b5a69788796a5b4c3d2e1f0";
    logs_as(
        url,
        "https://storage.googleapis.com/bucket/obj.pdf?X-Goog-Algorithm=REDACTED\
        &X-Goog-Credential=REDACTED&X-Goog-Date=REDACTED&X-Goog-Expires=REDACTED\
        &X-Goog-SignedHeaders=REDACTED&X-Goog-Signature=REDACTED",
    );
}

#[test]
fn a_court_document_identity_in_the_query_stays() {
    logs_as(
        "https://main.sci.gov.in/judgment.php?diary_no=12345&id=67890",
        "https://main.sci.gov.in/judgment.php?diary_no=12345&id=67890",
    );
}

#[test]
fn userinfo_and_fragment_go_and_the_query_stays() {
    logs_as(
        "https://user:pw@example.com/a?b=1#frag",
        "https://example.com/a?b=1",
    );
}

#[test]
fn a_long_hex_value_under_an_innocent_name_is_redacted() {
    let hex = "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(hex.len(), 40);
    logs_as(
        &format!("https://example.com/view?ref={hex}&page=2"),
        "https://example.com/view?ref=REDACTED&page=2",
    );
}

#[test]
fn a_short_code_is_redacted_because_its_name_is_listed() {
    logs_as(
        "https://example.com/cb?code=7",
        "https://example.com/cb?code=REDACTED",
    );
}

#[test]
fn a_url_with_no_query_is_unchanged_but_for_its_fragment() {
    logs_as(
        "https://example.com/path/x#top",
        "https://example.com/path/x",
    );
    logs_as("https://example.com/path/x", "https://example.com/path/x");
}

#[test]
fn a_string_that_is_not_an_absolute_http_url_loses_only_its_fragment() {
    logs_as("/relative/path?x=1#f", "/relative/path?x=1");
    logs_as("ftp://host/a?token=abc#b", "ftp://host/a?token=abc");
    logs_as("not a url#frag", "not a url");
    logs_as("https://#x", "https://");
}

#[test]
fn names_match_without_regard_to_case_or_percent_encoding() {
    logs_as(
        "https://e.com/?Access_Token=abc",
        "https://e.com/?Access_Token=REDACTED",
    );
    logs_as(
        "https://e.com/?X-AMZ-Signature=abc",
        "https://e.com/?X-AMZ-Signature=REDACTED",
    );
    logs_as(
        "https://e.com/?%74oken=abc",
        "https://e.com/?%74oken=REDACTED",
    );
    logs_as("https://e.com/?pass=abc", "https://e.com/?pass=REDACTED");
}

#[test]
fn a_listed_name_matches_exactly_except_a_star_prefix() {
    logs_as(
        "https://e.com/?tokens=abc&passwordx=abc&xx-amz-date=abc&x-amz=abc",
        "https://e.com/?tokens=abc&passwordx=abc&xx-amz-date=abc&x-amz=abc",
    );
}

#[test]
fn a_long_base64url_value_is_redacted_and_a_short_one_is_not() {
    // 32 characters with `=` padding in the value's end: base64url.
    logs_as(
        "https://e.com/?v=dGhpcyBpcyBhIHRlc3QgdmFsdWUhIQ==",
        "https://e.com/?v=REDACTED",
    );
    // 31 characters of hex under an innocent name: below the length, kept.
    logs_as(
        "https://e.com/?v=0123456789abcdef0123456789abcde",
        "https://e.com/?v=0123456789abcdef0123456789abcde",
    );
    // A long value with a character outside base64url (`.`) is kept.
    logs_as(
        "https://e.com/?v=abcdefghijklmnopqrstuvwxyz0123.456",
        "https://e.com/?v=abcdefghijklmnopqrstuvwxyz0123.456",
    );
}

#[test]
fn everything_else_stays_byte_for_byte() {
    logs_as(
        "https://e.com/A%20b/%E2%9C%93?z=%20%E2%9C%93&a=1+2&m=&=&bare",
        "https://e.com/A%20b/%E2%9C%93?z=%20%E2%9C%93&a=1+2&m=&=&bare",
    );
    logs_as("https://e.com/?", "https://e.com/?");
    logs_as("HTTPS://E.com:8443/x", "HTTPS://E.com:8443/x");
}

#[test]
fn userinfo_ends_at_the_authority_and_port_stays() {
    logs_as("https://user@host:8443/x", "https://host:8443/x");
    logs_as("https://e.com/a@b", "https://e.com/a@b");
    logs_as("https://e.com?a=b@c", "https://e.com?a=b@c");
}

#[test]
fn a_bare_long_hex_parameter_is_redacted_and_a_bare_name_is_kept() {
    logs_as(
        "https://e.com/?0123456789abcdef0123456789abcdef01234567",
        "https://e.com/?REDACTED",
    );
    logs_as("https://e.com/?token", "https://e.com/?token");
}
