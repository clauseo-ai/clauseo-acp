//! The relay's refusal of every command that would let a page past a certificate error
//! (`routes.rs`, [`super::routes::certificate_bypass`]).

use serde_json::{Value, json};

use super::routes::{CARRIES_COMMANDS, CERTIFICATE_BYPASS, FromClient, Routes};

/// The session ids each command is tried on: none, Chromium's two spellings of the root, a page's.
fn sessions() -> [Option<Value>; 4] {
    [
        None,
        Some(json!("")),
        Some(Value::Null),
        Some(json!("PAGE-SESSION")),
    ]
}

/// Command `id` 7 of `method` with `params` (none when `Value::Null`), on `session`.
fn command(method: &str, params: &Value, session: Option<&Value>) -> String {
    let mut message = json!({"id": 7, "method": method});
    if !params.is_null() {
        message["params"] = params.clone();
    }
    if let Some(session) = session {
        message["sessionId"] = session.clone();
    }
    message.to_string()
}

/// Every command that would skip a certificate error is refused with a CDP error naming why, on
/// the root session and on a page's, and never reaches the browser.
#[test]
fn a_certificate_bypass_is_refused_on_every_session() {
    let inner = json!({"id": 1, "method": "Security.setIgnoreCertificateErrors",
        "params": {"ignore": true}});
    let refused = [
        (
            "Security.setIgnoreCertificateErrors",
            json!({"ignore": true}),
            CERTIFICATE_BYPASS,
        ),
        (
            "Security.setIgnoreCertificateErrors",
            json!({"ignore": 1}),
            CERTIFICATE_BYPASS,
        ),
        (
            "Security.setIgnoreCertificateErrors",
            json!({}),
            CERTIFICATE_BYPASS,
        ),
        (
            "Security.setIgnoreCertificateErrors",
            Value::Null,
            CERTIFICATE_BYPASS,
        ),
        (
            "Security.setOverrideCertificateErrors",
            json!({"override": true}),
            CERTIFICATE_BYPASS,
        ),
        (
            "Security.setOverrideCertificateErrors",
            json!({"override": null}),
            CERTIFICATE_BYPASS,
        ),
        (
            "Security.handleCertificateError",
            json!({"eventId": 1, "action": "continue"}),
            CERTIFICATE_BYPASS,
        ),
        (
            "Security.handleCertificateError",
            json!({"eventId": 1}),
            CERTIFICATE_BYPASS,
        ),
        (
            "Target.sendMessageToTarget",
            json!({"sessionId": "S", "message": inner.to_string()}),
            CARRIES_COMMANDS,
        ),
        (
            "Target.exposeDevToolsProtocol",
            json!({"targetId": "T"}),
            CARRIES_COMMANDS,
        ),
    ];
    for (method, params, why) in &refused {
        for session in sessions() {
            let sent = command(method, params, session.as_ref());
            let FromClient::Refuse(reply) = Routes::default().client_message(&sent) else {
                panic!("{sent} was not refused");
            };
            let reply: Value = serde_json::from_str(&reply).unwrap();
            assert_eq!(reply["id"], 7, "{reply}");
            assert_eq!(reply["error"]["code"], -32600, "{reply}");
            assert_eq!(reply["error"]["message"], *why, "{reply}");
        }
    }
}

/// The same commands turning a bypass off pass, as do other `Security` and `Target` commands.
#[test]
fn turning_a_certificate_bypass_off_passes() {
    let allowed = [
        (
            "Security.setIgnoreCertificateErrors",
            json!({"ignore": false}),
        ),
        (
            "Security.setOverrideCertificateErrors",
            json!({"override": false}),
        ),
        (
            "Security.handleCertificateError",
            json!({"eventId": 1, "action": "cancel"}),
        ),
        ("Security.enable", Value::Null),
        ("Target.setDiscoverTargets", json!({"discover": true})),
    ];
    for (method, params) in &allowed {
        for session in sessions() {
            let sent = command(method, params, session.as_ref());
            let routed = Routes::default().client_message(&sent);
            assert!(
                matches!(routed, FromClient::Forward(_)),
                "{sent}: {routed:?}"
            );
        }
    }
}

/// A duplicate key means what the browser would read, the last one: the guard forwards its own
/// serialisation, so a refused value cannot hide behind an allowed one.
#[test]
fn a_duplicate_key_is_judged_as_the_browser_reads_it() {
    let method = r#""method":"Security.setIgnoreCertificateErrors""#;
    let hidden = format!(r#"{{"id":7,{method},"params":{{"ignore":false,"ignore":true}}}}"#);
    let routed = Routes::default().client_message(&hidden);
    assert!(matches!(routed, FromClient::Refuse(_)), "{routed:?}");
    let off = format!(r#"{{"id":7,{method},"params":{{"ignore":true,"ignore":false}}}}"#);
    let FromClient::Forward(forwarded) = Routes::default().client_message(&off) else {
        panic!("{off} was not forwarded");
    };
    let forwarded: Value = serde_json::from_str(&forwarded).unwrap();
    assert_eq!(forwarded["params"], json!({"ignore": false}));
}
