//! `WebCrypto`'s RS256 check for the Access JWT ([`crate::access::Verifier`]): the key is
//! imported from its JWK parts and `crypto.subtle.verify` checks the signature. The bindings
//! are declared here with wasm-bindgen, so the Worker reaches only these two functions and
//! `JSON.parse`, never the request. The macro's own path to `js_sys` (which worker-build asks
//! for) is the one use of that crate the source test allows.

use worker::wasm_bindgen::JsValue;
use worker::wasm_bindgen::prelude::wasm_bindgen;

use crate::access::{Key, Verifier};

#[wasm_bindgen(
    wasm_bindgen = worker::wasm_bindgen,
    wasm_bindgen_futures = worker::wasm_bindgen_futures,
    js_sys = worker::js_sys
)]
extern "C" {
    #[wasm_bindgen(js_namespace = JSON, js_name = parse, catch)]
    fn json_parse(text: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["crypto", "subtle"], js_name = importKey, catch)]
    async fn import_key(
        format: &str,
        key: &JsValue,
        algorithm: &JsValue,
        extractable: bool,
        usages: &JsValue,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["crypto", "subtle"], js_name = verify, catch)]
    async fn subtle_verify(
        algorithm: &JsValue,
        key: &JsValue,
        signature: &[u8],
        data: &[u8],
    ) -> Result<JsValue, JsValue>;
}

/// RSASSA-PKCS1-v1_5 with SHA-256, which is what RS256 names.
const ALGORITHM: &str = r#"{"name":"RSASSA-PKCS1-v1_5","hash":"SHA-256"}"#;

/// The Worker's signature check.
pub(crate) struct WebCrypto;

impl Verifier for WebCrypto {
    async fn verify(&self, key: &Key, signed: &[u8], signature: &[u8]) -> bool {
        rs256(key, signed, signature).await.unwrap_or(false)
    }
}

async fn rs256(key: &Key, signed: &[u8], signature: &[u8]) -> Result<bool, JsValue> {
    let jwk = serde_json::json!({
        "kty": "RSA",
        "n": key.n,
        "e": key.e,
        "alg": "RS256",
        "ext": true,
    });
    let jwk = json_parse(&jwk.to_string())?;
    let algorithm = json_parse(ALGORITHM)?;
    let usages = json_parse(r#"["verify"]"#)?;
    let crypto_key = import_key("jwk", &jwk, &algorithm, false, &usages).await?;
    let verified = subtle_verify(&algorithm, &crypto_key, signature, signed).await?;
    Ok(verified.as_bool() == Some(true))
}
