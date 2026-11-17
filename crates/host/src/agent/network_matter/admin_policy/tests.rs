//! When Claude Code's administrator policy tier counts as present (`admin_policy.rs`).

use super::AdminDoc::{Json, Plist};
use super::{AdminDoc, admin_policy_present};

fn json(text: &str) -> AdminDoc {
    Json(Ok(Some(text.as_bytes().to_vec())))
}

fn plist(text: &str) -> AdminDoc {
    Plist(Ok(Some(text.as_bytes().to_vec())))
}

const PLIST_HEAD: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "#,
    r#""http://www.apple.com/DTDs/PropertyList-1.0.dtd">"#,
    r#"<plist version="1.0">"#,
);

/// No document, documents that are not there, and empty ones: no tier.
#[test]
fn absent_or_empty_documents_are_no_tier() {
    assert!(!admin_policy_present(&[]));
    assert!(!admin_policy_present(&[Json(Ok(None)), Plist(Ok(None))]));
    assert!(!admin_policy_present(&[
        json("{}"),
        json("\u{feff} { }\n"),
        plist(""),
        plist(&format!("{PLIST_HEAD}<dict/></plist>")),
    ]));
}

/// Any document with content makes a tier, whatever it holds.
#[test]
fn a_document_with_content_is_a_tier() {
    for doc in [
        json(r#"{"permissions":{"ask":["Read(./secret/**)"]}}"#),
        json(r#"{"env":{"HTTPS_PROXY":"http://proxy:8080"}}"#),
        plist(&format!(
            "{PLIST_HEAD}<dict><key>model</key><string>x</string></dict></plist>"
        )),
        json(r#"{"permissions":{"deny":["WebFetch"]}}"#),
    ] {
        let label = format!("{doc:?}");
        assert!(admin_policy_present(&[Json(Ok(None)), doc]), "{label}");
    }
}

/// A document that is there but can't be read or parsed fails toward a tier.
#[test]
fn unreadable_or_unparsable_documents_are_a_tier() {
    for doc in [
        Json(Err(())),
        Plist(Err(())),
        json(""),
        json("{"),
        json("[]"),
        json("null"),
        plist("not a property list"),
        plist(&format!("{PLIST_HEAD}<array/></plist>")),
    ] {
        let label = format!("{doc:?}");
        assert!(admin_policy_present(&[doc]), "{label}");
    }
}
