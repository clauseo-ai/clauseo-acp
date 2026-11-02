//! The fixtures in `crates/connectors/fixtures/`, compiled in, and their origin swap.

use serde_json::Value;

use crate::known::PRODUCTION_ORIGIN;

/// Every fixture, by its path under `fixtures/`.
const FILES: &[(&str, &str)] = &[
    (
        "prm-case-law.json",
        include_str!("../../fixtures/prm-case-law.json"),
    ),
    (
        "as-metadata.json",
        include_str!("../../fixtures/as-metadata.json"),
    ),
    (
        "client-metadata.json",
        include_str!("../../fixtures/client-metadata.json"),
    ),
    ("token.json", include_str!("../../fixtures/token.json")),
    (
        "token-rotated.json",
        include_str!("../../fixtures/token-rotated.json"),
    ),
    (
        "token-invalid-grant.json",
        include_str!("../../fixtures/token-invalid-grant.json"),
    ),
    (
        "pricing-live.json",
        include_str!("../../fixtures/pricing-live.json"),
    ),
    (
        "pricing-preview.json",
        include_str!("../../fixtures/pricing-preview.json"),
    ),
    ("account.json", include_str!("../../fixtures/account.json")),
    (
        "account-open-payment.json",
        include_str!("../../fixtures/account-open-payment.json"),
    ),
    (
        "account-unknown-fields.json",
        include_str!("../../fixtures/account-unknown-fields.json"),
    ),
    (
        "recharge-created.json",
        include_str!("../../fixtures/recharge-created.json"),
    ),
    (
        "recharge-paid.json",
        include_str!("../../fixtures/recharge-paid.json"),
    ),
    ("api-402.json", include_str!("../../fixtures/api-402.json")),
    (
        "mcp/initialize.json",
        include_str!("../../fixtures/mcp/initialize.json"),
    ),
    (
        "mcp/tools-list.json",
        include_str!("../../fixtures/mcp/tools-list.json"),
    ),
    (
        "mcp/search-law-result.json",
        include_str!("../../fixtures/mcp/search-law-result.json"),
    ),
    (
        "mcp/read-document-result.json",
        include_str!("../../fixtures/mcp/read-document-result.json"),
    ),
    (
        "mcp/insufficient-credits-result.json",
        include_str!("../../fixtures/mcp/insufficient-credits-result.json"),
    ),
];

/// A fixture's text as it is on disk (every URL on `https://data.clauseo.chat`), by its path
/// under `crates/connectors/fixtures/` (`"account.json"`, `"mcp/tools-list.json"`).
pub fn fixture(name: &str) -> Option<&'static str> {
    FILES
        .iter()
        .find(|(file, _)| *file == name)
        .map(|(_, text)| *text)
}

/// A fixture's text with the production origin replaced by `base`.
pub(super) fn text(name: &str, base: &str) -> Option<String> {
    fixture(name).map(|text| text.replace(PRODUCTION_ORIGIN, base))
}

/// [`text`] parsed; `Null` for a missing fixture.
pub(super) fn value(name: &str, base: &str) -> Value {
    text(name, base)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fixture_is_listed_and_parses() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let mut on_disk = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                for inner in std::fs::read_dir(&path).unwrap() {
                    let name = inner.unwrap().file_name().into_string().unwrap();
                    on_disk.push(format!(
                        "{}/{name}",
                        path.file_name().unwrap().to_str().unwrap()
                    ));
                }
            } else if path.extension().is_some_and(|ext| ext == "json") {
                on_disk.push(path.file_name().unwrap().to_str().unwrap().to_owned());
            }
        }
        on_disk.sort();
        let mut listed: Vec<String> = FILES.iter().map(|(name, _)| (*name).to_owned()).collect();
        listed.sort();
        assert_eq!(listed, on_disk);
        for (name, text) in FILES {
            let value: Value = serde_json::from_str(text).unwrap();
            assert!(value.is_object(), "{name}");
        }
    }

    #[test]
    fn text_swaps_the_origin() {
        let text = text("prm-case-law.json", "http://127.0.0.1:9").unwrap();
        assert!(text.contains("http://127.0.0.1:9/mcp/case-law"));
        assert!(!text.contains(PRODUCTION_ORIGIN));
    }
}
