//! `list_versions {before?, limit?}`: this matter's versions, newest first.
//!
//! The first job on the matter's thread picks the page from the timeline (the versions older
//! than `before`, an id this matter's timeline must hold, else the newest) and diffs the first
//! [`VERSIONS_PER_JOB`] of them; each further job diffs the next as many, so a checkpoint
//! queued meanwhile runs between them. A version's files are `Matter::changes_between` its
//! previous version and itself (the root import's: every file of its tree, added), the same
//! exact-content pairing the step's own log holds, excluded paths left out; at most
//! [`MAX_FILES_PER_VERSION`] names are kept. Then the matter's log gives each version's time and
//! a restore's source ([`super::VersionsSource::logged`]).

use clauseo_matters::{Matter, TimelineEntry};
use clauseo_protocol::{ChangeKind, CheckpointId, CheckpointKind, MatterPath};
use serde_json::{Map, Value, json};

use super::mcp::Outcome;
use super::texts::Refusal;
use super::{Backend, DEFAULT_LIMIT, MAX_FILES_PER_VERSION, MAX_LIMIT, VERSIONS_PER_JOB, utc_time};

/// The tool's name.
pub(super) const NAME: &str = "list_versions";

/// One version of the page, with the version before it.
struct Row {
    entry: TimelineEntry,
    previous: Option<CheckpointId>,
}

/// The files one version changed: the first [`MAX_FILES_PER_VERSION`], and how many in all.
struct Files {
    listed: Vec<(MatterPath, ChangeKind)>,
    total: usize,
}

/// The page the first job picked, with the first chunk's files.
struct Picked {
    rows: Vec<Row>,
    next_before: Option<CheckpointId>,
    files: Vec<Files>,
}

/// Runs `list_versions` with `args`.
pub(super) fn run(backend: &Backend, args: &Map<String, Value>) -> Outcome {
    let limit = limit(args.get("limit"))?;
    let before = before(args.get("before"))?;
    let Picked {
        rows,
        next_before,
        mut files,
    } = backend.on_matter(move |matter| {
        let (rows, next_before) = pick(matter.timeline(), before, limit)?;
        let first: Vec<_> = rows
            .iter()
            .take(VERSIONS_PER_JOB)
            .map(|row| (row.entry.id, row.previous))
            .collect();
        let files = files_of(matter, &first)?;
        Ok(Picked {
            rows,
            next_before,
            files,
        })
    })?;
    for chunk in rows.chunks(VERSIONS_PER_JOB).skip(1) {
        let pairs: Vec<_> = chunk
            .iter()
            .map(|row| (row.entry.id, row.previous))
            .collect();
        files.extend(backend.on_matter(move |matter| files_of(matter, &pairs))?);
    }
    let ids: Vec<CheckpointId> = rows.iter().map(|row| row.entry.id).collect();
    let session = backend.config.session;
    let logged = backend.config.source.logged(session.matter, &ids);
    let versions: Vec<Value> = rows
        .iter()
        .zip(files)
        .map(|(row, files)| {
            let entry = &row.entry;
            let mut version = json!({
                "id": entry.id.to_string(),
                "at": logged.at.get(&entry.id).map(|at| utc_time(*at)),
                "kind": kind_name(entry.kind),
                "by": by(entry.kind),
                "this_conversation": entry.thread == Some(session.thread),
                "files": files.listed.iter().map(file_json).collect::<Vec<_>>(),
                "files_total": files.total,
            });
            if let (Some(from), Some(object)) =
                (logged.restored_from.get(&entry.id), version.as_object_mut())
            {
                object.insert("restored_from".to_owned(), json!(from.to_string()));
            }
            version
        })
        .collect();
    let mut page = json!({ "versions": versions });
    if let (Some(next), Some(object)) = (next_before, page.as_object_mut()) {
        object.insert("next_before".to_owned(), json!(next.to_string()));
    }
    Ok(page)
}

/// `limit`: [`DEFAULT_LIMIT`] when missing or null, else a whole number from 1 to
/// [`MAX_LIMIT`].
fn limit(value: Option<&Value>) -> Result<usize, Refusal> {
    match value {
        None | Some(Value::Null) => Ok(DEFAULT_LIMIT),
        Some(value) => value
            .as_u64()
            .and_then(|limit| usize::try_from(limit).ok())
            .filter(|limit| (1..=MAX_LIMIT).contains(limit))
            .ok_or(Refusal::BadLimit),
    }
}

/// Where a page ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Before {
    /// At the newest version: `before` was not given.
    Newest,
    /// Just before this id, which the timeline must hold.
    Id(CheckpointId),
    /// Before a string that is no version id: no timeline holds it.
    NoId,
}

/// `before`: [`Before::Newest`] when missing or null.
fn before(value: Option<&Value>) -> Result<Before, Refusal> {
    match value {
        None | Some(Value::Null) => Ok(Before::Newest),
        Some(Value::String(text)) => Ok(text.parse().map_or(Before::NoId, Before::Id)),
        Some(_) => Err(Refusal::BadBefore),
    }
}

/// The page of `timeline` (oldest first): up to `limit` versions older than `before` (all when
/// none), newest first, and the oldest of them when older ones are left. A `before` the
/// timeline does not hold is [`Refusal::UnknownVersion`].
fn pick(
    timeline: &[TimelineEntry],
    before: Before,
    limit: usize,
) -> Result<(Vec<Row>, Option<CheckpointId>), Refusal> {
    let end = match before {
        Before::Newest => timeline.len(),
        Before::Id(id) => timeline
            .iter()
            .position(|entry| entry.id == id)
            .ok_or(Refusal::UnknownVersion)?,
        Before::NoId => return Err(Refusal::UnknownVersion),
    };
    let start = end.saturating_sub(limit);
    let rows = (start..end)
        .rev()
        .filter_map(|index| {
            let entry = *timeline.get(index)?;
            let previous = index
                .checked_sub(1)
                .and_then(|before| timeline.get(before))
                .map(|before| before.id);
            Some(Row { entry, previous })
        })
        .collect();
    let next_before = (start > 0)
        .then(|| timeline.get(start).map(|entry| entry.id))
        .flatten();
    Ok((rows, next_before))
}

/// Each version's files, for `(version, previous)` pairs, on the matter's thread.
fn files_of(
    matter: &Matter,
    pairs: &[(CheckpointId, Option<CheckpointId>)],
) -> Result<Vec<Files>, Refusal> {
    pairs
        .iter()
        .map(|&(version, previous)| {
            let read = match previous {
                Some(previous) => matter.changes_between(previous, version).map(|changes| {
                    let total = changes.len();
                    let listed = changes
                        .into_iter()
                        .take(MAX_FILES_PER_VERSION)
                        .map(|change| (change.path().clone(), change.change().clone()))
                        .collect();
                    Files { listed, total }
                }),
                None => matter.files_at(version).map(|files| Files {
                    total: files.len(),
                    listed: files
                        .into_iter()
                        .take(MAX_FILES_PER_VERSION)
                        .map(|file| (file.path, ChangeKind::Added))
                        .collect(),
                }),
            };
            read.map_err(|error| {
                tracing::warn!(error = %crate::log_file::Cause(&error), "a version's files did not read");
                Refusal::NotRead
            })
        })
        .collect()
}

fn file_json((path, change): &(MatterPath, ChangeKind)) -> Value {
    let name = match change {
        ChangeKind::Added => "added",
        ChangeKind::Modified => "modified",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Renamed { .. } => "renamed",
        ChangeKind::Unknown { .. } => "unknown",
    };
    let mut file = json!({ "path": path.as_str(), "change": name });
    if let (ChangeKind::Renamed { from }, Some(object)) = (change, file.as_object_mut()) {
        object.insert("from".to_owned(), json!(from.as_str()));
    }
    file
}

/// The kind's name in the tool's answer.
pub(super) const fn kind_name(kind: CheckpointKind) -> &'static str {
    match kind {
        CheckpointKind::Import => "import",
        CheckpointKind::BeforeTurn => "before_turn",
        CheckpointKind::AfterTurn => "after_turn",
        CheckpointKind::OutsideEdits => "outside_edits",
        CheckpointKind::Undo => "undo",
        CheckpointKind::Restore => "restore",
        CheckpointKind::Revisions => "revisions",
        CheckpointKind::Unknown => "unknown",
    }
}

/// Who made the version's changes, by its kind: a turn's agent; the lawyer in Clauseo (the
/// import, an undo, a restore, tracked changes decided); or someone outside Clauseo (a
/// `before-turn` checkpoint is made only when the folder changed since the last one).
const fn by(kind: CheckpointKind) -> &'static str {
    match kind {
        CheckpointKind::AfterTurn => "agent",
        CheckpointKind::Import
        | CheckpointKind::Undo
        | CheckpointKind::Restore
        | CheckpointKind::Revisions => "lawyer",
        CheckpointKind::BeforeTurn | CheckpointKind::OutsideEdits => "outside_clauseo",
        CheckpointKind::Unknown => "unknown",
    }
}
