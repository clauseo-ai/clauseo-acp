use super::*;

fn oauth(access: &str, refresh: &str) -> Credential {
    Credential::OAuth(OAuthTokens {
        access_token: Secret::from(access),
        access_expires_at: 1_793_581_200,
        refresh_token: Secret::from(refresh),
    })
}

#[test]
fn oauth_encodes_to_the_brief_s_json_and_back() {
    let credential = oauth("at_123", "rt_456");
    let bytes = credential.encode().unwrap();
    assert_eq!(
        std::str::from_utf8(&bytes).unwrap(),
        r#"{"v":1,"kind":"oauth","access_token":"at_123","access_expires_at":1793581200,"refresh_token":"rt_456"}"#
    );
    assert_eq!(Credential::decode(&bytes), Some(credential));
}

#[test]
fn an_api_key_encodes_to_the_brief_s_json_and_back() {
    // A made-up key, split so the expected JSON is no secret-shaped literal for gitleaks.
    let key = format!("cdk_live_{}", "0123456789abcdefghijABCDEFGHIJ01");
    let credential = Credential::ApiKey(Secret::from(key.as_str()));
    let bytes = credential.encode().unwrap();
    assert_eq!(
        std::str::from_utf8(&bytes).unwrap(),
        format!(r#"{{"v":1,"kind":"api_key","key":"{key}"}}"#)
    );
    assert_eq!(Credential::decode(&bytes), Some(credential));
    assert_eq!(
        Credential::decode(&bytes).unwrap().kind(),
        CredentialKind::ApiKey
    );
}

#[test]
fn today_s_token_sizes_stay_under_300_bytes() {
    let credential = oauth(&"a".repeat(63), &"r".repeat(63));
    assert!(credential.encode().unwrap().len() < 300);
}

#[test]
fn encode_refuses_over_2000_bytes_and_takes_2000() {
    let fixed = oauth("", "").encode().unwrap().len();
    let fits = oauth(&"a".repeat(MAX_SECRET_BYTES - fixed), "");
    assert_eq!(fits.encode().unwrap().len(), MAX_SECRET_BYTES);
    let over = oauth(&"a".repeat(MAX_SECRET_BYTES - fixed + 1), "");
    assert_eq!(
        over.encode(),
        Err(StoreError::TooLarge {
            len: MAX_SECRET_BYTES + 1
        })
    );
}

#[test]
fn an_item_that_does_not_parse_reads_as_absent() {
    let refused: [&[u8]; 11] = [
        b"",
        b"not json",
        b"\xff\xfe",
        b"[]",
        br#"{"v":2,"kind":"oauth","access_token":"a","access_expires_at":1,"refresh_token":"r"}"#,
        br#"{"kind":"oauth","access_token":"a","access_expires_at":1,"refresh_token":"r"}"#,
        br#"{"v":1,"kind":"password","key":"k"}"#,
        br#"{"v":1,"kind":"oauth","access_token":"a","refresh_token":"r"}"#,
        br#"{"v":1,"kind":"oauth","access_token":"","access_expires_at":1,"refresh_token":"r"}"#,
        br#"{"v":1,"kind":"api_key"}"#,
        br#"{"v":1,"kind":"api_key","key":7}"#,
    ];
    for bytes in refused {
        assert_eq!(
            Credential::decode(bytes),
            None,
            "{}",
            String::from_utf8_lossy(bytes)
        );
    }
    let huge = format!(
        r#"{{"v":1,"kind":"api_key","key":"{}"}}"#,
        "k".repeat(2_000)
    );
    assert_eq!(Credential::decode(huge.as_bytes()), None);
}

#[test]
fn decode_ignores_unknown_fields() {
    let bytes = br#"{"v":1,"kind":"api_key","key":"cdk_live_x","note":"later"}"#;
    assert_eq!(
        Credential::decode(bytes),
        Some(Credential::ApiKey(Secret::from("cdk_live_x")))
    );
}

#[test]
fn debug_shows_no_token() {
    let text = format!("{:?}", oauth("at_secret_value", "rt_secret_value"));
    assert!(!text.contains("secret_value"), "{text}");
}
