//! The Access check's claims and pinning, with a stand-in signature check: a token whose
//! signature is the bytes `ok:` and its signed part verifies, anything else does not.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use serde_json::{Value, json};

use super::*;

const TEAM: &str = "clauseo";
const AUD: &str = "query-aud";
const NOW: u64 = 1_800_000_000;

struct Stand;

impl Verifier for Stand {
    fn verify(&self, key: &Key, signed: &[u8], signature: &[u8]) -> impl Future<Output = bool> {
        std::future::ready(key.n == "modulus" && signature == [b"ok:".as_slice(), signed].concat())
    }
}

/// Runs a future that never waits, as every one here is.
fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("the stand-in never waits"),
    }
}

fn b64(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}

fn claims() -> Value {
    json!({
        "aud": [AUD],
        "exp": NOW + 600,
        "iat": NOW - 10,
        "nbf": NOW - 10,
        "iss": "https://clauseo.cloudflareaccess.com",
        "type": "app",
        "sub": "",
    })
}

/// A token with this header and claims, signed the way [`Stand`] accepts.
fn token(header: &Value, claims: &Value) -> String {
    let signed = format!(
        "{}.{}",
        b64(header.to_string().as_bytes()),
        b64(claims.to_string().as_bytes())
    );
    let signature = [b"ok:".as_slice(), signed.as_bytes()].concat();
    format!("{signed}.{}", b64(&signature))
}

fn rs256() -> Value {
    json!({ "alg": "RS256", "kid": "k1", "typ": "JWT" })
}

fn keys() -> Vec<Key> {
    vec![Key {
        kid: "k1".to_owned(),
        n: "modulus".to_owned(),
        e: "AQAB".to_owned(),
    }]
}

fn access() -> Access {
    Access::new(TEAM, AUD).unwrap()
}

fn run(jwt: Option<&str>) -> Result<(), Denied> {
    ready(check(jwt, &access(), &keys(), NOW, &Stand))
}

fn with(edit: impl FnOnce(&mut Value)) -> Result<(), Denied> {
    let mut body = claims();
    edit(&mut body);
    run(Some(&token(&rs256(), &body)))
}

#[test]
fn a_good_token_passes() {
    assert_eq!(run(Some(&token(&rs256(), &claims()))), Ok(()));
    // A single-string aud is read too.
    assert_eq!(with(|c| c["aud"] = json!(AUD)), Ok(()));
    assert_eq!(with(|c| c["aud"] = json!(["other", AUD])), Ok(()));
}

#[test]
fn only_rs256_is_taken() {
    for alg in [
        "none", "HS256", "HS384", "HS512", "RS384", "ES256", "PS256", "rs256", "",
    ] {
        let header = json!({ "alg": alg, "kid": "k1" });
        assert_eq!(
            run(Some(&token(&header, &claims()))),
            Err(Denied::Algorithm),
            "{alg}"
        );
    }
    // An unsigned token, `alg: none` with an empty signature.
    let none = format!(
        "{}.{}.",
        b64(br#"{"alg":"none","kid":"k1"}"#),
        b64(claims().to_string().as_bytes())
    );
    assert_eq!(run(Some(&none)), Err(Denied::Algorithm));
    let empty_signature = format!(
        "{}.{}.",
        b64(rs256().to_string().as_bytes()),
        b64(claims().to_string().as_bytes())
    );
    assert_eq!(run(Some(&empty_signature)), Err(Denied::Signature));
}

#[test]
fn the_claims_are_checked() {
    assert_eq!(with(|c| c["exp"] = json!(NOW)), Err(Denied::Expired));
    assert_eq!(with(|c| c["exp"] = json!(NOW - 1)), Err(Denied::Expired));
    assert_eq!(with(|c| c["nbf"] = json!(NOW + 600)), Err(Denied::Expired));
    assert_eq!(
        with(|c| c["nbf"] = json!(NOW + 30)),
        Ok(()),
        "within the skew"
    );
    assert_eq!(
        with(|c| c["iss"] = json!("https://other.cloudflareaccess.com")),
        Err(Denied::Issuer)
    );
    assert_eq!(
        with(|c| c["iss"] = json!("https://clauseo.cloudflareaccess.com/")),
        Err(Denied::Issuer)
    );
    assert_eq!(
        with(|c| c["aud"] = json!(["dash-aud"])),
        Err(Denied::Audience)
    );
    assert_eq!(with(|c| c["aud"] = json!([])), Err(Denied::Audience));
    for missing in ["exp", "iss", "aud"] {
        let result = with(|c| {
            c.as_object_mut().unwrap().remove(missing);
        });
        assert_eq!(result, Err(Denied::Malformed), "{missing}");
    }
}

#[test]
fn the_kid_must_be_the_teams() {
    let header = json!({ "alg": "RS256", "kid": "k2" });
    assert_eq!(run(Some(&token(&header, &claims()))), Err(Denied::Kid));
    let header = json!({ "alg": "RS256" });
    assert_eq!(run(Some(&token(&header, &claims()))), Err(Denied::Kid));
}

#[test]
fn a_bad_signature_or_shape_is_refused() {
    let good = token(&rs256(), &claims());
    let (signed, _) = good.rsplit_once('.').unwrap();
    assert_eq!(
        run(Some(&format!("{signed}.{}", b64(b"forged")))),
        Err(Denied::Signature)
    );
    // The claims changed after signing.
    let mut other = claims();
    other["aud"] = json!(["dash-aud", AUD]);
    let (_, signature) = good.rsplit_once('.').unwrap();
    let (header, _) = signed.split_once('.').unwrap();
    let swapped = format!("{header}.{}.{signature}", b64(other.to_string().as_bytes()));
    assert_eq!(run(Some(&swapped)), Err(Denied::Signature));
    for bad in ["", "a.b", "a.b.c.d", "!!.??.**", &"a".repeat(9_000)] {
        assert_eq!(run(Some(bad)), Err(Denied::Malformed), "{bad:.20}");
    }
    assert_eq!(run(None), Err(Denied::Malformed));
}

#[test]
fn settings_fail_closed() {
    assert_eq!(Access::new("", AUD), None);
    assert_eq!(Access::new(TEAM, ""), None);
    assert_eq!(Access::new("evil.example/x", AUD), None);
    assert_eq!(Access::new("Clauseo", AUD), None);
    assert_eq!(
        access().certs_url(),
        "https://clauseo.cloudflareaccess.com/cdn-cgi/access/certs"
    );
}

#[test]
fn the_certs_document_gives_its_rs256_signing_keys() {
    let doc = json!({
        "keys": [
            { "kid": "k1", "kty": "RSA", "alg": "RS256", "use": "sig", "e": "AQAB", "n": "abc" },
            { "kid": "k2", "kty": "RSA", "e": "AQAB", "n": "def" },
            { "kid": "k3", "kty": "EC", "crv": "P-256", "x": "x", "y": "y" },
            { "kid": "k4", "kty": "RSA", "alg": "RS512", "e": "AQAB", "n": "ghi" },
            { "kid": "k5", "kty": "RSA", "use": "enc", "e": "AQAB", "n": "jkl" },
            { "kty": "RSA", "e": "AQAB", "n": "mno" }
        ],
        "public_cert": { "kid": "k1", "cert": "-----BEGIN CERTIFICATE-----" }
    });
    let keys = parse_keys(doc.to_string().as_bytes()).unwrap();
    let kids: Vec<&str> = keys.iter().map(|key| key.kid.as_str()).collect();
    assert_eq!(kids, ["k1", "k2"]);
    assert_eq!(parse_keys(b"not json"), None);
}

#[test]
fn the_keys_are_kept_an_hour_and_refetched_for_a_new_kid() {
    let url = access().certs_url();
    let cache = KeyCache {
        url: url.clone(),
        fetched: NOW,
        keys: keys(),
    };
    assert!(!cache.stale(&url, "k1", NOW + 3_599));
    assert!(cache.stale(&url, "k1", NOW + 3_600));
    assert!(
        !cache.stale(&url, "k9", NOW + 59),
        "a minute before a refetch"
    );
    assert!(cache.stale(&url, "k9", NOW + 60));
    assert!(cache.stale("https://other/", "k1", NOW));
    assert!(KeyCache::default().stale(&url, "k1", NOW));
}

#[test]
fn the_dev_certificate_url_is_loopback_only() {
    for good in [
        "http://127.0.0.1:8788/certs",
        "http://localhost:9000/cdn-cgi/access/certs",
    ] {
        assert_eq!(dev_certs_url(good), Some(good));
    }
    for bad in [
        "",
        "https://127.0.0.1:8788/certs",
        "http://127.0.0.1.evil.example/certs",
        "http://127.0.0.1:80@evil.example/",
        "http://localhost/certs",
        "http://10.0.0.1:80/certs",
        "https://clauseo.cloudflareaccess.com/cdn-cgi/access/certs",
    ] {
        assert_eq!(dev_certs_url(bad), None, "{bad}");
    }
}
