use super::*;

/// The raw texts of a stream's values.
fn texts(body: &str) -> Vec<String> {
    parse_event_stream(body)
        .unwrap()
        .iter()
        .map(|raw| raw.get().to_owned())
        .collect()
}

#[test]
fn an_event_stream_gives_one_value_per_data_event() {
    let body = ": comment\r\nevent: message\r\nid: 1\r\ndata: {\"a\":1}\r\n\r\n\
                event: message\ndata:{\"b\":\ndata: 2}\n\nretry: 5\n\ndata: [3]";
    // Raw, as sent: the two `data:` lines of the second event joined with `\n`.
    assert_eq!(texts(body), [r#"{"a":1}"#, "{\"b\":\n2}", "[3]"]);
    // Whitespace around a value is trimmed; inside it, kept.
    assert_eq!(texts("data:  { \"c\" : 3 }  \n\n"), [r#"{ "c" : 3 }"#]);
}

#[test]
fn an_empty_stream_is_no_values_and_bad_data_is_a_parse_error() {
    assert_eq!(texts(""), Vec::<String>::new());
    assert_eq!(texts("event: ping\n\n"), Vec::<String>::new());
    assert!(matches!(
        parse_event_stream("data: {nope\n\n"),
        Err(Error::Parse(_))
    ));
}

/// The SHA-256 of `assets/case_law/initialize.json`: the server's `initialize` result for
/// 2025-06-18 at connectors main e9448d7. When the server's text changes, the files and these
/// hashes change together, in one reviewed commit.
const SHIPPED_INITIALIZE_SHA256: &str =
    "41680f7410943dc6578c2446a8dbb54ac557e40cd8763dda3f65c9e426e5360f";
/// The SHA-256 of `assets/case_law/tools_list.json` (sent with `MCP-Protocol-Version: 2025-06-18`).
const SHIPPED_TOOLS_LIST_SHA256: &str =
    "eb3beb0ba76f59e980ba72bcc74d6de4343bb7b7a4f150af54570a64543ad0ed";
/// The SHA-256 of `assets/case_law/tools_list_2025-03-26.json` (sent with no version header).
const SHIPPED_TOOLS_LIST_2025_03_26_SHA256: &str =
    "40bb0812b3252495f2b196ec8f2c26cf0e2dbcf78dbb5957afe31aee98770cdd";

fn sha256_hex(text: &str) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .fold(String::new(), |mut hex, byte| {
            use std::fmt::Write as _;
            write!(hex, "{byte:02x}").unwrap();
            hex
        })
}

#[test]
fn the_shipped_copies_are_the_pinned_bytes() {
    assert_eq!(sha256_hex(CASE_LAW_INITIALIZE), SHIPPED_INITIALIZE_SHA256);
    assert_eq!(sha256_hex(CASE_LAW_TOOLS_LIST), SHIPPED_TOOLS_LIST_SHA256);
    assert_eq!(
        sha256_hex(CASE_LAW_TOOLS_LIST_2025_03_26),
        SHIPPED_TOOLS_LIST_2025_03_26_SHA256
    );
    let initialize: Value = serde_json::from_str(CASE_LAW_INITIALIZE).unwrap();
    assert!(initialize["instructions"].is_string());
    for list in [CASE_LAW_TOOLS_LIST, CASE_LAW_TOOLS_LIST_2025_03_26] {
        let tools: Value = serde_json::from_str(list).unwrap();
        assert!(tools["tools"].is_array());
    }
}
