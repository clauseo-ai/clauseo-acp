//! The stored forms: the `events` key, the `MessagePack` envelope, and the text of ids and rows.

use clauseo_protocol::{
    Envelope, EnvelopeRead, MAX_MSGPACK_DEPTH, MatterKind, SCHEMA_VERSION, Seq, StreamId,
};
use serde::Deserialize;

use crate::Error;

/// The largest `stream_id`: 2^31 - 1, so the key `(stream_id << 32) | seq` stays positive.
pub(crate) const MAX_STREAM_ID: i64 = (1 << 31) - 1;

/// The `events` key of `seq` in the stream numbered `stream_id`, or `KeyOutOfRange`.
pub(crate) fn key(stream: StreamId, stream_id: i64, seq: u64) -> Result<i64, Error> {
    let Ok(seq) = u32::try_from(seq) else {
        return Err(Error::KeyOutOfRange { stream });
    };
    if !(1..=MAX_STREAM_ID).contains(&stream_id) {
        return Err(Error::KeyOutOfRange { stream });
    }
    Ok((stream_id << 32) | i64::from(seq))
}

/// The first key of the stream numbered `stream_id` (seq 0, which no row has).
pub(crate) const fn first_key(stream_id: i64) -> i64 {
    stream_id << 32
}

/// The `seq` part of a key.
pub(crate) fn key_seq(key: i64) -> u64 {
    // Masked to 32 bits, so it is never negative.
    u64::try_from(key & 0xFFFF_FFFF).unwrap_or(0)
}

/// Appends `envelope` in `MessagePack` with named maps to `out`.
pub(crate) fn encode_envelope(envelope: &Envelope, out: &mut Vec<u8>) -> Result<(), Error> {
    rmp_serde::encode::write_named(out, envelope).map_err(|error| Error::Encode {
        detail: format!(
            "envelope {} of {}: {error}",
            envelope.seq.get(),
            envelope.stream
        ),
    })
}

/// Decodes one stored envelope in one pass from the row's bytes, and checks it names the place
/// its key gives.
pub(crate) fn decode_envelope(bytes: &[u8], stream: StreamId, seq: u64) -> Result<Envelope, Error> {
    let mut deserializer = rmp_serde::Deserializer::from_read_ref(bytes);
    deserializer.set_max_depth(MAX_MSGPACK_DEPTH);
    let read = EnvelopeRead::deserialize(&mut deserializer)
        .map_err(|error| Error::corrupt(format!("{stream} seq {seq} does not decode: {error}")))?;
    match read {
        EnvelopeRead::Current(envelope) => {
            if envelope.stream != stream || envelope.seq.get() != seq {
                return Err(Error::corrupt(format!(
                    "the row of {stream} seq {seq} holds {} seq {}",
                    envelope.stream,
                    envelope.seq.get()
                )));
            }
            Ok(envelope)
        }
        EnvelopeRead::Newer { schema_version } => Err(Error::NewerSchema {
            found: schema_version,
            supported: SCHEMA_VERSION,
        }),
    }
}

/// A `Seq` read back from a row.
pub(crate) fn stored_seq(value: u64, what: &str) -> Result<Seq, Error> {
    Seq::new(value).map_err(|error| Error::corrupt(format!("{what}: {error}")))
}

/// Parses text read back from a row: a value that does not parse is corrupt.
pub(crate) fn parse<T>(text: &str, what: &str) -> Result<T, Error>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    text.parse()
        .map_err(|error| Error::corrupt(format!("{what} {text:?} does not parse: {error}")))
}

/// The `matters.kind` text.
pub(crate) fn matter_kind_text(kind: MatterKind) -> Result<&'static str, Error> {
    match kind {
        MatterKind::Scratch => Ok("scratch"),
        MatterKind::Folder => Ok("folder"),
        MatterKind::Unknown => Err(Error::Encode {
            detail: "a matter of unknown kind has no stored form".to_owned(),
        }),
    }
}

/// A `matters.kind` read back: a kind this build does not know reads as `Unknown`.
pub(crate) fn matter_kind(text: &str) -> MatterKind {
    match text {
        "scratch" => MatterKind::Scratch,
        "folder" => MatterKind::Folder,
        _ => MatterKind::Unknown,
    }
}
