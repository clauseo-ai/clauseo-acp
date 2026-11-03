//! Running a [`Query`]: newest first while a word is being typed, otherwise ranked over the
//! newest [`WINDOW`] matches; paging, filters and the rows turned into [`Hit`]s.

use std::str::FromStr;

use clauseo_protocol::{MatterId, Seq, ThreadId, Timestamp};
use rusqlite::{Connection, Row, ToSql};

use crate::cursor::{self, At, Cursor};
use crate::{Error, Hit, Kind, fts, snippet};

/// The most hits one page returns.
pub const MAX_LIMIT: u8 = 50;

/// Ranked mode ranks the newest this many matches. bm25 costs a pass over every match, so
/// ranking them all fails the 100 ms target past about 2,000 conversations (decision 0043).
pub const WINDOW: u32 = 2_000;

/// Recency halves a score at this age: 180 days in milliseconds.
const HALF_AGE_MS: f64 = 180.0 * 86_400_000.0;

/// How matches are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Newest first, the last word matching as a prefix: for a word still being typed. bm25 on
    /// a prefix term costs a full pass, so this mode does not rank.
    Typing,
    /// bm25 × kind weight / (1 + age / 180 days), over the newest [`WINDOW`] matches; every word
    /// is whole.
    Ranked,
}

impl Mode {
    /// [`Mode::Typing`] unless `text` ends in whitespace (the last word is finished).
    pub fn for_input(text: &str) -> Self {
        if text.ends_with(char::is_whitespace) {
            Self::Ranked
        } else {
            Self::Typing
        }
    }
}

/// A search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// What the lawyer typed. Never FTS5 syntax: every word is quoted.
    pub text: String,
    /// Only this matter; `None` searches every matter.
    pub matter: Option<MatterId>,
    /// Only these kinds; `None` searches every kind (an empty list matches nothing).
    pub kinds: Option<Vec<Kind>>,
    /// Whether archived conversations' rows are included.
    pub include_archived: bool,
    /// How matches are ordered.
    pub mode: Mode,
    /// The page after the one that returned this cursor; `None` for the first page.
    pub cursor: Option<Cursor>,
    /// Hits per page, 1 to [`MAX_LIMIT`]; larger values are cut to it, 0 is read as 1.
    pub limit: u8,
    /// The time ranked mode measures age from. Later pages use the first page's.
    pub now: Timestamp,
}

/// A rebuild's progress, in conversations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Conversations indexed so far.
    pub done: u64,
    /// Conversations to index.
    pub total: u64,
}

/// One page of results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// The hits, best first in ranked mode and newest first while typing.
    pub hits: Vec<Hit>,
    /// The next page, when there may be one.
    pub next: Option<Cursor>,
    /// The mode that ordered the hits.
    pub mode: Mode,
    /// While a rebuild runs, how far it is: the hits come from what is indexed so far.
    pub indexing: Option<Progress>,
}

const COLUMNS: &str = "i.kind, i.matter, i.thread, i.entry, i.path, i.at, m.title, t.title";
const TITLES: &str = "LEFT JOIN matters m ON m.matter = i.matter
     LEFT JOIN threads t ON t.thread = i.thread";
const SNIPPET: &str = "snippet(items_fts, 0, char(2), char(3), '…', 32)";

pub(crate) fn run(
    conn: &Connection,
    query: &Query,
    indexing: Option<Progress>,
) -> Result<Page, Error> {
    let check = cursor::check(query);
    // A cursor made for another query, or at a position no page has, starts over.
    let at = query
        .cursor
        .filter(|cursor| cursor.check == check && cursor.valid())
        .map(|cursor| cursor.at);
    let empty = Page {
        hits: Vec::new(),
        next: None,
        mode: query.mode,
        indexing,
    };
    let Some(fts) = fts::build(&query.text, query.mode == Mode::Typing) else {
        return Ok(empty);
    };
    if query.kinds.as_ref().is_some_and(Vec::is_empty) {
        return Ok(empty);
    }
    let limit = query.limit.clamp(1, MAX_LIMIT);
    let (hits, next) = match query.mode {
        Mode::Typing => typing(conn, query, &fts, limit, check, at)?,
        Mode::Ranked => ranked(conn, query, &fts, limit, check, at)?,
    };
    Ok(Page {
        hits,
        next,
        mode: query.mode,
        indexing,
    })
}

fn typing(
    conn: &Connection,
    query: &Query,
    fts: &str,
    limit: u8,
    check: u64,
    at: Option<At>,
) -> Result<(Vec<Hit>, Option<Cursor>), Error> {
    let before = match at {
        Some(At::Before(rowid)) => rowid,
        Some(At::Ranked { .. }) | None => i64::MAX,
    };
    let sql = format!(
        "SELECT items_fts.rowid, {COLUMNS}, {SNIPPET}
         FROM items_fts JOIN items i ON i.id = items_fts.rowid {TITLES}
         WHERE items_fts MATCH :q AND items_fts.rowid < :before {}
         ORDER BY items_fts.rowid DESC LIMIT :limit",
        filters(query)
    );
    let mut statement = conn.prepare_cached(&sql)?;
    let matter = query.matter.map(|m| m.to_string());
    let params: &[(&str, &dyn ToSql)] = &[
        (":q", &fts),
        (":before", &before),
        (":matter", &matter),
        (":archived", &query.include_archived),
        (":limit", &(i64::from(limit) + 1)),
    ];
    let mut rows = statement.query(params)?;
    let mut hits = Vec::new();
    let mut last = None;
    let mut more = false;
    while let Some(row) = rows.next()? {
        if hits.len() == usize::from(limit) {
            more = true;
            break;
        }
        last = Some(row.get::<_, i64>(0)?);
        let raw: String = row.get(9)?;
        hits.push(hit(row, &raw)?);
    }
    let next = match (more, last) {
        (true, Some(rowid)) => Some(Cursor {
            check,
            at: At::Before(rowid),
        }),
        _ => None,
    };
    Ok((hits, next))
}

fn ranked(
    conn: &Connection,
    query: &Query,
    fts: &str,
    limit: u8,
    check: u64,
    at: Option<At>,
) -> Result<(Vec<Hit>, Option<Cursor>), Error> {
    let (offset, top, now) = match at {
        Some(At::Ranked { offset, top, now }) => (offset, top, now),
        Some(At::Before(_)) | None => {
            let top: i64 = conn.query_row("SELECT coalesce(max(id), 0) FROM items", [], |row| {
                row.get(0)
            })?;
            (0, top, query.now.as_millis())
        }
    };
    let sql = format!(
        "SELECT w.id, {COLUMNS}
         FROM (SELECT items_fts.rowid AS id, bm25(items_fts) AS r
               FROM items_fts JOIN items i ON i.id = items_fts.rowid
               WHERE items_fts MATCH :q AND items_fts.rowid <= :top {}
               ORDER BY items_fts.rowid DESC LIMIT {WINDOW}) w
         JOIN items i ON i.id = w.id {TITLES}
         ORDER BY w.r * (CASE i.kind WHEN 1 THEN 3.0 WHEN 2 THEN 3.0 WHEN 6 THEN 2.0
                         WHEN 3 THEN 1.2 WHEN 5 THEN 0.8 ELSE 1.0 END)
                  / (1.0 + max(0, :now - i.at) / {HALF_AGE_MS:.1}), w.id DESC
         LIMIT :limit OFFSET :offset",
        filters(query)
    );
    let mut statement = conn.prepare_cached(&sql)?;
    let matter = query.matter.map(|m| m.to_string());
    let params: &[(&str, &dyn ToSql)] = &[
        (":q", &fts),
        (":top", &top),
        (":matter", &matter),
        (":archived", &query.include_archived),
        (":now", &now),
        (":limit", &(i64::from(limit) + 1)),
        (":offset", &offset),
    ];
    let mut rows = statement.query(params)?;
    let mut found: Vec<(i64, Hit)> = Vec::new();
    let mut more = false;
    while let Some(row) = rows.next()? {
        if found.len() == usize::from(limit) {
            more = true;
            break;
        }
        found.push((row.get(0)?, hit(row, "")?));
    }
    drop(rows);
    snippets(conn, fts, &mut found)?;
    let end = offset.saturating_add(u32::from(limit));
    let next = (more && end < WINDOW).then_some(Cursor {
        check,
        at: At::Ranked {
            offset: end,
            top,
            now,
        },
    });
    Ok((found.into_iter().map(|(_, hit)| hit).collect(), next))
}

/// Fills each hit's snippet with a second statement over just the page's rows.
fn snippets(conn: &Connection, fts: &str, found: &mut [(i64, Hit)]) -> Result<(), Error> {
    if found.is_empty() {
        return Ok(());
    }
    let mut ids = String::from("[");
    for (i, (id, _)) in found.iter().enumerate() {
        if i > 0 {
            ids.push(',');
        }
        ids.push_str(&id.to_string());
    }
    ids.push(']');
    let mut statement = conn.prepare_cached(&format!(
        "SELECT rowid, {SNIPPET} FROM items_fts
         WHERE items_fts MATCH ?1 AND rowid IN (SELECT value FROM json_each(?2))"
    ))?;
    let mut rows = statement.query((fts, ids))?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let raw: String = row.get(1)?;
        if let Some((_, hit)) = found.iter_mut().find(|(found, _)| *found == id) {
            hit.snippet = snippet::parts(&raw);
        }
    }
    Ok(())
}

/// The WHERE terms for the scope, the archived choice and the kinds. The kinds are numbers this
/// code writes, never text from the query.
fn filters(query: &Query) -> String {
    let mut out = String::from(
        "AND (:matter IS NULL OR i.matter = :matter) AND (:archived OR i.archived = 0)",
    );
    if let Some(kinds) = &query.kinds {
        out.push_str(" AND i.kind IN (");
        for (i, kind) in kinds.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&kind.code().to_string());
        }
        out.push(')');
    }
    out
}

/// Columns 1 to 8 of a row as a hit, with the snippet from `raw`.
fn hit(row: &Row<'_>, raw: &str) -> Result<Hit, Error> {
    let code: i64 = row.get(1)?;
    let kind = Kind::from_code(code).ok_or_else(|| Error::Corrupt(format!("kind {code}")))?;
    let matter: String = row.get(2)?;
    let thread: Option<String> = row.get(3)?;
    let entry: Option<i64> = row.get(4)?;
    let at: i64 = row.get(6)?;
    Ok(Hit {
        kind,
        matter: parse(&matter)?,
        thread: thread.as_deref().map(parse::<ThreadId>).transpose()?,
        entry: entry
            .map(|e| {
                u64::try_from(e)
                    .ok()
                    .and_then(|e| Seq::new(e).ok())
                    .ok_or_else(|| Error::Corrupt(format!("entry {e}")))
            })
            .transpose()?,
        path: row.get(5)?,
        at: Timestamp::new(at).map_err(|error| Error::Corrupt(error.to_string()))?,
        matter_title: row.get(7)?,
        thread_title: row.get(8)?,
        snippet: snippet::parts(raw),
    })
}

fn parse<T: FromStr<Err = clauseo_protocol::Error>>(text: &str) -> Result<T, Error> {
    text.parse()
        .map_err(|error: clauseo_protocol::Error| Error::Corrupt(error.to_string()))
}
