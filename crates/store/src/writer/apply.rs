//! What one commit writes, inside the transaction (or savepoint) the writer opened for it.

use std::collections::HashMap;

use clauseo_protocol::{AccountLimits, AgentKind, AgentSessionId, Seq, StreamId};
use rusqlite::{Connection, params};

use super::StreamState;
use crate::codec::{self, MAX_STREAM_ID};
use crate::{Commit, Committed, Error, Receipt, SnapshotIn, read};

/// The streams one commit touches, over the writer's map and its group's staged changes.
pub(super) struct Changes<'a> {
    base: &'a HashMap<StreamId, StreamState>,
    staged: &'a HashMap<StreamId, StreamState>,
    /// This commit's own changes; a commit names few streams, so a list beats a map.
    local: Vec<(StreamId, StreamState)>,
    last_id: i64,
}

impl<'a> Changes<'a> {
    pub(super) fn new(
        base: &'a HashMap<StreamId, StreamState>,
        staged: &'a HashMap<StreamId, StreamState>,
        last_id: i64,
    ) -> Self {
        Self {
            base,
            staged,
            local: Vec::new(),
            last_id,
        }
    }

    /// The streams this commit changed and the highest `stream_id` given.
    pub(super) fn into_parts(self) -> (Vec<(StreamId, StreamState)>, i64) {
        (self.local, self.last_id)
    }

    fn get(&self, stream: StreamId) -> Option<StreamState> {
        self.local
            .iter()
            .find(|(id, _)| *id == stream)
            .map(|(_, state)| *state)
            .or_else(|| self.staged.get(&stream).copied())
            .or_else(|| self.base.get(&stream).copied())
    }

    fn set(&mut self, stream: StreamId, state: StreamState) {
        match self.local.iter_mut().find(|(id, _)| *id == stream) {
            Some(entry) => entry.1 = state,
            None => self.local.push((stream, state)),
        }
    }

    /// The stream's state, giving it the next `stream_id` (a row with `last_seq` 0) when it
    /// has none.
    fn get_or_add(&mut self, conn: &Connection, stream: StreamId) -> Result<StreamState, Error> {
        if let Some(state) = self.get(stream) {
            return Ok(state);
        }
        let id = self.last_id + 1;
        if id > MAX_STREAM_ID {
            return Err(Error::KeyOutOfRange { stream });
        }
        conn.prepare_cached(
            "INSERT INTO streams (stream_id, stream, last_seq) VALUES (?1, ?2, 0)",
        )?
        .execute(params![id, stream.to_string()])?;
        self.last_id = id;
        let state = StreamState { id, last_seq: 0 };
        self.set(stream, state);
        Ok(state)
    }
}

/// Writes all of `commit`, or returns the error that stopped it (the caller rolls back).
pub(super) fn apply(
    conn: &Connection,
    buf: &mut Vec<u8>,
    commit: &Commit,
    changes: &mut Changes<'_>,
) -> Result<Committed, Error> {
    if let Some(receipt) = &commit.receipt {
        put_receipt(conn, receipt, changes)?;
    }
    let heads = put_events(conn, buf, commit, changes)?;
    put_rows(conn, commit)?;
    if let Some(snapshot) = &commit.snapshot {
        put_snapshot(conn, snapshot, changes)?;
    }
    Ok(Committed { heads })
}

/// Appends the commit's events after each stream's last `seq`, and moves the heads.
fn put_events(
    conn: &Connection,
    buf: &mut Vec<u8>,
    commit: &Commit,
    changes: &mut Changes<'_>,
) -> Result<Vec<(StreamId, Seq)>, Error> {
    let mut heads: Vec<(StreamId, Seq)> = Vec::new();
    let mut moved: Vec<(StreamId, StreamState)> = Vec::new();
    {
        let mut insert = conn.prepare_cached(
            "INSERT INTO events (key, event_id, schema_version, kind, envelope) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        // The event id's text, written on the stack: about 12 ns an event less than
        // `to_string` (measured on an Apple M6; see the PR).
        let mut id_text = [0_u8; 36];
        for envelope in &commit.events {
            let stream = envelope.stream;
            let mut state = match moved.iter().find(|(id, _)| *id == stream) {
                Some((_, state)) => *state,
                None => changes.get_or_add(conn, stream)?,
            };
            let expected = state.last_seq + 1;
            if envelope.seq.get() != expected {
                return Err(match Seq::new(expected) {
                    Ok(expected) => Error::SeqGap {
                        stream,
                        expected,
                        got: envelope.seq,
                    },
                    Err(_) => Error::KeyOutOfRange { stream },
                });
            }
            let key = codec::key(stream, state.id, expected)?;
            buf.clear();
            codec::encode_envelope(envelope, buf)?;
            insert.execute(params![
                key,
                &*envelope
                    .event_id
                    .as_uuid()
                    .hyphenated()
                    .encode_lower(&mut id_text),
                envelope.schema_version(),
                envelope.event.kind(),
                buf.as_slice(),
            ])?;
            state.last_seq = expected;
            match moved.iter_mut().find(|(id, _)| *id == stream) {
                Some(entry) => entry.1 = state,
                None => moved.push((stream, state)),
            }
        }
    }
    if !moved.is_empty() {
        let mut update =
            conn.prepare_cached("UPDATE streams SET last_seq = ?1 WHERE stream_id = ?2")?;
        for (stream, state) in moved {
            // Below 2^32: the key took it.
            let last_seq = i64::try_from(state.last_seq).unwrap_or(i64::MAX);
            update.execute(params![last_seq, state.id])?;
            changes.set(stream, state);
            if let Ok(head) = Seq::new(state.last_seq) {
                heads.push((stream, head));
            }
        }
    }
    Ok(heads)
}

/// Writes the commit's blobs, matter, thread and checkpoint rows.
fn put_rows(conn: &Connection, commit: &Commit) -> Result<(), Error> {
    if !commit.blobs.is_empty() {
        let mut insert = conn.prepare_cached(
            "INSERT OR IGNORE INTO blobs (hash, size_bytes, data) VALUES (?1, ?2, ?3)",
        )?;
        for blob in &commit.blobs {
            let size = i64::try_from(blob.bytes().len()).unwrap_or(i64::MAX);
            insert.execute(params![
                blob.hash().as_bytes().as_slice(),
                size,
                blob.bytes()
            ])?;
        }
    }
    if let Some(matter) = &commit.matter {
        conn.prepare_cached(
            "INSERT INTO matters (id, name, kind, folder_path, folder_locator, shadow_repo_path, \
             created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT (id) DO UPDATE SET \
             name = excluded.name, kind = excluded.kind, folder_path = excluded.folder_path, \
             folder_locator = excluded.folder_locator, \
             shadow_repo_path = excluded.shadow_repo_path, created_ms = excluded.created_ms",
        )?
        .execute(params![
            matter.id.to_string(),
            matter.name,
            codec::matter_kind_text(matter.kind)?,
            matter.folder_path,
            matter.folder_locator,
            matter.shadow_repo_path,
            matter.created.as_millis(),
        ])?;
    }
    if let Some(thread) = &commit.thread {
        conn.prepare_cached(
            "INSERT INTO threads (id, matter_id, agent_id, title, acp_session_id, archived, \
             created_ms, updated_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT (id) DO UPDATE SET matter_id = excluded.matter_id, \
             agent_id = excluded.agent_id, title = excluded.title, \
             acp_session_id = excluded.acp_session_id, archived = excluded.archived, \
             created_ms = excluded.created_ms, updated_ms = excluded.updated_ms",
        )?
        .execute(params![
            thread.id.to_string(),
            thread.matter.to_string(),
            thread.agent.as_str(),
            thread.title,
            thread.acp_session_id.as_ref().map(AgentSessionId::as_str),
            thread.archived,
            thread.created.as_millis(),
            thread.updated.as_millis(),
        ])?;
    }
    if !commit.checkpoints.is_empty() {
        let mut insert = conn.prepare_cached(
            "INSERT INTO checkpoints (id, matter_id, commit_oid, seq) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for checkpoint in &commit.checkpoints {
            insert.execute(params![
                checkpoint.id.to_string(),
                checkpoint.matter.to_string(),
                checkpoint.commit_oid,
                i64::try_from(checkpoint.seq.get()).unwrap_or(i64::MAX),
            ])?;
        }
    }
    Ok(())
}

/// Stores the snapshot and keeps only the two newest of its stream.
fn put_snapshot(
    conn: &Connection,
    snapshot: &SnapshotIn,
    changes: &mut Changes<'_>,
) -> Result<(), Error> {
    {
        let state = changes.get_or_add(conn, snapshot.stream)?;
        conn.prepare_cached(
            "INSERT OR REPLACE INTO snapshots (stream_id, seq, reducer_schema, state) \
             VALUES (?1, ?2, ?3, ?4)",
        )?
        .execute(params![
            state.id,
            i64::try_from(snapshot.seq.get()).unwrap_or(i64::MAX),
            snapshot.reducer_schema,
            snapshot.compressed.as_slice(),
        ])?;
        // Keep the two newest, so one bad snapshot has a fallback.
        conn.prepare_cached(
            "DELETE FROM snapshots WHERE stream_id = ?1 AND seq < (SELECT seq FROM snapshots \
             WHERE stream_id = ?1 ORDER BY seq DESC LIMIT 1 OFFSET 1)",
        )?
        .execute([state.id])?;
    }
    Ok(())
}

/// Inserts the receipt. An existing one that decodes is `DuplicateCommand`; one that does not
/// counts as missing and is replaced, which allows one re-run per `command_id`.
fn put_receipt(
    conn: &Connection,
    receipt: &Receipt,
    changes: &mut Changes<'_>,
) -> Result<(), Error> {
    let stream_id = match receipt.stream {
        Some(stream) => Some(changes.get_or_add(conn, stream)?.id),
        None => None,
    };
    let result = serde_json::to_string(&receipt.result).map_err(|error| Error::Encode {
        detail: format!("the receipt of {}: {error}", receipt.command_id),
    })?;
    let command_id = receipt.command_id.to_string();
    let inserted = conn
        .prepare_cached(
            "INSERT INTO receipts (command_id, stream_id, payload_hash, result, ts_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT (command_id) DO NOTHING",
        )?
        .execute(params![
            command_id,
            stream_id,
            receipt.payload_hash.as_slice(),
            result,
            receipt.ts.as_millis(),
        ])?;
    if inserted == 1 {
        return Ok(());
    }
    if let Some(stored) = read::receipt(conn, receipt.command_id)? {
        return Err(Error::DuplicateCommand {
            stored: Box::new(stored),
        });
    }
    conn.prepare_cached(
        "UPDATE receipts SET stream_id = ?2, payload_hash = ?3, result = ?4, ts_ms = ?5 \
         WHERE command_id = ?1",
    )?
    .execute(params![
        command_id,
        stream_id,
        receipt.payload_hash.as_slice(),
        result,
        receipt.ts.as_millis(),
    ])?;
    Ok(())
}

/// Keeps `limits` as the last value for its agent and account.
pub(super) fn put_account_limits(conn: &Connection, limits: &AccountLimits) -> Result<(), Error> {
    let json = serde_json::to_string(limits).map_err(|error| Error::Encode {
        detail: format!("account limits: {error}"),
    })?;
    conn.prepare_cached(
        "INSERT INTO account_limits (agent, account, limits) VALUES (?1, ?2, ?3) \
         ON CONFLICT (agent, account) DO UPDATE SET limits = excluded.limits",
    )?
    .execute(params![agent_text(limits.agent), limits.account.0, json])?;
    Ok(())
}

/// The `account_limits.agent` text, the agent's serde name.
const fn agent_text(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
    }
}
