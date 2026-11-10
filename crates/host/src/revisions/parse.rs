//! Reading stetdoc's answers into the wire's [`Revision`]s, and writing the edit batch.
//!
//! `check --json` gives each change's ids, kind, full author, block and date, by author;
//! `view --revisions --full --all --json` gives the text, in its `text` ledger, one line per
//! change: `#2,3 insert C 2026-11-09 p4 "New para."` (the text JSON-escaped; a formatting
//! change has none). The two are joined by ids.

use std::cmp::Ordering;
use std::collections::HashMap;

use clauseo_protocol::{
    ErrorCode, MAX_REVISION_TEXT_CHARS, MAX_REVISIONS, MatterPath, Revision, RevisionDecision,
    RevisionKind, Timestamp,
};
use serde::Deserialize;

use crate::error::Error;

/// Whether `path` names a Word document: it ends in `.docx`, in any case.
pub(crate) fn is_docx(path: &MatterPath) -> bool {
    std::path::Path::new(path.as_str())
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
}

/// The edit batch for `decision` on `ids`, in stetdoc's block text.
pub(super) fn batch(decision: RevisionDecision, ids: &[u32]) -> String {
    let op = match decision {
        RevisionDecision::Accept => "accept",
        RevisionDecision::Reject => "reject",
    };
    let ids: Vec<String> = ids.iter().map(u32::to_string).collect();
    format!("@{op}\nids: {}\n", ids.join(", "))
}

#[derive(Deserialize)]
struct Check {
    #[serde(default)]
    existing: Vec<ByAuthor>,
}

#[derive(Deserialize)]
struct ByAuthor {
    author: String,
    #[serde(default)]
    changes: Vec<Change>,
}

#[derive(Deserialize)]
struct Change {
    block: String,
    #[serde(default)]
    date: Option<String>,
    ids: Vec<u32>,
    kind: String,
}

#[derive(Deserialize)]
struct View {
    text: String,
}

/// The revisions `check` and `view` describe, in document order (by block, then id), the first
/// [`MAX_REVISIONS`], and whether there were more.
pub(super) fn revisions(
    check: &serde_json::Value,
    view: &serde_json::Value,
) -> Result<(Vec<Revision>, bool), Error> {
    let unreadable = |err: serde_json::Error| {
        // The class only: serde's message can quote authors or text (ADR 0047).
        tracing::warn!(class = ?err.classify(), "stetdoc's revisions did not read");
        Error::refused(ErrorCode::WordEditFailed, "stetdoc's answer did not read")
    };
    let check = Check::deserialize(check).map_err(unreadable)?;
    let view = View::deserialize(view).map_err(unreadable)?;
    let texts = &ledger_texts(&view.text);
    let mut revisions: Vec<Revision> = check
        .existing
        .into_iter()
        .flat_map(|by| {
            let author = cut(&by.author);
            by.changes.into_iter().map(move |change| {
                let mut ids = change.ids;
                ids.sort_unstable();
                let text = ids
                    .iter()
                    .find_map(|id| texts.get(id))
                    .map(|text| cut(text));
                Revision {
                    kind: kind(&change.kind),
                    author: author.clone(),
                    at: change.date.as_deref().and_then(timestamp),
                    block: change.block,
                    text,
                    ids,
                }
            })
        })
        .collect();
    revisions.sort_by(|a, b| {
        block_order(&a.block, &b.block).then_with(|| a.ids.first().cmp(&b.ids.first()))
    });
    let truncated = revisions.len() > MAX_REVISIONS;
    revisions.truncate(MAX_REVISIONS);
    Ok((revisions, truncated))
}

/// Each ledger line's text, by every id the line names.
fn ledger_texts(ledger: &str) -> HashMap<u32, String> {
    let mut texts = HashMap::new();
    for line in ledger.lines() {
        let Some(rest) = line.strip_prefix('#') else {
            continue;
        };
        let ids = rest.split(' ').next().unwrap_or_default();
        let Some(quoted) = line.find(" \"").and_then(|at| line.get(at + 1..)) else {
            continue;
        };
        let Ok(text) = serde_json::from_str::<String>(quoted) else {
            continue;
        };
        for id in ids.split(',').filter_map(|id| id.parse::<u32>().ok()) {
            texts.insert(id, text.clone());
        }
    }
    texts
}

/// stetdoc's change kind as the wire's.
fn kind(kind: &str) -> RevisionKind {
    if kind.contains("move") {
        RevisionKind::Move
    } else if kind.contains("insert") {
        RevisionKind::Insert
    } else if kind.contains("deletion") || kind.contains("deleted") {
        RevisionKind::Delete
    } else if [
        "format",
        "style",
        "property",
        "properties",
        "paragraph",
        "run",
        "section",
        "table",
        "number",
    ]
    .iter()
    .any(|word| kind.contains(word))
    {
        RevisionKind::Format
    } else {
        RevisionKind::Unknown
    }
}

/// `text` cut to [`MAX_REVISION_TEXT_CHARS`] characters.
fn cut(text: &str) -> String {
    text.chars().take(MAX_REVISION_TEXT_CHARS).collect()
}

/// Two block names in document order: their runs of digits compared as numbers (`p9` before
/// `p10`), the rest as text.
fn block_order(a: &str, b: &str) -> Ordering {
    let key = |name: &str| -> Vec<(String, u64)> {
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut number: Option<u64> = None;
        for c in name.chars() {
            match (c.to_digit(10), number) {
                (Some(digit), Some(n)) => {
                    number = Some(n.saturating_mul(10).saturating_add(u64::from(digit)));
                }
                (Some(digit), None) => number = Some(u64::from(digit)),
                (None, Some(n)) => {
                    parts.push((std::mem::take(&mut text), n));
                    number = None;
                    text.push(c);
                }
                (None, None) => text.push(c),
            }
        }
        parts.push((text, number.unwrap_or(0)));
        parts
    };
    key(a).cmp(&key(b))
}

/// `YYYY-MM-DDTHH:MM:SSZ` (Word's `w:date`, as stetdoc prints it) as a timestamp; `None` for
/// anything else.
pub(super) fn timestamp(text: &str) -> Option<Timestamp> {
    let bytes = text.as_bytes();
    let field = |from: usize, len: usize| -> Option<i64> {
        let part = text.get(from..from + len)?;
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let fixed = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if bytes.len() != 20
        || bytes.get(19) != Some(&b'Z')
        || fixed.iter().any(|(at, b)| bytes.get(*at) != Some(b))
    {
        return None;
    }
    let (year, month, day) = (field(0, 4)?, field(5, 2)?, field(8, 2)?);
    let (hour, minute, second) = (field(11, 2)?, field(14, 2)?, field(17, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    if second > 60 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    Timestamp::new(seconds.checked_mul(1_000)?).ok()
}

/// Days from 1970-01-01 to the proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
