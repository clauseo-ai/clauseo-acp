//! A matter's kept rules file (`always.rs` has why and where): read at each Claude launch and
//! for the lawyer's list, a rule added on the lawyer's answer, one removed from the list. Every
//! read and write holds one lock, so two of them never lose a rule. The lock is per process:
//! Clauseo runs one instance (`tauri_plugin_single_instance`, `crates/desktop/src/lib.rs`).
//!
//! An add and a removal carry the sequence number the actor gave them (`actor/kept.rs`
//! `Ledger`), and run on threads of their own, so they can reach the lock out of order. Under it,
//! an operation on a rule that one the actor gave a later number already applied to is skipped:
//! an add whose write was still pending when the lawyer removed the rule never puts it back.
//!
//! A file this host cannot take (not JSON it reads, larger than [`MAX_FILE_LEN`], unreadable,
//! or without a valid MAC) is never written over: the next write sets it aside first, as
//! `claude-allow.json.bad-<n>`, and starts empty. Until then it reads as no rules.
//!
//! Version 2 (decision record 0063): the file carries an HMAC-SHA256 ([`mac`]) over its matter's
//! id (the folder it sits in, `<data>/agent-state/<matter>/`) and every rule, keyed by 32 random
//! bytes in the OS secret store (account [`KEY_ACCOUNT`], [`use_secrets`]). A file edited by
//! hand, planted, copied from another matter, or of version 1 holds nothing. With no secret store
//! (Linux, a refused Keychain prompt, any error) nothing is read or kept: [`read_beside`] says so,
//! and the card offers no "Always".

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use clauseo_connectors::SecretStore;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use super::{MAX_RULES, file_beside, well_formed};

/// The largest file read.
const MAX_FILE_LEN: u64 = 1 << 20;

/// The most set-aside names tried.
const MAX_ASIDE: u32 = 100;

/// The one lock of every read and write of a rules file, over each file's rules' latest
/// applied sequence number.
static LOCK: Mutex<Applied> = Mutex::new(BTreeMap::new());

/// For each rules file and rule, the sequence number of the last add or removal applied to it.
type Applied = BTreeMap<(PathBuf, String), u64>;

/// The last sequence number given, process-wide like [`LOCK`]'s map: a second host in the
/// process (a test harness, a restart in process) numbers above the first, so none of its
/// operations is skipped for one the first host applied.
static SEQ: AtomicU64 = AtomicU64::new(0);

/// The secret store's account of the key.
pub(crate) const KEY_ACCOUNT: &str = "always-allowed-hmac";

/// The file version this host writes and reads.
const VERSION: u64 = 2;

/// The secret store the key lives in, process-wide (one Clauseo per machine); none: nothing is
/// kept.
static SECRETS: RwLock<Option<Arc<dyn SecretStore>>> = RwLock::new(None);

/// The key, once read or made.
static KEY: Mutex<Option<[u8; 32]>> = Mutex::new(None);

/// The secret store the key is kept in, from the host's start (the desktop's Keychain or
/// Credential Manager; `MemoryStore` in tests); none keeps nothing.
pub(crate) fn use_secrets(secrets: Option<Arc<dyn SecretStore>>) {
    *SECRETS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = secrets;
    *KEY.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// The key: read from the secret store, or made (32 random bytes) and stored there the first
/// time; none without a store or on any error. Calls the secret store: off the actor.
fn key() -> Option<[u8; 32]> {
    let mut cached = KEY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(key) = *cached {
        return Some(key);
    }
    let store = SECRETS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()?;
    let stored = match store.get(KEY_ACCOUNT) {
        Ok(stored) => stored,
        Err(error) => {
            tracing::warn!(%error, "the kept-permissions key is unreadable; nothing is kept");
            return None;
        }
    };
    let key = match stored {
        // A key of another length was never written by this host: it is not replaced, which
        // would void every matter's rules, and nothing is kept until it is dealt with.
        Some(bytes) => match <[u8; 32]>::try_from(bytes.as_slice()) {
            Ok(key) => key,
            Err(_) => {
                tracing::error!(
                    len = bytes.len(),
                    "the kept-permissions key is malformed; nothing is kept or offered"
                );
                return None;
            }
        },
        None => new_key(&*store)?,
    };
    *cached = Some(key);
    Some(key)
}

/// A new key, 32 random bytes, stored in `store`; none when either fails.
fn new_key(store: &dyn SecretStore) -> Option<[u8; 32]> {
    let mut key = [0_u8; 32];
    getrandom::fill(&mut key).ok()?;
    if let Err(error) = store.set(KEY_ACCOUNT, &key) {
        tracing::warn!(%error, "the kept-permissions key was not stored; nothing is kept");
        return None;
    }
    Some(key)
}

/// HMAC-SHA256 (RFC 2104) of `message` under `key`.
fn hmac(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut inner = Sha256::new();
    let mut outer = Sha256::new();
    let mut pad = [0_u8; 64];
    for (at, byte) in pad.iter_mut().enumerate() {
        *byte = key.get(at).copied().unwrap_or(0);
    }
    inner.update(pad.map(|byte| byte ^ 0x36));
    inner.update(message);
    outer.update(pad.map(|byte| byte ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// The MAC of `rules` kept for the matter whose folder holds `file`, as lower-case hex: over
/// `"v2\0" + matter id + "\0" + rules joined by "\0"`. None when `file` sits in no folder.
fn mac(key: &[u8; 32], file: &Path, rules: &[String]) -> Option<String> {
    let matter = file.parent()?.file_name()?.to_str()?;
    let mut message = format!("v{VERSION}\0{matter}\0").into_bytes();
    message.extend_from_slice(rules.join("\0").as_bytes());
    Some(
        hmac(key, &message)
            .iter()
            .fold(String::new(), |mut hex, byte| {
                use std::fmt::Write as _;
                let _infallible = write!(hex, "{byte:02x}");
                hex
            }),
    )
}

/// Whether two texts are equal, in time that depends on their length only.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0_u8, |diff, (x, y)| diff | (x ^ y))
            == 0
}

/// A new sequence number, above every one given before in this process.
pub(crate) fn next_seq() -> u64 {
    SEQ.fetch_add(1, Ordering::Relaxed).saturating_add(1)
}

/// What a rules file holds.
enum Kept {
    /// No file.
    Missing,
    /// Its rules, each [`well_formed`], at most [`MAX_RULES`].
    Rules(Vec<String>),
    /// A file this host cannot take: it is never written over.
    Bad,
}

impl Kept {
    fn rules(self) -> Vec<String> {
        match self {
            Self::Rules(rules) => rules,
            Self::Missing | Self::Bad => Vec::new(),
        }
    }
}

fn locked() -> std::sync::MutexGuard<'static, Applied> {
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether an operation numbered `seq` on `rule` in `file` comes too late: one with a later
/// number already applied. When not, it is recorded as the latest, before it runs.
fn superseded(applied: &mut Applied, file: &Path, rule: &str, seq: u64) -> bool {
    let latest = applied
        .entry((file.to_owned(), rule.to_owned()))
        .or_insert(seq);
    if *latest > seq {
        return true;
    }
    *latest = seq;
    false
}

/// A launch's kept rules, read from [`file_beside`] `notes`, for either agent; none when there is
/// no key, so nothing can be kept either. File system and secret store calls: off the actor.
pub(crate) fn read_beside(notes: &Path) -> Option<Vec<String>> {
    key()?;
    Some(
        file_beside(notes)
            .map(|file| read(&file))
            .unwrap_or_default(),
    )
}

/// The rules kept in `file`; none when it is missing, a link, or one this host cannot take.
/// File system calls: off the actor.
pub(crate) fn read(file: &Path) -> Vec<String> {
    let _held = locked();
    read_unlocked(file).rules()
}

/// The rules kept in `file`, for the lawyer's list: [`read`].
pub(crate) fn list(file: &Path) -> Vec<String> {
    read(file)
}

fn read_unlocked(file: &Path) -> Kept {
    let opened = match crate::write::open_read_nofollow(file) {
        Ok(opened) => opened,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Kept::Missing,
        Err(_) => {
            tracing::warn!("a matter's kept rules are unreadable");
            return Kept::Bad;
        }
    };
    let mut bytes = Vec::new();
    let read = opened.take(MAX_FILE_LEN + 1).read_to_end(&mut bytes);
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if read.is_err() || len > MAX_FILE_LEN {
        tracing::warn!("a matter's kept rules are unreadable or too large");
        return Kept::Bad;
    }
    let Some(key) = key() else {
        return Kept::Bad;
    };
    let value = serde_json::from_slice::<Value>(&bytes).ok();
    let parsed = value.as_ref().and_then(|value| {
        let version = value.get("version")?.as_u64()?;
        let rules: Vec<String> = serde_json::from_value(value.get("allow")?.clone()).ok()?;
        let stored = value.get("mac")?.as_str()?;
        let made = mac(&key, file, &rules)?;
        (version == VERSION && same(stored, &made)).then_some(rules)
    });
    let Some(rules) = parsed else {
        tracing::warn!("a matter's kept rules are not a file this host wrote for this matter");
        return Kept::Bad;
    };
    let mut kept: Vec<String> = Vec::new();
    for rule in rules {
        if well_formed(&rule) && !kept.contains(&rule) && kept.len() < MAX_RULES {
            kept.push(rule);
        }
    }
    Kept::Rules(kept)
}

/// Adds `rule` to `file` on a thread of its own, as operation `seq`: read, merged, then
/// replaced atomically through no link. A failure is a warning that names no rule (a command may
/// hold a client's name).
pub(crate) fn store(file: PathBuf, rule: String, seq: u64) {
    let spawned = std::thread::Builder::new()
        .name("clauseo-always".to_owned())
        .spawn(move || {
            if let Err(error) = store_now(&file, rule, seq) {
                tracing::warn!(%error, "a matter's rule was not kept");
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "a matter's rule was not kept");
    }
}

/// [`store`]'s work, on the calling thread; nothing when a later operation on `rule` applied,
/// or the matter was forgotten after the lawyer's answer ([`delete`]).
pub(crate) fn store_now(file: &Path, rule: String, seq: u64) -> std::io::Result<()> {
    let mut held = locked();
    if forgotten_after(&held, file, seq) || superseded(&mut held, file, &rule, seq) {
        return Ok(());
    }
    if key().is_none() {
        return Err(std::io::Error::other("no key to keep the rule with"));
    }
    let mut rules = match read_unlocked(file) {
        Kept::Bad => {
            set_aside(file)?;
            Vec::new()
        }
        kept @ (Kept::Missing | Kept::Rules(_)) => kept.rules(),
    };
    if rules.contains(&rule) || rules.len() >= MAX_RULES || !well_formed(&rule) {
        return Ok(());
    }
    rules.push(rule);
    write_unlocked(file, &rules)
}

/// Removes `rule` from `file` as operation `seq`: whether it was there. A file this host cannot
/// take is left as it is (it holds no rule this host applies), and so is a rule a later add put
/// back. File system calls: off the actor.
pub(crate) fn remove_now(file: &Path, rule: &str, seq: u64) -> std::io::Result<bool> {
    let mut held = locked();
    if superseded(&mut held, file, rule, seq) {
        return Ok(false);
    }
    let Kept::Rules(mut rules) = read_unlocked(file) else {
        return Ok(false);
    };
    let before = rules.len();
    rules.retain(|kept| kept != rule);
    if rules.len() == before {
        return Ok(false);
    }
    write_unlocked(file, &rules)?;
    Ok(true)
}

/// The [`Applied`] key under which a forgotten matter's file records its deletion's number:
/// no rule is empty (`well_formed`).
const FORGOTTEN: &str = "";

/// Whether `file`'s matter was forgotten by an operation numbered after `seq`.
fn forgotten_after(applied: &Applied, file: &Path, seq: u64) -> bool {
    applied
        .get(&(file.to_owned(), FORGOTTEN.to_owned()))
        .is_some_and(|forgotten| *forgotten > seq)
}

/// Deletes a forgotten matter's `file` and any of it set aside, as operation `seq`, on a
/// thread of its own, under the lock: nothing the lawyer kept for it outlives it (decision
/// record 0063), not even an add answered before the forget whose write was still pending, which
/// is skipped. A missing file is fine; a failure is a warning.
pub(crate) fn delete(file: PathBuf, seq: u64) {
    let spawned = std::thread::Builder::new()
        .name("clauseo-always".to_owned())
        .spawn(move || {
            if let Err(error) = delete_now(&file, seq) {
                tracing::warn!(%error, "a forgotten matter's kept rules were not deleted");
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "a forgotten matter's kept rules were not deleted");
    }
}

/// [`delete`]'s work, on the calling thread.
pub(crate) fn delete_now(file: &Path, seq: u64) -> std::io::Result<()> {
    let mut held = locked();
    let latest = held
        .entry((file.to_owned(), FORGOTTEN.to_owned()))
        .or_insert(seq);
    *latest = (*latest).max(seq);
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    for n in 1..=MAX_ASIDE {
        crate::write::remove_old_file(&file.with_file_name(format!("{name}.bad-{n}")))?;
    }
    crate::write::remove_old_file(file)
}

fn write_unlocked(file: &Path, rules: &[String]) -> std::io::Result<()> {
    let key = key().ok_or_else(|| std::io::Error::other("no key to keep the rules with"))?;
    let mac = mac(&key, file, rules)
        .ok_or_else(|| std::io::Error::other("the rules file sits in no matter folder"))?;
    let text = serde_json::json!({ "version": VERSION, "allow": rules, "mac": mac });
    let mut bytes = serde_json::to_vec_pretty(&text).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    crate::write::replace_private_file(file, &bytes)?;
    match file.parent() {
        Some(folder) => crate::write::sync_dir(folder),
        None => Ok(()),
    }
}

/// Moves a file this host cannot take to the first free `<name>.bad-<n>` beside it, never
/// replacing one; the write that follows fails when none is free.
fn set_aside(file: &Path) -> std::io::Result<()> {
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    for n in 1..=MAX_ASIDE {
        let aside = file.with_file_name(format!("{name}.bad-{n}"));
        match crate::write::move_file_no_replace(file, &aside) {
            Ok(()) => {
                tracing::warn!(n, "a matter's kept rules file was set aside; starts empty");
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::other(
        "no free name to set the rules file aside",
    ))
}
