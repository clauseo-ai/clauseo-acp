//! keyring 4.2.0's calls (its `v1` module, over keyring-core 1.0), as in `connectors`:
//! `Entry::store_status`, which sets the OS default store once per process;
//! `Entry::new(service, account)`; `Entry::set_secret(&[u8])`, `Entry::get_secret() -> Vec<u8>`
//! and `Entry::delete_credential()`, where `Error::NoEntry` means no item.

use clauseo_protocol::AgentId;
use keyring::{Entry, Error};

use super::{KEYCHAIN_SERVICE, KeyError, account};

/// Whether keyring's one-time store setup succeeded.
pub(super) fn available() -> bool {
    Entry::store_status().is_ok()
}

fn entry(agent: &AgentId) -> Result<Entry, KeyError> {
    if !available() {
        return Err(KeyError::NoKeychain {
            agent: agent.clone(),
        });
    }
    Entry::new(KEYCHAIN_SERVICE, &account(agent)).map_err(|err| map(agent, &err))
}

pub(super) fn get(agent: &AgentId) -> Result<Option<Vec<u8>>, KeyError> {
    match entry(agent)?.get_secret() {
        Ok(secret) => Ok(Some(secret)),
        Err(Error::NoEntry) => Ok(None),
        Err(err) => Err(map(agent, &err)),
    }
}

pub(super) fn set(agent: &AgentId, secret: &[u8]) -> Result<(), KeyError> {
    entry(agent)?
        .set_secret(secret)
        .map_err(|err| map(agent, &err))
}

pub(super) fn delete(agent: &AgentId) -> Result<(), KeyError> {
    match entry(agent)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(err) => Err(map(agent, &err)),
    }
}

/// A keyring error as a `KeyError` naming the agent and a fixed description of the error's kind:
/// some variants carry the item's bytes (`BadEncoding`, `BadDataFormat`), so neither their
/// `Debug` nor their `Display` is used.
fn map(agent: &AgentId, err: &Error) -> KeyError {
    let agent = agent.clone();
    let kind = match err {
        Error::NoStorageAccess(_) => return KeyError::NoAccess { agent },
        Error::NoDefaultStore => return KeyError::NoKeychain { agent },
        Error::TooLong(..) => "an attribute is too long",
        Error::Invalid(..) => "an invalid attribute",
        Error::NotSupportedByStore(_) => "not supported by the store",
        Error::Ambiguous(_) => "more than one matching item",
        Error::BadEncoding(_) | Error::BadDataFormat(..) | Error::BadStoreFormat(_) => {
            "an unreadable item"
        }
        Error::NoEntry => "no item",
        Error::PlatformFailure(_) => "a platform failure",
        _ => "an unknown failure",
    };
    KeyError::Keychain { agent, kind }
}
