//! The upload's endpoint rules and what each POST carries. No request leaves the machine.

use super::*;

#[test]
fn only_a_plain_https_origin_is_an_endpoint() {
    assert_eq!(
        endpoint("https://t.clauseo.chat"),
        Some("https://t.clauseo.chat".to_owned())
    );
    assert_eq!(
        endpoint("https://127.0.0.1:8787/"),
        Some("https://127.0.0.1:8787".to_owned())
    );
    for refused in [
        "",
        "http://t.clauseo.chat",
        "ftp://t.clauseo.chat",
        "https://user@t.clauseo.chat",
        "https://user:pass@t.clauseo.chat",
        "https://t.clauseo.chat/v1",
        "https://t.clauseo.chat/?a=1",
        "https://t.clauseo.chat/#a",
        "t.clauseo.chat",
        "file:///etc/passwd",
    ] {
        assert_eq!(endpoint(refused), None, "{refused}");
    }
}

#[test]
fn release_builds_have_an_empty_endpoint_and_ignore_the_variable() {
    assert_eq!(RELEASE_ENDPOINT, "");
    assert_eq!(endpoint(RELEASE_ENDPOINT), None);
}

#[test]
fn a_post_names_only_its_three_headers_and_the_gzip_body() {
    let upload = HttpsUpload::to(Some("https://t.clauseo.chat"));
    let request = upload
        .request(UploadPath::Counts, vec![0x1f, 0x8b, 1, 2])
        .unwrap();
    assert_eq!(request.method(), reqwest::Method::POST);
    assert_eq!(request.url().as_str(), "https://t.clauseo.chat/v1/counts");
    let headers: Vec<(String, String)> = request
        .headers()
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_str().unwrap().to_owned()))
        .collect();
    assert_eq!(
        headers,
        [
            ("user-agent".to_owned(), "Clauseo".to_owned()),
            ("content-type".to_owned(), "application/json".to_owned()),
            ("content-encoding".to_owned(), "gzip".to_owned()),
        ]
    );
    assert_eq!(
        request.body().and_then(reqwest::Body::as_bytes),
        Some(&[0x1f, 0x8b, 1, 2][..])
    );
    let tally = upload.request(UploadPath::Tally, Vec::new()).unwrap();
    assert_eq!(tally.url().as_str(), "https://t.clauseo.chat/v1/tally");
}

#[test]
fn with_no_endpoint_a_post_fails_without_the_network() {
    let upload = HttpsUpload::to(None);
    assert!(upload.request(UploadPath::Counts, Vec::new()).is_none());
    let answer = futures::executor::block_on(upload.post(UploadPath::Counts, vec![1]));
    assert_eq!(answer, Err(UploadError));
    let plain_http = HttpsUpload::to(Some("http://127.0.0.1:1"));
    let answer = futures::executor::block_on(plain_http.post(UploadPath::Counts, vec![1]));
    assert_eq!(answer, Err(UploadError));
}
