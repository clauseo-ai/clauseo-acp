//! Opening `search.sqlite`: the pragmas, the schema, and the rule that an index this code cannot
//! trust is emptied and reported for a rebuild.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use crate::{Error, write};

/// The index file's name inside the folder the caller passes.
pub const FILE_NAME: &str = "search.sqlite";

/// The index format. Any change to the schema, the tokenizer or what a column means bumps it;
/// an index of another format is emptied and rebuilt, never migrated.
pub const FORMAT: u32 = 1;

/// The FTS5 tokenizer. `M*` keeps combining marks (Devanagari vowel signs and the virama) inside
/// words; `remove_diacritics 2` folds Latin accents.
pub const TOKENIZER: &str = "unicode61 remove_diacritics 2 categories 'L* N* Co M*'";

/// What [`SearchIndex::open`](crate::SearchIndex::open) found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenState {
    /// The file was missing, did not open, had another [`FORMAT`] or [`TOKENIZER`], or holds a
    /// build that never reached [`finish_rebuild`](crate::SearchIndex::finish_rebuild). The index
    /// is empty and the caller rebuilds it from the log.
    pub needs_rebuild: bool,
}

const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE items (
  id INTEGER PRIMARY KEY,
  kind INTEGER NOT NULL,
  matter TEXT NOT NULL,
  thread TEXT,
  entry INTEGER,
  path TEXT,
  content TEXT,
  part INTEGER,
  at INTEGER NOT NULL,
  archived INTEGER NOT NULL DEFAULT 0,
  body TEXT NOT NULL
);
CREATE INDEX items_thread ON items(thread);
CREATE INDEX items_matter ON items(matter);
CREATE TABLE matters (matter TEXT PRIMARY KEY, title TEXT NOT NULL);
CREATE TABLE threads (
  thread TEXT PRIMARY KEY,
  matter TEXT NOT NULL,
  title TEXT,
  archived INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX threads_matter ON threads(matter);
";

/// Opens the index in `folder`, or creates it empty; the flag is `needs_rebuild`.
pub(crate) fn open(folder: &Path) -> Result<(Connection, bool), Error> {
    let path = folder.join(FILE_NAME);
    if path.try_exists()? {
        // Any failure to read what is there means the same as a missing file: rebuild.
        if let Ok(Some(conn)) = open_existing(&path) {
            return Ok((conn, false));
        }
    }
    discard(&path)?;
    Ok((create(&path)?, true))
}

/// The connection when the file is ours, of this format and completely built.
fn open_existing(path: &Path) -> Result<Option<Connection>, Error> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    let format = meta(&conn, "format")?;
    let tokenizer = meta(&conn, "tokenizer")?;
    let complete = meta(&conn, "complete")?;
    let ours = format.as_deref() == Some(FORMAT.to_string().as_str())
        && tokenizer.as_deref() == Some(TOKENIZER)
        && complete.as_deref() == Some("1");
    Ok(ours.then_some(conn))
}

fn meta(conn: &Connection, key: &str) -> Result<Option<String>, Error> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

/// Removes the file and its WAL and shared-memory files. No connection is open on them.
fn discard(path: &Path) -> Result<(), Error> {
    write::remove_if_present(path)?;
    for suffix in ["-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        write::remove_if_present(Path::new(&name))?;
    }
    Ok(())
}

fn create(path: &Path) -> Result<Connection, Error> {
    // Created here with mode 0o600 before SQLite opens it; SQLite gives -wal and -shm the
    // database's mode.
    write::create_private(path)?;
    let mut conn = Connection::open(path)?;
    configure(&conn)?;
    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    tx.execute_batch(&format!(
        "CREATE VIRTUAL TABLE items_fts USING fts5(body, content='items', content_rowid='id', \
         tokenize=\"{TOKENIZER}\");
         INSERT INTO items_fts(items_fts, rank) VALUES('secure-delete', 1);"
    ))?;
    tx.execute(
        "INSERT INTO meta(key, value) VALUES ('format', ?1), ('tokenizer', ?2), \
         ('complete', '0')",
        [FORMAT.to_string().as_str(), TOKENIZER],
    )?;
    tx.commit()?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<(), Error> {
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(Error::Corrupt(format!("journal_mode is {mode}, not wal")));
    }
    // NORMAL: a crash may lose the last writes of a derived file, never corrupt it.
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;
         PRAGMA secure_delete = ON;
         PRAGMA temp_store = MEMORY;",
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    Ok(())
}

/// Marks the build complete (the next open trusts the file) or not (the next open empties it).
pub(crate) fn set_complete(conn: &Connection, complete: bool) -> Result<(), Error> {
    conn.execute(
        "UPDATE meta SET value = ?1 WHERE key = 'complete'",
        [if complete { "1" } else { "0" }],
    )?;
    Ok(())
}
