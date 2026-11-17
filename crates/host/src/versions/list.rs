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
use serde_json::{Map, Value};

use super::mcp::Outcome;
use super::texts::Refusal;
use super::{
    Backend, DEFAULT_LIMIT, LINE_SEPARATORS, MAX_FILES_PER_VERSION, MAX_LIMIT, VERSIONS_PER_JOB,
    utc_time,
};

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
    } = backend.on_matter(move |opened| {
        let matter = opened.matter;
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
        files.extend(backend.on_matter(move |opened| files_of(opened.matter, &pairs))?);
    }
    let ids: Vec<CheckpointId> = rows.iter().map(|row| row.entry.id).collect();
    let session = backend.config.session;
    let logged = backend.config.source.logged(session.matter, &ids);
    let next = next_before.map_or_else(
        || "none (no older versions)".to_owned(),
        |id| id.to_string(),
    );
    let mut lines = vec![
        format!("versions: {}, newest first", rows.len()),
        format!("next_before: {next}"),
    ];
    for (row, files) in rows.iter().zip(files) {
        let entry = &row.entry;
        let at = logged
            .at
            .get(&entry.id)
            .map_or_else(|| "unknown".to_owned(), |at| utc_time(*at));
        let this = if entry.thread == Some(session.thread) {
            "yes"
        } else {
            "no"
        };
        lines.push(String::new());
        lines.push(format!("id: {}", entry.id));
        lines.push(format!("at: {at}"));
        lines.push(format!("kind: {}", kind_name(entry.kind)));
        lines.push(format!("by: {}", by(entry.kind)));
        lines.push(format!("this_conversation: {this}"));
        if let Some(from) = logged.restored_from.get(&entry.id) {
            lines.push(format!("restored_from: {from}"));
        }
        lines.push(format!("files_total: {}", files.total));
        lines.push("files:".to_owned());
        for file in &files.listed {
            file_lines(&mut lines, file);
        }
    }
    Ok(lines.join("\n"))
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

/// One file's lines: `  <change>: <path>`, and for a rename `    from: <old path>` after it. A
/// path holds no line break (`MatterPath` refuses control characters, and [`shown`] writes the
/// line and paragraph separators as text), so each is one line.
fn file_lines(lines: &mut Vec<String>, (path, change): &(MatterPath, ChangeKind)) {
    let name = match change {
        ChangeKind::Added => "added",
        ChangeKind::Modified => "modified",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Renamed { .. } => "renamed",
        ChangeKind::Unknown { .. } => "unknown",
    };
    lines.push(format!("  {name}: {}", shown(path)));
    if let ChangeKind::Renamed { from } = change {
        lines.push(format!("    from: {}", shown(from)));
    }
}

/// `path` as the list writes it: each of [`LINE_SEPARATORS`] as the text `\u{2028}` or
/// `\u{2029}`.
fn shown(path: &MatterPath) -> String {
    path.as_str()
        .replace(LINE_SEPARATORS[0], "\\u{2028}")
        .replace(LINE_SEPARATORS[1], "\\u{2029}")
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
