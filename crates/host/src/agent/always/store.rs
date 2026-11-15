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
//! A file this host cannot take (not JSON it reads, larger than [`MAX_FILE_LEN`], unreadable) is
//! never written over: the next write sets it aside first, as `claude-allow.json.bad-<n>`, and
//! starts empty. Until then it reads as no rules.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

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

/// A Claude launch's kept rules (`claude`), read from [`file_beside`] `notes`; none for any
/// other agent. File system calls: off the actor.
pub(crate) fn read_beside(notes: &Path, claude: bool) -> Vec<String> {
    match file_beside(notes) {
        Some(file) if claude => read(&file),
        _ => Vec::new(),
    }
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
            tracing::warn!("a matter's kept Claude rules are unreadable");
            return Kept::Bad;
        }
    };
    let mut bytes = Vec::new();
    let read = opened.take(MAX_FILE_LEN + 1).read_to_end(&mut bytes);
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if read.is_err() || len > MAX_FILE_LEN {
        tracing::warn!("a matter's kept Claude rules are unreadable or too large");
        return Kept::Bad;
    }
    let parsed: Option<Vec<String>> = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| serde_json::from_value(value.get("allow")?.clone()).ok());
    let Some(rules) = parsed else {
        tracing::warn!("a matter's kept Claude rules are not JSON this host reads");
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
                tracing::warn!(%error, "a matter's Claude rule was not kept");
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "a matter's Claude rule was not kept");
    }
}

/// [`store`]'s work, on the calling thread; nothing when a later operation on `rule` applied.
pub(crate) fn store_now(file: &Path, rule: String, seq: u64) -> std::io::Result<()> {
    let mut held = locked();
    if superseded(&mut held, file, &rule, seq) {
        return Ok(());
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

fn write_unlocked(file: &Path, rules: &[String]) -> std::io::Result<()> {
    let text = serde_json::json!({ "version": 1, "allow": rules });
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
                tracing::warn!(
                    n,
                    "a matter's kept Claude rules file was set aside; starts empty"
                );
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
