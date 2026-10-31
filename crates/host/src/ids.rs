//! Id minting: every id the host makes is a v7 UUID from the host's wall time. Tests swap in
//! `SequentialIds` (feature `test-support`) for ids that repeat run to run.

use clauseo_protocol::{
    CheckpointId, ElicitationId, EventId, FileRefId, FolderRefId, MatterId, MessageId,
    PermissionRequestId, ThreadId, Timestamp, ToolCallId, TurnId, ViewEpoch,
};
use uuid::Uuid;

use crate::error::Error;

/// Makes the UUIDs behind the host's ids.
pub trait IdSource: Send {
    /// A new v7 UUID whose time is `wall`; never one this source returned before.
    fn mint(&mut self, wall: Timestamp) -> Uuid;
}

/// Random v7 UUIDs at the host's wall time, kept in order within this source.
#[derive(Debug)]
pub struct RandomV7 {
    context: uuid::ContextV7,
}

impl RandomV7 {
    /// A new source.
    pub fn new() -> Self {
        Self {
            context: uuid::ContextV7::new(),
        }
    }
}

impl Default for RandomV7 {
    fn default() -> Self {
        Self::new()
    }
}

/// Milliseconds since the epoch as a `u64`; a time before 1970 counts as the epoch.
fn unix_millis(wall: Timestamp) -> u64 {
    u64::try_from(wall.as_millis()).unwrap_or(0)
}

impl IdSource for RandomV7 {
    fn mint(&mut self, wall: Timestamp) -> Uuid {
        let millis = unix_millis(wall);
        let nanos = u32::try_from((millis % 1000) * 1_000_000).unwrap_or(0);
        let ts = uuid::Timestamp::from_unix(&self.context, millis / 1000, nanos);
        Uuid::new_v7(ts)
    }
}

/// Deterministic v7 UUIDs: the milliseconds of `wall`, then a run number and a counter where
/// the random bits go. Two sources with different runs never make the same id.
#[cfg(feature = "test-support")]
#[derive(Debug, Default)]
pub struct SequentialIds {
    run: u16,
    counter: u64,
}

#[cfg(feature = "test-support")]
impl SequentialIds {
    /// Run 0, counting from 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `run`, for a test that starts the host again: its ids never repeat another run's.
    pub fn with_run(run: u16) -> Self {
        Self { run, counter: 0 }
    }
}

#[cfg(feature = "test-support")]
impl IdSource for SequentialIds {
    fn mint(&mut self, wall: Timestamp) -> Uuid {
        // The 10 bytes after the time: 12 bits of `rand_a`, then 62 of `rand_b`, after the
        // builder writes the version and variant bits. `rand_a` holds the counter's bits 44..56,
        // `rand_b` the run (16 bits) above the counter's low 44 bits.
        const LOW: u64 = (1 << 44) - 1;
        let counter = self.counter;
        self.counter = self.counter.wrapping_add(1);
        let rand_a = u16::try_from((counter >> 44) & 0xfff).unwrap_or(0);
        let rand_b = (u64::from(self.run) << 44) | (counter & LOW);
        let mut bytes = [0_u8; 10];
        for (slot, byte) in bytes
            .iter_mut()
            .zip(rand_a.to_be_bytes().into_iter().chain(rand_b.to_be_bytes()))
        {
            *slot = byte;
        }
        uuid::Builder::from_unix_timestamp_millis(unix_millis(wall), &bytes).into_uuid()
    }
}

/// An id type the host mints from a UUID.
pub(crate) trait MintId: Sized {
    /// The id, or the type's refusal of `u`.
    fn from_uuid(u: Uuid) -> Result<Self, clauseo_protocol::Error>;
}

macro_rules! mint_id {
    ($($t:ty),+ $(,)?) => {
        $(impl MintId for $t {
            fn from_uuid(u: Uuid) -> Result<Self, clauseo_protocol::Error> {
                <$t>::new(u)
            }
        })+
    };
}

mint_id!(
    EventId,
    MatterId,
    ThreadId,
    TurnId,
    MessageId,
    ToolCallId,
    PermissionRequestId,
    ElicitationId,
    CheckpointId,
    FileRefId,
    FolderRefId,
    ViewEpoch,
);

/// The host's id source, typed.
pub(crate) struct Ids(Box<dyn IdSource>);

impl Ids {
    /// Wraps `source`.
    pub(crate) fn new(source: Box<dyn IdSource>) -> Self {
        Self(source)
    }

    /// A new id of type `T` at `wall`. A source that makes a UUID `T` refuses is a host bug.
    pub(crate) fn mint<T: MintId>(&mut self, wall: Timestamp) -> Result<T, Error> {
        T::from_uuid(self.0.mint(wall))
            .map_err(|error| Error::Internal(format!("the id source made a bad id: {error}")))
    }
}

#[cfg(test)]
mod tests;
