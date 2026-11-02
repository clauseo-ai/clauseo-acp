//! [`MemoryStore`]: a keychain in memory, with knobs for tests.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use zeroize::Zeroize as _;

use super::{SecretStore, check_size};
use crate::error::StoreError;

/// A [`SecretStore`] in memory, for tests. Knobs: fail the next N writes, count reads, writes and
/// deletes, read an item back.
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    items: BTreeMap<String, Vec<u8>>,
    fail_writes: usize,
    reads: usize,
    writes: usize,
    deletes: usize,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The next `n` calls to `set` fail with `StoreError::Platform` and change nothing.
    pub fn fail_next_writes(&self, n: usize) {
        self.lock().fail_writes = n;
    }

    /// How many times `get` was called.
    pub fn reads(&self) -> usize {
        self.lock().reads
    }

    /// How many times `set` was called, failed calls included.
    pub fn writes(&self) -> usize {
        self.lock().writes
    }

    /// How many times `delete` was called.
    pub fn deletes(&self) -> usize {
        self.lock().deletes
    }

    /// The item's bytes, without counting a read.
    pub fn peek(&self, account: &str) -> Option<Vec<u8>> {
        self.lock().items.get(account).cloned()
    }

    /// Writes an item directly (an earlier install's, or garbage), without counting a write.
    pub fn put(&self, account: &str, secret: &[u8]) {
        self.lock()
            .items
            .insert(account.to_owned(), secret.to_vec());
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let mut inner = self.lock();
        inner.reads = inner.reads.saturating_add(1);
        Ok(inner.items.get(account).cloned())
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        let mut inner = self.lock();
        inner.writes = inner.writes.saturating_add(1);
        if inner.fail_writes > 0 {
            inner.fail_writes -= 1;
            return Err(StoreError::Platform(
                "a write failure (MemoryStore)".to_owned(),
            ));
        }
        check_size(secret)?;
        if let Some(mut old) = inner.items.insert(account.to_owned(), secret.to_vec()) {
            old.zeroize();
        }
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), StoreError> {
        let mut inner = self.lock();
        inner.deletes = inner.deletes.saturating_add(1);
        if let Some(mut old) = inner.items.remove(account) {
            old.zeroize();
        }
        Ok(())
    }
}
