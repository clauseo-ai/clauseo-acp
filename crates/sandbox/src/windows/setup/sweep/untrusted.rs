//! The tasks whose owner hardening may have written (`harden.rs`): a pool user may swap in its
//! own task under the name across hardening's write, which then stamps our `O:SY` or `O:BA`
//! owner onto it. So before the write the task's path and `Id` go to the record's
//! `OwnerUntrusted` (`REG_MULTI_SZ` in `HKLM\SOFTWARE\Clauseo\Sandbox`: SYSTEM and
//! Administrators write, Users read), pending: `P{Id}\A\name`, the `Id` read before the
//! write (no write when that fails). After it the entry goes when the `Id` read is the same,
//! and otherwise is settled: `{Id}\A\name` with the new `Id`, or [`ANY_ID`] when it
//! couldn't be read.
//!
//! A pending entry left by a crash or a failed clear settles itself on the next sweep
//! ([`settle`]): the same `Id` proves the same task (the Task Scheduler assigns `Id`s, and a
//! deleted task can't come back with its old one), so it clears and the task is judged as
//! usual; another `Id`, or one that can't be read, settles it as above; a task gone clears it.
//!
//! A task listed there whose `Id` still matches has its owner ignored ([`distrusts`]): it is
//! judged by its principal and its file alone, deleted when they name the pool, and 56
//! otherwise, since only its owner could have passed it. An entry clears once its task is gone
//! or its `Id` no longer matches ([`clear_gone`]). A list that can't be read is 56, every
//! owner then in doubt.

use super::{Failed, Sweeper};

/// One task whose owner hardening may have written: its path (`\A\name`) and its `Id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untrusted {
    /// The task's path.
    pub path: String,
    /// Its `TaskCache\Tree` `Id`, a GUID in braces, or [`ANY_ID`].
    pub id: String,
    /// Written ahead of hardening's write and not yet settled: `id` is the one read before it.
    pub pending: bool,
}

impl Untrusted {
    /// The entry as the record holds it: `{Id}\A\name`, or `P{Id}\A\name` when pending.
    pub fn text(&self) -> String {
        let pending = if self.pending { "P" } else { "" };
        format!("{pending}{}{}", self.id, self.path)
    }

    /// The entry the record's `text` holds, `None` when it isn't `{Id}` then a path from `\`.
    pub fn parse(text: &str) -> Option<Untrusted> {
        let (pending, text) = match text.strip_prefix('P') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (id, path) = text.split_at_checked(text.find('}')? + 1)?;
        if !id.starts_with('{') || !path.starts_with('\\') {
            return None;
        }
        Some(Untrusted {
            path: path.to_owned(),
            id: id.to_owned(),
            pending,
        })
    }
}

/// The record's list, as [`Sweeper::owner_untrusted`] reads it; `Err` when it can't be read or
/// an entry is malformed, never a shorter list.
pub(super) fn read<S: Sweeper>(fs: &mut S) -> Result<Vec<Untrusted>, Failed> {
    fs.owner_untrusted()?
        .iter()
        .map(|text| {
            Untrusted::parse(text)
                .ok_or_else(|| "an entry of the record's OwnerUntrusted is malformed".to_owned())
        })
        .collect()
}

/// The `Id` an entry holds when the one after hardening's write couldn't be read: it matches
/// any `Id`, so the entry holds until its task is gone.
pub const ANY_ID: &str = "{*}";

/// Whether the entry `one` holds for the `Id` read, `id`: the same `Id`, or any for
/// [`ANY_ID`]; an `Id` that can't be read holds it too, the owner staying in doubt. A pending
/// one holds while its task is there, for [`settle`] to settle.
fn holds(one: &Untrusted, id: &Result<Option<String>, Failed>) -> bool {
    match id {
        Ok(Some(id)) => one.pending || one.id == ANY_ID || *id == one.id,
        Ok(None) => false,
        Err(_) => true,
    }
}

/// Whether the task at `path` is one of `list` whose `Id` still matches, so its owner is
/// ignored.
pub(super) fn distrusts<S: Sweeper>(fs: &mut S, list: &[Untrusted], path: &str) -> bool {
    for one in list {
        let same = one.path == path || fs.same_task_path(&one.path, path).unwrap_or(true);
        if same && holds(one, &fs.task_cache_id(path)) {
            return true;
        }
    }
    false
}

/// Changes the record's list by `change`, written only when it changed, under the set-up lock
/// when `lock` (the sweep; set-up holds it already).
pub(super) fn update<S: Sweeper>(
    fs: &mut S,
    lock: bool,
    change: impl FnOnce(&mut S, &mut Vec<Untrusted>),
) -> Result<(), Failed> {
    if lock && !fs.lock()? {
        return Err("the set-up lock is held".to_owned());
    }
    let done = read(fs).and_then(|read| {
        let mut list = read.clone();
        change(fs, &mut list);
        if list == read {
            return Ok(());
        }
        let texts: Vec<String> = list.iter().map(Untrusted::text).collect();
        fs.set_owner_untrusted(&texts)
    });
    if lock {
        fs.unlock();
    }
    done
}

/// Clears each entry whose task is gone or has another `Id` (the sweep's end).
pub(super) fn clear_gone<S: Sweeper>(fs: &mut S) -> Result<(), Failed> {
    // Read first without the lock: most runs have nothing to clear.
    let listed = read(fs)?;
    if listed
        .iter()
        .all(|one| holds(one, &fs.task_cache_id(&one.path)))
    {
        return Ok(());
    }
    update(fs, true, |fs, list| {
        list.retain(|one| holds(one, &fs.task_cache_id(&one.path)));
    })
}

/// Sets the entry for `path` to `id`, pending when `pending`, or takes it off when `id` is
/// `None`.
pub(super) fn set<S: Sweeper>(
    fs: &mut S,
    lock: bool,
    path: &str,
    id: Option<&str>,
    pending: bool,
) -> Result<(), Failed> {
    update(fs, lock, |_, list| {
        list.retain(|one| one.path != path);
        if let Some(id) = id {
            list.push(Untrusted {
                path: path.to_owned(),
                id: id.to_owned(),
                pending,
            });
        }
    })
}

/// `list` with each pending entry settled, as the module says: cleared for the same `Id` or a
/// task gone, else settled with the `Id` read, or [`ANY_ID`] when it can't be read.
fn settled<S: Sweeper>(fs: &mut S, list: &[Untrusted]) -> Vec<Untrusted> {
    let mut out = Vec::new();
    for one in list {
        if !one.pending {
            out.push(one.clone());
            continue;
        }
        let id = match fs.task_cache_id(&one.path) {
            Ok(None) => continue,
            Ok(Some(id)) if id == one.id => continue,
            Ok(Some(id)) => id,
            Err(_) => ANY_ID.to_owned(),
        };
        out.push(Untrusted {
            path: one.path.clone(),
            id,
            pending: false,
        });
    }
    out
}

/// `list`, as read at the walk's start, with its pending entries settled (the record written
/// under the set-up lock when one changes): what the walk holds tasks to, and whether the
/// record could be written. One not written settles again on the next sweep.
pub(super) fn settle<S: Sweeper>(
    fs: &mut S,
    list: Vec<Untrusted>,
) -> (Vec<Untrusted>, Result<(), Failed>) {
    if !list.iter().any(|one| one.pending) {
        return (list, Ok(()));
    }
    let mut written = None;
    let stored = update(fs, true, |fs, now| {
        *now = settled(fs, now);
        written = Some(now.clone());
    });
    let settled = written.unwrap_or_else(|| settled(fs, &list));
    (settled, stored)
}
