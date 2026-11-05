//! API keys: the lawyer's key per agent (Claude's `ANTHROPIC_API_KEY`, Codex's
//! `CODEX_API_KEY`), kept in the OS keychain and nowhere else.
//!
//! # Design
//!
//! - One keychain item per agent: service [`KEYCHAIN_SERVICE`] (`ai.clauseo.desktop`), account
//!   `agent-key/<agent id>` ([`account`]). The value is the key's UTF-8 bytes (keyring 4's
//!   `set_secret` and `get_secret`): Keychain Services on macOS, Credential Manager on Windows.
//! - Linux has no keychain in this build, as in `connectors`: `keyring` is not a Linux dependency
//!   (its Linux store is the Secret Service, and Linux is not a product target). There
//!   [`available`] is false (it feeds `SetupInfo { keys_available }`), [`store`] answers
//!   [`KeyError::NoKeychain`], [`load`] answers `Ok(None)` and [`clear`] answers `Ok(())`: no key
//!   can be kept there, so none is found and none is left to remove.
//! - The key is never logged, never in a `Debug` or an error's text, and never written to a file.
//!   An error names the agent and the keychain's error kind only: some keyring errors carry the
//!   item's bytes (`BadEncoding`, `BadDataFormat`), so neither their `Debug` nor their `Display`
//!   is ever used. A loaded key comes back as `Zeroizing<String>`; an unreadable item's bytes
//!   are zeroed before the error returns.
//! - The rule for a key is one function, [`refusal`]: it takes only printable ASCII, bytes
//!   0x21–0x7E (no space, no line break, no control character, no non-ASCII such as U+200B or
//!   U+FEFF), and at most [`MAX_KEY_BYTES`] (Credential Manager caps an item at 2,560 bytes). Real
//!   Anthropic and `OpenAI` keys are printable ASCII. [`store`] applies it before writing, and
//!   [`load`] applies it to the stored item, so a key edited in Keychain Access or planted by
//!   another app cannot reach the agent's environment. The caller trims what the lawyer pasted.
//! - A stored item that fails the rule is a [`KeyError::StoredInvalid`], never `Ok(None)`, which
//!   would read as "no key".
//! - Every call is a blocking keychain call (macOS may show a prompt), so the caller keeps them
//!   off the host's actor: desktop loads the keys once at start on the agents thread and keeps
//!   them in memory.
//! - Every write in this process ([`Keychain::store`], [`Keychain::clear`] and
//!   [`Keychain::store_if_absent`]) takes one lock for the whole process, so
//!   `store_if_absent`'s check and store are one step against every other write here. The OS
//!   stores have no create-only write (Credential Manager has none), so another process can
//!   still write between the two: desktop's single-instance plugin keeps a second Clauseo from
//!   running.
//! - Test seam: [`Keychain`], whose [`Keychain::os`] is the free functions' keychain. Tests use
//!   [`Keychain::memory`] (an in-memory store) and [`Keychain::none`] (the Linux rule), never the
//!   real keychain.
//! - Keychain items belong to the app's signature: an unsigned build, or a re-signed one, may not
//!   read an item an earlier build stored, and the lawyer enters the key again.

#[cfg(any(target_os = "macos", windows))]
mod os;

#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

use clauseo_protocol::AgentId;
use zeroize::Zeroizing;

#[cfg(any(test, feature = "test-support"))]
use std::collections::BTreeMap;
#[cfg(any(test, feature = "test-support"))]
use std::sync::Arc;

/// The keychain service every agent's key is stored under.
pub const KEYCHAIN_SERVICE: &str = "ai.clauseo.desktop";

/// The longest key [`store`] takes, in bytes.
pub const MAX_KEY_BYTES: usize = 2_000;

/// The keychain account for `agent`'s key: `agent-key/<agent id>`.
pub fn account(agent: &AgentId) -> String {
    format!("agent-key/{}", agent.as_str())
}

/// Whether this computer has a usable keychain: macOS and Windows when the OS store opens, never
/// on Linux.
pub fn available() -> bool {
    Keychain::os().available()
}

/// Stores `key` as `agent`'s API key in the OS keychain, replacing any.
pub fn store(agent: &AgentId, key: &str) -> Result<(), KeyError> {
    Keychain::os().store(agent, key)
}

/// `agent`'s API key from the OS keychain, or `None` when there is none.
pub fn load(agent: &AgentId) -> Result<Option<Zeroizing<String>>, KeyError> {
    Keychain::os().load(agent)
}

/// Stores `key` as `agent`'s API key in the OS keychain only when it has none: see
/// [`Keychain::store_if_absent`].
pub fn store_if_absent(agent: &AgentId, key: &str) -> Result<bool, KeyError> {
    Keychain::os().store_if_absent(agent, key)
}

/// Removes `agent`'s API key from the OS keychain; `Ok` when there is none.
pub fn clear(agent: &AgentId) -> Result<(), KeyError> {
    Keychain::os().clear(agent)
}

/// Why a key could not be stored, loaded or cleared. The text names the agent and the kind of
/// failure, never the key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// This computer has no keychain Clauseo can use (Linux, or an OS store that did not open).
    #[error("no keychain on this computer to keep {agent}'s API key")]
    NoKeychain {
        /// The agent whose key it was.
        agent: AgentId,
    },
    /// [`store`] refused the key; nothing was written.
    #[error("{agent}'s API key was not stored: {reason}")]
    Invalid {
        /// The agent whose key it was.
        agent: AgentId,
        /// What is wrong with it, without the key.
        reason: &'static str,
    },
    /// The keychain holds an item for `agent` that is not a key [`store`] would take (edited in
    /// Keychain Access, or planted by another app). It is not returned.
    #[error("the stored key is not valid: {agent}'s API key in the keychain must be entered again")]
    StoredInvalid {
        /// The agent whose key it was.
        agent: AgentId,
    },
    /// The keychain refused access (locked, or the lawyer denied the prompt).
    #[error("the keychain refused access to {agent}'s API key")]
    NoAccess {
        /// The agent whose key it was.
        agent: AgentId,
    },
    /// Any other keychain failure, by kind.
    #[error("the keychain failed on {agent}'s API key: {kind}")]
    Keychain {
        /// The agent whose key it was.
        agent: AgentId,
        /// A fixed description of the keychain's error kind.
        kind: &'static str,
    },
}

/// Where keys go: the OS keychain, none (Linux), or memory (tests). The free functions use
/// [`Keychain::os`]. `Debug` names the backend and, for memory, its accounts; never a key.
#[derive(Clone)]
pub struct Keychain {
    backend: Backend,
}

#[derive(Clone)]
enum Backend {
    None,
    #[cfg(any(target_os = "macos", windows))]
    Os,
    #[cfg(any(test, feature = "test-support"))]
    Memory(Arc<Mutex<Items>>),
}

impl fmt::Debug for Keychain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.backend {
            Backend::None => f.write_str("Keychain(none)"),
            #[cfg(any(target_os = "macos", windows))]
            Backend::Os => f.write_str("Keychain(os)"),
            #[cfg(any(test, feature = "test-support"))]
            Backend::Memory(items) => f
                .debug_tuple("Keychain::memory")
                .field(&lock(items).keys().collect::<Vec<_>>())
                .finish(),
        }
    }
}

impl Keychain {
    /// The OS keychain on macOS and Windows; [`Keychain::none`] elsewhere.
    pub fn os() -> Keychain {
        #[cfg(any(target_os = "macos", windows))]
        let backend = Backend::Os;
        #[cfg(not(any(target_os = "macos", windows)))]
        let backend = Backend::None;
        Keychain { backend }
    }

    /// No keychain, as on Linux: not available, nothing stored, nothing found.
    pub fn none() -> Keychain {
        Keychain {
            backend: Backend::None,
        }
    }

    /// Tests only: an empty keychain in memory. Clones share it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn memory() -> Keychain {
        Keychain {
            backend: Backend::Memory(Arc::default()),
        }
    }

    /// Whether keys can be kept: the OS store opened (macOS, Windows), or memory.
    pub fn available(&self) -> bool {
        match &self.backend {
            Backend::None => false,
            #[cfg(any(target_os = "macos", windows))]
            Backend::Os => os::available(),
            #[cfg(any(test, feature = "test-support"))]
            Backend::Memory(_) => true,
        }
    }

    /// Stores `key` as `agent`'s key, replacing any; refuses it (writing nothing) unless it is
    /// printable ASCII of at most [`MAX_KEY_BYTES`] bytes (see [`refusal`]).
    pub fn store(&self, agent: &AgentId, key: &str) -> Result<(), KeyError> {
        let _writes = writes();
        self.store_unlocked(agent, key)
    }

    /// Stores `key` as `agent`'s key only when none is stored: `Ok(true)` when it was stored,
    /// `Ok(false)` (nothing written) when a key was there. A stored item that is not a valid key
    /// is [`KeyError::StoredInvalid`], and nothing is written; a refused key is refused as
    /// [`Keychain::store`] refuses it.
    ///
    /// Atomic within this process only: it holds the process's write lock from the load to the
    /// store, so no other write here falls between them; the OS stores have no create-only
    /// write, so another process could, and desktop's single-instance plugin keeps a second
    /// Clauseo from running.
    pub fn store_if_absent(&self, agent: &AgentId, key: &str) -> Result<bool, KeyError> {
        let _writes = writes();
        if self.load(agent)?.is_some() {
            return Ok(false);
        }
        self.store_unlocked(agent, key).map(|()| true)
    }

    /// [`Keychain::store`] with the write lock already held.
    fn store_unlocked(&self, agent: &AgentId, key: &str) -> Result<(), KeyError> {
        if let Some(reason) = refusal(key) {
            return Err(KeyError::Invalid {
                agent: agent.clone(),
                reason,
            });
        }
        match &self.backend {
            Backend::None => Err(KeyError::NoKeychain {
                agent: agent.clone(),
            }),
            #[cfg(any(target_os = "macos", windows))]
            Backend::Os => os::set(agent, key.as_bytes()),
            #[cfg(any(test, feature = "test-support"))]
            Backend::Memory(items) => {
                let bytes = Zeroizing::new(key.as_bytes().to_vec());
                lock(items).insert(account(agent), bytes);
                Ok(())
            }
        }
    }

    /// `agent`'s key, or `None` when there is none (always `None` with no keychain). A stored item
    /// that is not a valid key is [`KeyError::StoredInvalid`], never `None`.
    pub fn load(&self, agent: &AgentId) -> Result<Option<Zeroizing<String>>, KeyError> {
        let bytes: Option<Vec<u8>> = match &self.backend {
            Backend::None => None,
            #[cfg(any(target_os = "macos", windows))]
            Backend::Os => os::get(agent)?,
            #[cfg(any(test, feature = "test-support"))]
            Backend::Memory(items) => lock(items).get(&account(agent)).map(|b| b.to_vec()),
        };
        bytes.map(|bytes| into_key(agent, bytes)).transpose()
    }

    /// Removes `agent`'s key; `Ok` when there is none (always `Ok` with no keychain).
    #[cfg_attr(
        not(any(target_os = "macos", windows)),
        expect(
            clippy::unnecessary_wraps,
            reason = "the macOS and Windows keychain can fail; one signature serves every OS"
        )
    )]
    pub fn clear(&self, agent: &AgentId) -> Result<(), KeyError> {
        let _writes = writes();
        match &self.backend {
            Backend::None => {
                // No keychain, so no key of this agent's can exist.
                let _ = agent;
                Ok(())
            }
            #[cfg(any(target_os = "macos", windows))]
            Backend::Os => os::delete(agent),
            #[cfg(any(test, feature = "test-support"))]
            Backend::Memory(items) => {
                lock(items).remove(&account(agent));
                Ok(())
            }
        }
    }
}

/// The lock every keychain write in this process takes, past a poisoning.
fn writes() -> MutexGuard<'static, ()> {
    static WRITES: Mutex<()> = Mutex::new(());
    WRITES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The memory keychain's items: account to bytes.
#[cfg(any(test, feature = "test-support"))]
type Items = BTreeMap<String, Zeroizing<Vec<u8>>>;

/// The memory keychain's items, past a poisoned lock.
#[cfg(any(test, feature = "test-support"))]
fn lock(items: &Mutex<Items>) -> MutexGuard<'_, Items> {
    items.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Why a key is refused, or `None` when it is taken: [`store`] refuses it and [`load`] treats a
/// stored item that fails it as invalid. A key is 1 to [`MAX_KEY_BYTES`] bytes, each printable
/// ASCII (0x21–0x7E): no space, no line break, no control character, no non-ASCII character.
fn refusal(key: &str) -> Option<&'static str> {
    if key.is_empty() {
        Some("it is empty")
    } else if key.len() > MAX_KEY_BYTES {
        Some("it is too long")
    } else if !key.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        Some(
            "it is not printable ASCII (a space, a line break, a control or a non-ASCII character)",
        )
    } else {
        None
    }
}

/// The item's bytes as a key. An item that is not UTF-8 is zeroed and reads as unreadable; one
/// that fails [`refusal`] is zeroed and reads as [`KeyError::StoredInvalid`], which never says
/// what is wrong with it beyond that.
fn into_key(agent: &AgentId, bytes: Vec<u8>) -> Result<Zeroizing<String>, KeyError> {
    let key = match String::from_utf8(bytes) {
        Ok(key) => Zeroizing::new(key),
        Err(err) => {
            drop(Zeroizing::new(err.into_bytes()));
            return Err(KeyError::Keychain {
                agent: agent.clone(),
                kind: "an unreadable item",
            });
        }
    };
    if refusal(&key).is_some() {
        return Err(KeyError::StoredInvalid {
            agent: agent.clone(),
        });
    }
    Ok(key)
}
