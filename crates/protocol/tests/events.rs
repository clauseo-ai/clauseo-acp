//! Events: ids on the wire, and enums that decode values a newer Clauseo may add.

#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;

use clauseo_protocol::StreamId;

use fixtures::{matter_id, thread_id, v7_text};

#[test]
fn stream_id_is_a_plain_string() {
    let thread = StreamId::Thread(thread_id(1));
    let matter = StreamId::Matter(matter_id(2));
    let cases = [
        (thread, format!("thread:{}", v7_text(1))),
        (matter, format!("matter:{}", v7_text(2))),
    ];
    for (stream, text) in cases {
        assert_eq!(stream.to_string(), text);
        assert_eq!(text.parse::<StreamId>().unwrap(), stream);
        let json = serde_json::to_string(&stream).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<StreamId>(&json).unwrap(), stream);
        // MessagePack carries the same text, as a str, not a map or a byte array.
        let packed = rmp_serde::to_vec_named(&stream).unwrap();
        assert_eq!(packed, rmp_serde::to_vec_named(&text).unwrap());
        assert_eq!(rmp_serde::from_slice::<StreamId>(&packed).unwrap(), stream);
    }
    let nil_v4 = "matter:00000000-0000-4000-8000-000000000000";
    for bad in ["thread", "topic:x", "thread:not-a-uuid", nil_v4] {
        assert!(bad.parse::<StreamId>().is_err(), "{bad} parsed");
        let json = format!("\"{bad}\"");
        assert!(
            serde_json::from_str::<StreamId>(&json).is_err(),
            "{bad} decoded"
        );
    }
}

mod unknown_variants {
    use clauseo_protocol::{ErrorCode, HostError};

    #[test]
    fn error_code() {
        let disk_full = serde_json::from_str::<ErrorCode>(r#""store.disk_full""#).unwrap();
        assert_eq!(disk_full, ErrorCode::StoreDiskFull);
        let later = serde_json::from_str::<ErrorCode>(r#""store.on_fire""#).unwrap();
        assert_eq!(later, ErrorCode::Unknown);
        assert_eq!(ErrorCode::Unknown.as_str(), "unknown");
        assert_eq!(
            serde_json::to_string(&ErrorCode::Unknown).unwrap(),
            r#""unknown""#
        );
        let packed = rmp_serde::to_vec_named("store.on_fire").unwrap();
        assert_eq!(
            rmp_serde::from_slice::<ErrorCode>(&packed).unwrap(),
            ErrorCode::Unknown
        );

        // HostError keeps the unknown code in its message.
        let json = r#"{"code":"store.on_fire","message":"smoke","retryable":true}"#;
        let error: HostError = serde_json::from_str(json).unwrap();
        let message = "store.on_fire: smoke".to_owned();
        assert_eq!(
            error,
            HostError {
                code: ErrorCode::Unknown,
                message,
                retryable: true
            }
        );
        let packed =
            rmp_serde::to_vec_named(&serde_json::from_str::<HostErrorShape>(json).unwrap());
        let error: HostError = rmp_serde::from_slice(&packed.unwrap()).unwrap();
        assert_eq!(error.message, "store.on_fire: smoke");

        let known = r#"{"code":"thread.queue_full","message":"ten queued","retryable":true}"#;
        let error: HostError = serde_json::from_str(known).unwrap();
        assert_eq!(
            error,
            HostError::new(ErrorCode::ThreadQueueFull, "ten queued")
        );
        assert_eq!(serde_json::to_string(&error).unwrap(), known);
    }

    /// A `HostError` with any code string, to build `MessagePack` input.
    #[derive(serde::Serialize, serde::Deserialize)]
    struct HostErrorShape {
        code: String,
        message: String,
        retryable: bool,
    }
}
