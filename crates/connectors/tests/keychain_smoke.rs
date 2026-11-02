//! The OS keychain, for real: run by hand and in CI on macOS and Windows with
//! `cargo nextest run -p clauseo-connectors --run-ignored only -E 'test(keychain_smoke)'`.
//! It writes a 2,000-byte secret with `KeyringStore` under a fresh account name, reads it back
//! byte for byte, deletes it, and reads `None`. A guard deletes the item however the test ends.

use clauseo_connectors::{KeyringStore, MAX_SECRET_BYTES, SecretStore, new_relay_token};

/// Deletes the item on drop.
#[cfg(test)]
struct Cleanup<'a> {
    store: &'a KeyringStore,
    account: &'a str,
}

#[cfg(test)]
impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        if let Err(err) = self.store.delete(self.account) {
            eprintln!("keychain_smoke: could not delete {}: {err}", self.account);
        }
    }
}

#[test]
#[ignore = "touches the OS keychain; run by hand and in CI's macOS and Windows jobs"]
fn keychain_smoke() {
    let store = KeyringStore::new().unwrap();
    let account = format!("keychain-smoke-{}", &new_relay_token().expose()[..16]);
    let _cleanup = Cleanup {
        store: &store,
        account: &account,
    };
    let secret: Vec<u8> = (0..MAX_SECRET_BYTES)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    assert_eq!(secret.len(), 2_000);
    store.set(&account, &secret).unwrap();
    assert_eq!(
        store.get(&account).unwrap().as_deref(),
        Some(secret.as_slice())
    );
    store.delete(&account).unwrap();
    assert_eq!(store.get(&account).unwrap(), None);
    store.delete(&account).unwrap();
}
