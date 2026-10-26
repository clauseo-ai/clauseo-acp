//! Opening the file: the exclusive lock, the private mode, the pragmas and the migrations.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::{Error, StoreOptions, write};

/// The database's file name in the folder the caller passes.
pub const DB_FILE: &str = "state.sqlite";
/// The lock file beside it, held while the store is open.
pub const LOCK_FILE: &str = "state.sqlite.lock";

/// The numbered migrations: entry `i` takes `user_version` from `i` to `i + 1`.
const MIGRATIONS: &[&str] = &[include_str!("migrations/0001_init.sql")];

/// The schema version this build writes, the number of the last migration.
pub const SCHEMA: u32 = 1;

/// Statements each connection keeps prepared; the writer uses about twenty.
const STATEMENT_CACHE: usize = 32;

/// The writer's connection, with the lock that makes it the only one.
pub(crate) struct Opened {
    pub(crate) conn: Connection,
    pub(crate) lock: File,
    pub(crate) path: PathBuf,
}

/// Takes the lock, creates the file if new, sets the writer's pragmas and migrates.
pub(crate) fn open_writer(options: &StoreOptions) -> Result<Opened, Error> {
    let lock = write::open_private(&options.dir.join(LOCK_FILE))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Err(Error::Locked),
        Err(TryLockError::Error(error)) => return Err(error.into()),
    }
    let path = options.dir.join(DB_FILE);
    // Created here with mode 0o600 before SQLite opens it; SQLite gives -wal and -shm the
    // database's mode.
    drop(write::open_private(&path)?);
    let conn = Connection::open(&path)?;
    conn.set_prepared_statement_cache_capacity(STATEMENT_CACHE);
    // Before journal_mode and before the first CREATE: the page size of an existing file
    // cannot change, and on one this is a no-op.
    conn.execute_batch("PRAGMA page_size = 16384")?;
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(Error::corrupt(format!("journal_mode is {mode}, not wal")));
    }
    conn.execute_batch(
        "PRAGMA synchronous = FULL;
         PRAGMA temp_store = MEMORY;
         PRAGMA foreign_keys = ON;",
    )?;
    #[cfg(target_os = "macos")]
    conn.execute_batch("PRAGMA fullfsync = ON")?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    let _limit: i64 =
        conn.query_row("PRAGMA journal_size_limit = 33554432", [], |row| row.get(0))?;
    if let Some(pages) = options.max_page_count {
        let _pages: i64 =
            conn.query_row(&format!("PRAGMA max_page_count = {pages}"), [], |row| {
                row.get(0)
            })?;
    }
    migrate(&conn)?;
    Ok(Opened { conn, lock, path })
}

/// The reader's connection: the same file, `query_only`, SQLite's default cache, no mmap.
pub(crate) fn open_reader(path: &Path) -> Result<Connection, Error> {
    let conn = Connection::open(path)?;
    conn.set_prepared_statement_cache_capacity(STATEMENT_CACHE);
    conn.execute_batch("PRAGMA query_only = ON")?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    Ok(conn)
}

/// Runs each migration past the file's `user_version`, each in one transaction that also sets
/// `user_version`. A file newer than this build is refused.
fn migrate(conn: &Connection) -> Result<(), Error> {
    let found: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let found = u32::try_from(found)
        .map_err(|error| Error::corrupt(format!("user_version {found}: {error}")))?;
    if found > SCHEMA {
        return Err(Error::NewerSchema {
            found,
            supported: SCHEMA,
        });
    }
    for (version, sql) in (1..).zip(MIGRATIONS).skip(found as usize) {
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE;\n{sql}\nPRAGMA user_version = {version};\nCOMMIT;"
        ))
        .inspect_err(|_| {
            if !conn.is_autocommit() {
                drop(conn.execute_batch("ROLLBACK"));
            }
        })?;
    }
    Ok(())
}
