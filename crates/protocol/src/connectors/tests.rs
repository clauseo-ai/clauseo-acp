//! The connector types on the wire: the API key never encodes or prints, and the state and the
//! hint's event read back.

use zeroize::Zeroizing;

use super::{
    Balance, ConnectorErrorKind, ConnectorSignInMethod, ConnectorStatus, ConnectorView,
    ConnectorsState, REDACTED_TEXT, RechargeOutcome, RedactedString,
};
use crate::{CommandEnvelope, CommandId, ConnectorCommand, HostCommand, HostEvent};

/// The one error a connector command that does not decode gives.
const UNREADABLE: &str = "a connector command that could not be read";

/// A key that must never show anywhere but where it is stored.
const KEY: &str = "ik_live_3f9a0c51e2d84b7aa6d1f00ddeadbeef";

fn key(text: &str) -> RedactedString {
    RedactedString::new(Zeroizing::new(text.to_owned())).unwrap()
}

fn add_key(text: &str) -> HostCommand {
    HostCommand::Connector(ConnectorCommand::AddApiKey {
        id: "case-law".to_owned(),
        key: key(text),
    })
}

fn envelope(command: HostCommand) -> CommandEnvelope {
    CommandEnvelope {
        command_id: CommandId::new(uuid::Uuid::from_u128(7)).unwrap(),
        command,
    }
}

#[test]
fn the_key_prints_and_encodes_as_redacted() {
    let command = add_key(KEY);
    for text in [
        format!("{command:?}"),
        format!("{command:#?}"),
        format!("{:?}", envelope(command.clone())),
        serde_json::to_string(&command).unwrap(),
        serde_json::to_string(&envelope(command.clone())).unwrap(),
        String::from_utf8_lossy(&rmp_serde::to_vec_named(&command).unwrap()).into_owned(),
    ] {
        assert!(!text.contains(KEY), "{text}");
        assert!(text.contains(REDACTED_TEXT), "{text}");
    }
    assert_eq!(
        serde_json::to_string(&command).unwrap(),
        r#"{"connector":{"add_api_key":{"id":"case-law","key":"[redacted]"}}}"#
    );
    // The key is still there for the one place that stores it.
    let HostCommand::Connector(ConnectorCommand::AddApiKey { key, .. }) = command else {
        panic!("not an add");
    };
    assert_eq!(key.expose(), KEY);
}

#[test]
fn the_key_decodes_from_a_plain_string_and_compares_by_its_text() {
    let json = format!(r#"{{"connector":{{"add_api_key":{{"id":"case-law","key":"{KEY}"}}}}}}"#);
    let command: HostCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(command, add_key(KEY));
    assert_ne!(command, add_key("another"));
}

#[test]
fn a_key_out_of_bounds_fails_to_decode_without_quoting_it() {
    let long = "k".repeat(crate::MAX_SECRET_BYTES + 1);
    for bad in ["", long.as_str()] {
        let json =
            format!(r#"{{"connector":{{"add_api_key":{{"id":"case-law","key":"{bad}"}}}}}}"#);
        let error = serde_json::from_str::<HostCommand>(&json)
            .unwrap_err()
            .to_string();
        assert!(error.contains(UNREADABLE), "{error}");
        assert!(bad.is_empty() || !error.contains(bad), "{error}");
    }
}

#[test]
fn an_encoded_command_decodes_to_a_placeholder_that_validation_refuses() {
    let json = serde_json::to_string(&envelope(add_key(KEY))).unwrap();
    let decoded: CommandEnvelope = serde_json::from_str(&json).unwrap();
    assert!(decoded.validate().is_err());
    assert!(envelope(add_key(KEY)).validate().is_ok());
}

#[test]
fn ids_are_checked_by_shape() {
    for bad in ["", "Case-Law", "case law", "../x", &"a".repeat(65)] {
        let command = HostCommand::Connector(ConnectorCommand::SignOut { id: bad.to_owned() });
        assert!(envelope(command).validate().is_err(), "{bad:?}");
        let credits = HostCommand::Connector(ConnectorCommand::AddCredits {
            id: "case-law".to_owned(),
            pack: bad.to_owned(),
        });
        assert!(envelope(credits).validate().is_err(), "{bad:?}");
    }
    let ok = HostCommand::Connector(ConnectorCommand::AddCredits {
        id: "case-law".to_owned(),
        pack: "pack_500".to_owned(),
    });
    assert!(envelope(ok).validate().is_ok());
}

/// A misshapen command is refused with one fixed message that quotes none of the input: serde's
/// own would echo `invalid type: string "<key>"` back to the window.
#[test]
fn a_misshapen_connector_command_never_quotes_the_key_in_its_error() {
    let shapes = [
        serde_json::json!({"connector": {"add_api_key": KEY}}),
        serde_json::json!({"connector": {"add_api_key": {"id": KEY, "key": 7}}}),
        serde_json::json!({"connector": {"add_api_key": {"id": "case-law", "key": KEY, KEY: 1}}}),
        serde_json::json!({"connector": {KEY: {}}}),
        serde_json::json!({"connector": KEY}),
    ];
    for shape in shapes {
        let json = shape.to_string();
        let packed = rmp_serde::to_vec_named(&shape).unwrap();
        let wrapped = serde_json::json!({"command_id": uuid::Uuid::from_u128(7), "command": shape})
            .to_string();
        let errors = [
            serde_json::from_str::<ConnectorCommand>(&shape["connector"].to_string())
                .unwrap_err()
                .to_string(),
            serde_json::from_str::<HostCommand>(&json)
                .unwrap_err()
                .to_string(),
            serde_json::from_str::<CommandEnvelope>(&wrapped)
                .unwrap_err()
                .to_string(),
            rmp_serde::from_slice::<HostCommand>(&packed)
                .unwrap_err()
                .to_string(),
        ];
        for error in errors {
            assert!(!error.contains(KEY), "{error}");
            assert!(error.contains(UNREADABLE), "{error}");
        }
    }
}

#[test]
fn a_connector_command_refuses_unknown_fields() {
    for json in [
        r#"{"connector":{"sign_out":{"id":"case-law","extra":1}}}"#,
        r#"{"connector":{"sign_in_somehow":{"id":"case-law"}}}"#,
    ] {
        assert!(serde_json::from_str::<HostCommand>(json).is_err(), "{json}");
    }
}

#[test]
fn the_state_and_the_hint_read_back() {
    let state = ConnectorsState {
        available: true,
        locked: false,
        connectors: vec![ConnectorView {
            id: "case-law".to_owned(),
            name: "Indian case law".to_owned(),
            price_paise: None,
            packs: Vec::new(),
            recent_spend: None,
            status: ConnectorStatus::SignedIn {
                account_label: None,
                balance: Some(Balance {
                    credits: 180,
                    paise: Some(2_124),
                    as_of: None,
                }),
                low: true,
                method: None,
            },
            pay_url: None,
            last_recharge: None,
        }],
    };
    let event = HostEvent::ConnectorsChanged(state);
    let json = serde_json::to_string(&event).unwrap();
    assert_eq!(serde_json::from_str::<HostEvent>(&json).unwrap(), event);
    assert!(
        !json.contains("pay_url")
            && !json.contains("account_label")
            && !json.contains("as_of")
            && !json.contains("method")
            && !json.contains("last_recharge"),
        "{json}"
    );
}

/// R24's wire-gaps fields are additive: an older host's state (none of them) still decodes,
/// and each new enum is written in snake case.
#[test]
fn the_wire_gaps_fields_are_additive() {
    let older: ConnectorStatus =
        serde_json::from_str(r#"{"error":{"message":"the sign-in did not finish"}}"#).unwrap();
    assert_eq!(
        older,
        ConnectorStatus::Error {
            message: "the sign-in did not finish".to_owned(),
            kind: None,
        }
    );
    let older: ConnectorStatus =
        serde_json::from_str(r#"{"signed_in":{"balance":{"credits":5},"low":true}}"#).unwrap();
    assert_eq!(
        older,
        ConnectorStatus::SignedIn {
            account_label: None,
            balance: Some(Balance {
                credits: 5,
                paise: None,
                as_of: None,
            }),
            low: true,
            method: None,
        }
    );
    let older: ConnectorView = serde_json::from_str(
        r#"{"id":"case-law","name":"Indian case law","packs":[],"status":"not_added"}"#,
    )
    .unwrap();
    assert_eq!(older.last_recharge, None);
    let words = |value: serde_json::Value| value.as_str().unwrap().to_owned();
    for (value, want) in [
        (
            serde_json::to_value(ConnectorErrorKind::SignInFailed),
            "sign_in_failed",
        ),
        (
            serde_json::to_value(ConnectorErrorKind::PersistFailed),
            "persist_failed",
        ),
        (
            serde_json::to_value(ConnectorSignInMethod::Account),
            "account",
        ),
        (
            serde_json::to_value(ConnectorSignInMethod::ApiKey),
            "api_key",
        ),
        (serde_json::to_value(RechargeOutcome::Paid), "paid"),
        (serde_json::to_value(RechargeOutcome::Expired), "expired"),
        (
            serde_json::to_value(RechargeOutcome::Cancelled),
            "cancelled",
        ),
    ] {
        assert_eq!(words(value.unwrap()), want);
    }
}
