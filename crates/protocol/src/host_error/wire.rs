//! `HostError` read from the wire: its code looked up, a code this version does not know kept in
//! the message as `unknown`.

use serde::{Deserialize, Deserializer};

use super::{FileRefusal, HostError, WriteFailure};
use crate::serde_util::BorrowedStr;
use crate::{ErrorCode, MatterId, MatterPath};

/// `HostError` as read, before its code is looked up.
#[derive(Deserialize)]
struct HostErrorWire<'a> {
    #[serde(borrow)]
    code: BorrowedStr<'a>,
    message: String,
    retryable: bool,
    #[serde(default)]
    matter: Option<MatterId>,
    #[serde(default)]
    files: Vec<MatterPath>,
    #[serde(default)]
    refusal: Option<FileRefusal>,
    #[serde(default)]
    write_failure: Option<WriteFailure>,
}

impl<'de> Deserialize<'de> for HostError {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = HostErrorWire::deserialize(deserializer)?;
        let raw = wire.code.as_str();
        Ok(match ErrorCode::lookup(raw) {
            // `lookup` finds `Unknown` for the string "unknown", which has no `retryable` of its
            // own: the peer's value stays, as it does for a code this version does not know.
            Some(code) => Self {
                code,
                message: wire.message,
                retryable: if code == ErrorCode::Unknown {
                    wire.retryable
                } else {
                    code.is_retryable()
                },
                matter: wire.matter,
                files: wire.files,
                refusal: wire.refusal,
                write_failure: wire.write_failure,
            },
            None => Self {
                code: ErrorCode::Unknown,
                message: format!("{raw}: {}", wire.message),
                retryable: wire.retryable,
                matter: wire.matter,
                files: wire.files,
                refusal: wire.refusal,
                write_failure: wire.write_failure,
            },
        })
    }
}
