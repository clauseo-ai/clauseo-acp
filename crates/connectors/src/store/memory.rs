//! [`MemoryStore`]: a keychain in memory, with knobs for tests.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use zeroize::Zeroize as _;

use super::{SecretStore, check_size};
use crate::error::StoreError;

/// A [`SecretStore`] in memory, for tests. Knobs: fail the next N writes, or one write after N
/// that land, fail every read, write or delete with a given error, count reads, writes and
/// deletes, read an item back. `Debug` names the accounts and never prints an item.
#[derive(Default)]
pub struct MemoryStore {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.lock();
        f.debug_struct("MemoryStore")
            .field("accounts", &inner.items.keys().collect::<Vec<_>>())
            .field("reads", &inner.reads)
            .field("writes", &inner.writes)
            .field("deletes", &inner.deletes)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct Inner {
    items: BTreeMap<String, Vec<u8>>,
    fail_writes: usize,
    /// How many more writes land before the one that fails ([`MemoryStore::fail_write_after`]).
    fail_after: Option<usize>,
    /// While set, every `get` fails with it (a keychain read refused).
    read_error: Option<StoreError>,
    /// While set, every `set` fails with it and changes nothing.
    write_error: Option<StoreError>,
    /// While set, every `delete` fails with it and the item stays.
    delete_error: Option<StoreError>,
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

    /// The next `landing` calls to `set` land; the one after fails with `StoreError::Platform`
    /// and changes nothing (a failure partway through a batch of writes).
    pub fn fail_write_after(&self, landing: usize) {
        self.lock().fail_after = Some(landing);
    }

    /// While `on`, every `get` fails with the error a keychain read refused without asking
    /// gives (macOS's errSecInteractionNotAllowed, which keyring reports as a platform failure),
    /// never `Ok(None)`: an item that is there but may not be read now.
    pub fn refuse_reads(&self, on: bool) {
        let error = on.then(|| StoreError::Platform("a platform failure".to_owned()));
        self.fail_reads_with(error);
    }

    /// While `Some`, every `get` fails with that error (as the keychain's own refusals map to
    /// it: a platform failure for errSecAuthFailed or a duplicate item, `NoAccess` for
    /// errSecNoAccessForItem), never `Ok(None)`.
    pub fn fail_reads_with(&self, error: Option<StoreError>) {
        self.lock().read_error = error;
    }

    /// While `Some`, every `set` fails with that error and the item stays as it was.
    pub fn fail_writes_with(&self, error: Option<StoreError>) {
        self.lock().write_error = error;
    }

    /// While `Some`, every `delete` fails with that error and the item stays.
    pub fn fail_deletes_with(&self, error: Option<StoreError>) {
        self.lock().delete_error = error;
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
        if let Some(error) = &inner.read_error {
            return Err(error.clone());
        }
        Ok(inner.items.get(account).cloned())
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        let mut inner = self.lock();
        inner.writes = inner.writes.saturating_add(1);
        let fails_now = match inner.fail_after {
            Some(0) => {
                inner.fail_after = None;
                true
            }
            Some(left) => {
                inner.fail_after = Some(left - 1);
                false
            }
            None => false,
        };
        if fails_now {
            return Err(StoreError::Platform(
                "a write failure (MemoryStore)".to_owned(),
            ));
        }
        if let Some(error) = &inner.write_error {
            return Err(error.clone());
        }
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
        if let Some(error) = &inner.delete_error {
            return Err(error.clone());
        }
        if let Some(mut old) = inner.items.remove(account) {
            old.zeroize();
        }
        Ok(())
    }
}
