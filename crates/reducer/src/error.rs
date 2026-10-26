//! The crate's one error type. None of it reaches a client: `host` maps a failed `apply` to
//! `internal`, and a snapshot that does not decode or has another schema to a replay from events.

use clauseo_protocol::{Seq, StreamId};

/// Why the reducer refused a call. A refused call leaves the state as it was.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The envelope belongs to another stream than the state's.
    #[error("the envelope's stream {got} is not this state's stream {expected}")]
    WrongStream {
        /// The state's stream.
        expected: StreamId,
        /// The envelope's stream.
        got: StreamId,
    },
    /// The envelope's `seq` is not the state's `seq + 1` (1 for a new state): a gap or a repeat,
    /// which is a bug in the single writer.
    #[error("seq {got} is out of order: the state expects {expected}")]
    SeqOutOfOrder {
        /// The `seq` the state takes next.
        expected: u64,
        /// The envelope's `seq`.
        got: u64,
    },
    /// A text delta's `pending_seq` is behind the state's `seq`. The host's pending head runs
    /// ahead of the state, never behind it, so this is a bug in the host.
    #[error("the delta's pending seq {pending_seq} is behind the state's seq {seq}")]
    PendingSeqBehind {
        /// The state's `seq`.
        seq: Seq,
        /// The delta's `pending_seq`.
        pending_seq: Seq,
    },
    /// The snapshot was written by another `REDUCER_SCHEMA_VERSION`; rebuild it from events.
    #[error("the snapshot has reducer schema {found}; this build reads {expected}")]
    SchemaMismatch {
        /// The schema the snapshot was written with.
        found: u32,
        /// This build's `REDUCER_SCHEMA_VERSION`.
        expected: u32,
    },
    /// The snapshot bytes do not decode; rebuild the state from events.
    #[error("the snapshot bytes do not decode")]
    SnapshotDecode(#[source] rmp_serde::decode::Error),
    /// The state does not encode (a value one of `protocol`'s encoders refuses).
    #[error("the state does not encode as a snapshot")]
    SnapshotEncode(#[source] rmp_serde::encode::Error),
}
