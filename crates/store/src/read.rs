//! The reader thread's queries, on a second connection with `query_only`, so a long replay
//! never delays a commit. `receipt` also runs on the writer, for a duplicate `command_id`.

use std::collections::HashMap;

use clauseo_protocol::{
    AccountLimits, CheckpointId, CommandId, CommandOutcome, Envelope, MatterId, Seq, StreamId,
    Timestamp,
};
use rusqlite::{Connection, OptionalExtension, Row};

use crate::codec::{self, first_key, key_seq};
use crate::{CheckpointRow, Error, MatterRow, Receipt, Reopen, Snapshot, ThreadRow};

/// The most envelopes one `read_after` returns.
pub const READ_PAGE_MAX: u32 = 500;

/// The reader's memory of `stream_id`s. A stream's number never changes once given, so the
/// cache only grows.
#[derive(Default)]
pub(crate) struct StreamIds(HashMap<StreamId, i64>);

impl StreamIds {
    /// The stream's number and last `seq`, or `None` if no row names it yet.
    fn lookup(&mut self, conn: &Connection, stream: StreamId) -> Result<Option<(i64, u64)>, Error> {
        let found = conn
            .prepare_cached("SELECT stream_id, last_seq FROM streams WHERE stream = ?1")?
            .query_row([stream.to_string()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .optional()?;
        let Some((id, last_seq)) = found else {
            return Ok(None);
        };
        let last_seq = u64::try_from(last_seq).map_err(|error| {
            Error::corrupt(format!("{stream} has last_seq {last_seq}: {error}"))
        })?;
        self.0.insert(stream, id);
        Ok(Some((id, last_seq)))
    }

    /// The stream's number alone, from the cache when it is there.
    fn id(&mut self, conn: &Connection, stream: StreamId) -> Result<Option<i64>, Error> {
        if let Some(id) = self.0.get(&stream) {
            return Ok(Some(*id));
        }
        Ok(self.lookup(conn, stream)?.map(|(id, _)| id))
    }
}

/// Up to `limit` (at most [`READ_PAGE_MAX`]) envelopes of `stream` after `after`, by index
/// seek on the key.
pub(crate) fn read_after(
    conn: &Connection,
    ids: &mut StreamIds,
    stream: StreamId,
    after: Option<Seq>,
    limit: u32,
) -> Result<Vec<Envelope>, Error> {
    let Some(id) = ids.id(conn, stream)? else {
        return Ok(Vec::new());
    };
    let after = after.map_or(0, Seq::get);
    // No event has a seq past u32::MAX (the key refuses it), so nothing comes after one.
    if after > u64::from(u32::MAX) {
        return Ok(Vec::new());
    }
    let limit = limit.min(READ_PAGE_MAX);
    let low = codec::key(stream, id, after)?;
    let high = first_key(id) | 0xFFFF_FFFF;
    rows(conn, stream, after, low, high, Some(limit))
}

/// The newest snapshot of `stream` with `reducer_schema` that decompresses, and every event
/// after it.
pub(crate) fn reopen(
    conn: &Connection,
    ids: &mut StreamIds,
    stream: StreamId,
    reducer_schema: u32,
) -> Result<Reopen, Error> {
    // One read transaction, so the snapshot, the tail and the head agree.
    conn.prepare_cached("BEGIN")?.execute([])?;
    let result = reopen_in(conn, ids, stream, reducer_schema);
    conn.prepare_cached("COMMIT")?.execute([])?;
    result
}

fn reopen_in(
    conn: &Connection,
    ids: &mut StreamIds,
    stream: StreamId,
    reducer_schema: u32,
) -> Result<Reopen, Error> {
    let Some((id, last_seq)) = ids.lookup(conn, stream)? else {
        return Ok(Reopen {
            snapshot: None,
            tail: Vec::new(),
            head: None,
        });
    };
    let snapshot = newest_snapshot(conn, id, reducer_schema, last_seq)?;
    let after = snapshot.as_ref().map_or(0, |snapshot| snapshot.seq.get());
    let low = codec::key(stream, id, after)?;
    let high = first_key(id) | i64::try_from(last_seq).unwrap_or(0);
    let tail = rows(conn, stream, after, low, high, None)?;
    let read_to = tail.last().map_or(after, |envelope| envelope.seq.get());
    if read_to != last_seq {
        return Err(Error::corrupt(format!(
            "{stream} ends at seq {read_to}, its head is {last_seq}"
        )));
    }
    Ok(Reopen {
        snapshot,
        tail,
        head: Seq::new(last_seq).ok(),
    })
}

/// The newest snapshot with `reducer_schema` at or below the head that decompresses. One that
/// does not is skipped, as the reducer skips one it cannot decode: the older one, or the log,
/// takes its place.
fn newest_snapshot(
    conn: &Connection,
    id: i64,
    reducer_schema: u32,
    last_seq: u64,
) -> Result<Option<Snapshot>, Error> {
    let mut statement = conn.prepare_cached(
        "SELECT seq, state FROM snapshots WHERE stream_id = ?1 AND reducer_schema = ?2 \
         AND seq <= ?3 ORDER BY seq DESC",
    )?;
    let mut rows = statement.query(rusqlite::params![
        id,
        reducer_schema,
        i64::try_from(last_seq).unwrap_or(i64::MAX)
    ])?;
    while let Some(row) = rows.next()? {
        let seq = u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0);
        let Ok(seq) = Seq::new(seq) else { continue };
        if let Some(state) = decompress(row.get_ref(1)?.as_blob()?) {
            return Ok(Some(Snapshot {
                seq,
                reducer_schema,
                state,
            }));
        }
    }
    Ok(None)
}

/// The most bytes a snapshot may claim to decompress to: 1 GiB, far above a long thread's.
const SNAPSHOT_MAX_BYTES: u64 = 1 << 30;

/// The decompressed snapshot, or `None` if its frame is bad or claims more than the cap.
fn decompress(compressed: &[u8]) -> Option<Vec<u8>> {
    let size = zstd::zstd_safe::get_frame_content_size(compressed).ok()??;
    if size > SNAPSHOT_MAX_BYTES {
        return None;
    }
    let state = zstd::bulk::decompress(compressed, usize::try_from(size).ok()?).ok()?;
    (state.len() as u64 == size).then_some(state)
}

/// The envelopes with keys in `(low, high]`, which must run from `after + 1` without a gap.
fn rows(
    conn: &Connection,
    stream: StreamId,
    after: u64,
    low: i64,
    high: i64,
    limit: Option<u32>,
) -> Result<Vec<Envelope>, Error> {
    let mut statement = conn.prepare_cached(
        "SELECT key, envelope FROM events WHERE key > ?1 AND key <= ?2 ORDER BY key LIMIT ?3",
    )?;
    let limit = limit.map_or(-1, i64::from);
    let capacity = if limit < 0 {
        usize::try_from(high - low).unwrap_or(0)
    } else {
        usize::try_from(limit).unwrap_or(0)
    };
    let mut out = Vec::with_capacity(capacity.min(1 << 16));
    let mut rows = statement.query(rusqlite::params![low, high, limit])?;
    let mut expected = after + 1;
    while let Some(row) = rows.next()? {
        let seq = key_seq(row.get(0)?);
        if seq != expected {
            return Err(Error::corrupt(format!(
                "{stream} has a gap: seq {seq} where {expected} was due"
            )));
        }
        out.push(codec::decode_envelope(
            row.get_ref(1)?.as_blob()?,
            stream,
            seq,
        )?);
        expected += 1;
    }
    Ok(out)
}

/// The receipt of `command`, or `None` when there is none or it does not decode.
pub(crate) fn receipt(conn: &Connection, command: CommandId) -> Result<Option<Receipt>, Error> {
    let found = conn
        .prepare_cached(
            "SELECT s.stream, r.payload_hash, r.result, r.ts_ms FROM receipts r \
             LEFT JOIN streams s ON s.stream_id = r.stream_id WHERE r.command_id = ?1",
        )?
        .query_row([command.to_string()], |row| {
            let stream: Option<String> = row.get(0)?;
            let hash: Vec<u8> = row.get(1)?;
            let result: String = row.get(2)?;
            let ts: i64 = row.get(3)?;
            Ok((stream, hash, result, ts))
        })
        .optional()?;
    let Some((stream, hash, result, ts)) = found else {
        return Ok(None);
    };
    // A receipt is not part of the log: one that does not read back counts as missing.
    let Ok(stream) = stream.map(|text| text.parse::<StreamId>()).transpose() else {
        return Ok(None);
    };
    let (Ok(payload_hash), Ok(result), Ok(ts)) = (
        <[u8; 32]>::try_from(hash.as_slice()),
        serde_json::from_str::<CommandOutcome>(&result),
        Timestamp::new(ts),
    ) else {
        return Ok(None);
    };
    Ok(Some(Receipt {
        command_id: command,
        stream,
        payload_hash,
        result,
        ts,
    }))
}

/// Every matter, oldest first.
pub(crate) fn matters(conn: &Connection) -> Result<Vec<MatterRow>, Error> {
    let mut statement = conn.prepare_cached(
        "SELECT id, name, kind, folder_path, folder_locator, shadow_repo_path, created_ms \
         FROM matters ORDER BY created_ms, id",
    )?;
    let mut rows = statement.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(MatterRow {
            id: codec::parse(row.get_ref(0)?.as_str()?, "a matter id")?,
            name: row.get(1)?,
            kind: codec::matter_kind(row.get_ref(2)?.as_str()?),
            folder_path: row.get(3)?,
            folder_locator: row.get(4)?,
            shadow_repo_path: row.get(5)?,
            created: timestamp(row, 6)?,
        });
    }
    Ok(out)
}

/// The threads of `matter`, most recently updated first.
pub(crate) fn threads(conn: &Connection, matter: MatterId) -> Result<Vec<ThreadRow>, Error> {
    let mut statement = conn.prepare_cached(
        "SELECT id, matter_id, agent_id, title, acp_session_id, archived, created_ms, \
         updated_ms FROM threads WHERE matter_id = ?1 ORDER BY updated_ms DESC, id",
    )?;
    let mut rows = statement.query([matter.to_string()])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let session: Option<String> = row.get(4)?;
        out.push(ThreadRow {
            id: codec::parse(row.get_ref(0)?.as_str()?, "a thread id")?,
            matter: codec::parse(row.get_ref(1)?.as_str()?, "a matter id")?,
            agent: codec::parse(row.get_ref(2)?.as_str()?, "an agent id")?,
            title: row.get(3)?,
            acp_session_id: session
                .map(|text| codec::parse(&text, "an agent session id"))
                .transpose()?,
            archived: row.get(5)?,
            created: timestamp(row, 6)?,
            updated: timestamp(row, 7)?,
        });
    }
    Ok(out)
}

/// The checkpoint `id`.
pub(crate) fn checkpoint(
    conn: &Connection,
    id: CheckpointId,
) -> Result<Option<CheckpointRow>, Error> {
    let found = conn
        .prepare_cached("SELECT matter_id, commit_oid, seq FROM checkpoints WHERE id = ?1")?
        .query_row([id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .optional()?;
    let Some((matter, commit_oid, seq)) = found else {
        return Ok(None);
    };
    let seq = u64::try_from(seq).unwrap_or(0);
    Ok(Some(CheckpointRow {
        id,
        matter: codec::parse(&matter, "a matter id")?,
        commit_oid,
        seq: codec::stored_seq(seq, "a checkpoint's seq")?,
    }))
}

/// The last limits of every account. A value that no longer decodes is skipped: the cache is
/// disposable, and the host reads the limits again.
pub(crate) fn account_limits(conn: &Connection) -> Result<Vec<AccountLimits>, Error> {
    let mut statement =
        conn.prepare_cached("SELECT limits FROM account_limits ORDER BY agent, account")?;
    let mut rows = statement.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        if let Ok(limits) = serde_json::from_slice(row.get_ref(0)?.as_bytes()?) {
            out.push(limits);
        }
    }
    Ok(out)
}

fn timestamp(row: &Row<'_>, column: usize) -> Result<Timestamp, Error> {
    let millis: i64 = row.get(column)?;
    Timestamp::new(millis).map_err(|error| Error::corrupt(format!("a time {millis}: {error}")))
}
