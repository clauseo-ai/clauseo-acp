//! The client's name, the date and an update's field change: what each reads and refuses.

use super::{ClientName, Date, FieldChange, MatterStatus};
use crate::{MAX_TITLE_CHARS, Timestamp};

#[test]
fn a_client_name_is_trimmed_then_held_to_the_title_rules() {
    assert_eq!(
        ClientName::new("  Acme Ltd \t").unwrap().as_str(),
        "Acme Ltd"
    );
    let decoded: ClientName = serde_json::from_str(r#"" Rathi Steels ""#).unwrap();
    assert_eq!(decoded.as_str(), "Rathi Steels");
    assert_eq!(
        serde_json::to_string(&decoded).unwrap(),
        r#""Rathi Steels""#
    );
    for refused in ["", "   ", "a\u{0}b", "a\u{202E}b"] {
        assert!(ClientName::new(refused).is_err(), "{refused:?}");
        let json = serde_json::to_string(refused).unwrap();
        assert!(
            serde_json::from_str::<ClientName>(&json).is_err(),
            "{refused:?}"
        );
    }
    let longest = "a".repeat(MAX_TITLE_CHARS);
    assert!(ClientName::new(&format!("  {longest}  ")).is_ok());
    assert!(ClientName::new(&format!("{longest}a")).is_err());
}

#[test]
fn a_date_is_a_day_that_exists_in_iso_form() {
    let date: Date = "2024-02-29".parse().unwrap();
    assert_eq!((date.year(), date.month(), date.day()), (2024, 2, 29));
    assert_eq!(date.to_string(), "2024-02-29");
    assert_eq!(serde_json::to_string(&date).unwrap(), r#""2024-02-29""#);
    assert_eq!(
        serde_json::from_str::<Date>(r#""0001-01-01""#)
            .unwrap()
            .year(),
        1
    );
    assert!("2000-02-29".parse::<Date>().is_ok(), "2000 is a leap year");
    for refused in [
        "2023-02-29",
        "1900-02-29",
        "2024-04-31",
        "2024-13-01",
        "2024-00-10",
        "2024-01-00",
        "0000-01-01",
        "2024-1-01",
        "2024/01/01",
        "24-01-01",
        "2024-01-01T00:00",
        "+024-01-01",
        "2024-01-0x",
        "",
    ] {
        assert!(refused.parse::<Date>().is_err(), "{refused:?}");
        let json = serde_json::to_string(refused).unwrap();
        assert!(serde_json::from_str::<Date>(&json).is_err(), "{refused:?}");
    }
    let earlier: Date = "2024-12-31".parse().unwrap();
    let later: Date = "2025-01-01".parse().unwrap();
    assert!(earlier < later);
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    #[serde(default, skip_serializing_if = "FieldChange::is_keep")]
    client: FieldChange<ClientName>,
}

#[test]
fn a_field_change_reads_missing_as_keep_null_as_clear_and_a_value_as_set() {
    let keep: Update = serde_json::from_str("{}").unwrap();
    assert_eq!(keep.client, FieldChange::Keep);
    let clear: Update = serde_json::from_str(r#"{"client": null}"#).unwrap();
    assert_eq!(clear.client, FieldChange::Clear);
    let set: Update = serde_json::from_str(r#"{"client": "Acme"}"#).unwrap();
    assert_eq!(
        set.client,
        FieldChange::Set(ClientName::new("Acme").unwrap())
    );
    assert!(serde_json::from_str::<Update>(r#"{"client": "  "}"#).is_err());
    for update in [keep, clear, set] {
        let json = serde_json::to_string(&update).unwrap();
        assert_eq!(
            serde_json::from_str::<Update>(&json).unwrap(),
            update,
            "{json}"
        );
    }
}

#[test]
fn a_status_is_a_string_or_a_one_key_object() {
    let at = Timestamp::new(1_700_000_000_000).unwrap();
    let cases = [
        (MatterStatus::Idle, r#""idle""#),
        (MatterStatus::NeedsPermission, r#""needs_permission""#),
        (
            MatterStatus::Stopped { at },
            r#"{"stopped":{"at":1700000000000}}"#,
        ),
        (
            MatterStatus::PausedAtLimit { resets_at: None },
            r#"{"paused_at_limit":{}}"#,
        ),
    ];
    for (status, json) in cases {
        assert_eq!(serde_json::to_string(&status).unwrap(), json);
        assert_eq!(serde_json::from_str::<MatterStatus>(json).unwrap(), status);
    }
}
