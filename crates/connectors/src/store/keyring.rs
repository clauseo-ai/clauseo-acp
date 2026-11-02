//! keyring 4.2.0's calls (its `v1` module, over keyring-core 1.0): `Entry::store_status`, which
//! sets the OS default store once per process; `Entry::new(service, account)`;
//! `Entry::set_secret(&[u8])`, `Entry::get_secret() -> Vec<u8>` and `Entry::delete_credential()`,
//! where `Error::NoEntry` means no item.

use keyring::{Entry, Error};

use crate::error::StoreError;
use crate::known::KEYCHAIN_SERVICE;

/// keyring's one-time store setup, or its error.
pub(super) fn init() -> Result<(), StoreError> {
    match Entry::store_status() {
        Ok(()) => Ok(()),
        Err(err) => Err(map(err)),
    }
}

fn entry(account: &str) -> Result<Entry, StoreError> {
    Entry::new(KEYCHAIN_SERVICE, account).map_err(|err| map(&err))
}

pub(super) fn get(account: &str) -> Result<Option<Vec<u8>>, StoreError> {
    match entry(account)?.get_secret() {
        Ok(secret) => Ok(Some(secret)),
        Err(Error::NoEntry) => Ok(None),
        Err(err) => Err(map(&err)),
    }
}

pub(super) fn set(account: &str, secret: &[u8]) -> Result<(), StoreError> {
    entry(account)?.set_secret(secret).map_err(|err| map(&err))
}

pub(super) fn delete(account: &str) -> Result<(), StoreError> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(err) => Err(map(&err)),
    }
}

/// A keyring error as a `StoreError` with a fixed description of its kind: some variants carry
/// the item's bytes (`BadEncoding`, `BadDataFormat`), so neither their `Debug` nor their
/// `Display` is ever used.
fn map(err: &Error) -> StoreError {
    let kind = match err {
        Error::NoStorageAccess(_) => return StoreError::NoAccess("no storage access".to_owned()),
        Error::TooLong(..) => "an attribute is too long",
        Error::Invalid(..) => "an invalid attribute",
        Error::NoDefaultStore => "no default store",
        Error::NotSupportedByStore(_) => "not supported by the store",
        Error::Ambiguous(_) => "more than one matching item",
        Error::BadEncoding(_) | Error::BadDataFormat(..) | Error::BadStoreFormat(_) => {
            "an unreadable item"
        }
        Error::NoEntry => "no item",
        Error::PlatformFailure(_) => "a platform failure",
        _ => "an unknown failure",
    };
    StoreError::Platform(kind.to_owned())
}
