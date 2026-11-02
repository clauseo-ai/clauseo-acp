//! [`SecretStore`]: where the credential lives. [`KeyringStore`] is the OS keychain;
//! [`MemoryStore`] (feature `test-support`) stands in for it in tests.

#[cfg(any(target_os = "macos", windows))]
mod keyring;
#[cfg(feature = "test-support")]
mod memory;

#[cfg(feature = "test-support")]
pub use memory::MemoryStore;

use crate::error::StoreError;

/// The largest value a store takes, in bytes. Windows Credential Manager caps an item at 2,560
/// bytes (`CRED_MAX_CREDENTIAL_BLOB_SIZE`); the credential JSON is under 300 today.
pub const MAX_SECRET_BYTES: usize = 2_000;

/// One item per connector, under the account `<connector id>`.
pub trait SecretStore: Send + Sync + 'static {
    /// The item's bytes, or `None` when there is none.
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, StoreError>;
    /// Writes the item, replacing any; refuses a value over [`MAX_SECRET_BYTES`].
    fn set(&self, account: &str, secret: &[u8]) -> Result<(), StoreError>;
    /// Deletes the item; `Ok` when there is none.
    fn delete(&self, account: &str) -> Result<(), StoreError>;
}

/// The OS keychain under [`KEYCHAIN_SERVICE`](crate::KEYCHAIN_SERVICE): Keychain Services on
/// macOS, Credential Manager on Windows, raw bytes (`set_secret`, `get_secret`,
/// `delete_credential` of keyring 4). Elsewhere every call answers [`StoreError::Unsupported`].
#[derive(Debug, Clone, Copy)]
pub struct KeyringStore {
    _private: (),
}

impl KeyringStore {
    /// Sets up the OS store once per process (keyring 4's default store); fails when the OS
    /// store cannot be opened, or with `Unsupported` off macOS and Windows.
    pub fn new() -> Result<KeyringStore, StoreError> {
        #[cfg(any(target_os = "macos", windows))]
        {
            keyring::init()?;
            Ok(KeyringStore { _private: () })
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            Err(StoreError::Unsupported)
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, StoreError> {
        keyring::get(account)
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        check_size(secret)?;
        keyring::set(account, secret)
    }

    fn delete(&self, account: &str) -> Result<(), StoreError> {
        keyring::delete(account)
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
impl SecretStore for KeyringStore {
    fn get(&self, _account: &str) -> Result<Option<Vec<u8>>, StoreError> {
        Err(StoreError::Unsupported)
    }

    fn set(&self, _account: &str, _secret: &[u8]) -> Result<(), StoreError> {
        Err(StoreError::Unsupported)
    }

    fn delete(&self, _account: &str) -> Result<(), StoreError> {
        Err(StoreError::Unsupported)
    }
}

/// Refuses a value over [`MAX_SECRET_BYTES`].
pub(crate) fn check_size(secret: &[u8]) -> Result<(), StoreError> {
    if secret.len() > MAX_SECRET_BYTES {
        return Err(StoreError::TooLarge { len: secret.len() });
    }
    Ok(())
}
