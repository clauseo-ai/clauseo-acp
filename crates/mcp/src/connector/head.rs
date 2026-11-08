//! The two facts the connector reads from a JSON-RPC line: its `id`, and whether it has a
//! `method`. Nothing else is kept, so the connector never holds a request's body.

use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// A line's `id` (`None` when the key is absent; `Some(Value::Null)` for a null id) and whether it
/// has a `method` key.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Head {
    pub(crate) id: Option<Value>,
    pub(crate) method: bool,
}

/// The reader for the two fields. Every other field is skipped without being built.
#[derive(Deserialize)]
struct Fields {
    #[serde(default, deserialize_with = "keep")]
    id: Option<Value>,
    #[serde(default, deserialize_with = "flag")]
    method: bool,
}

/// Keeps an `id` as given, null included, so a present null is told from an absent key.
fn keep<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(de).map(Some)
}

/// Reports a `method` key as present, whatever its value.
fn flag<'de, D: Deserializer<'de>>(de: D) -> Result<bool, D::Error> {
    IgnoredAny::deserialize(de).map(|_| true)
}

/// The head of `line`, or `None` when it is not a JSON object.
pub(crate) fn read(line: &[u8]) -> Option<Head> {
    // serde reads a struct from a JSON array too, field by field; a batch is not a request.
    if line.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return None;
    }
    if let Ok(fields) = serde_json::from_slice::<Fields>(line) {
        return Some(Head {
            id: fields.id,
            method: fields.method,
        });
    }
    // A repeated key fails the reader above. The host keeps the last of each, so take that.
    let value: Value = serde_json::from_slice(line).ok()?;
    let object = value.as_object()?;
    Some(Head {
        id: object.get("id").cloned(),
        method: object.contains_key("method"),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Head, read};

    #[test]
    fn a_request_a_notification_and_a_response_read_as_their_fields() {
        let request = read(br#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"a":[1]}}"#);
        assert_eq!(
            request,
            Some(Head {
                id: Some(json!(7)),
                method: true
            })
        );
        let notice = read(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        assert_eq!(
            notice,
            Some(Head {
                id: None,
                method: true
            })
        );
        let response = read(br#"{"jsonrpc":"2.0","id":"a","result":{}}"#);
        assert_eq!(
            response,
            Some(Head {
                id: Some(json!("a")),
                method: false
            })
        );
    }

    #[test]
    fn a_null_id_is_present_and_a_repeated_key_takes_its_last_value() {
        assert_eq!(
            read(br#"{"id":null,"method":"ping"}"#),
            Some(Head {
                id: Some(json!(null)),
                method: true
            })
        );
        assert_eq!(
            read(br#"{"id":1,"id":2,"method":"ping"}"#),
            Some(Head {
                id: Some(json!(2)),
                method: true
            })
        );
    }

    #[test]
    fn a_batch_or_a_line_that_is_not_json_has_no_head() {
        assert_eq!(read(br#"[{"id":1,"method":"ping"}]"#), None);
        assert_eq!(read(b"nope"), None);
        assert_eq!(read(b""), None);
    }
}
