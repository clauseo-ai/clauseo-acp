//! The tasks whose owner hardening may have written (`harden.rs`): a pool user may swap in its
//! own task under the name across hardening's write, which then stamps our `O:SY` or `O:BA`
//! owner onto it. So before the write the task's path and `Id` go to the record's
//! `OwnerUntrusted` (`REG_MULTI_SZ` in `HKLM\SOFTWARE\Clauseo\Sandbox`: SYSTEM and
//! Administrators write, Users read), each entry `{Id}\A\name` (no write when that fails).
//! After it the entry goes when the `Id` read is the same, and otherwise stays, with the new
//! `Id`, or [`ANY_ID`] when it couldn't be read.
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
    /// Its `TaskCache\Tree` `Id`, a GUID in braces.
    pub id: String,
}

impl Untrusted {
    /// The entry as the record holds it: `{Id}\A\name`.
    pub fn text(&self) -> String {
        format!("{}{}", self.id, self.path)
    }

    /// The entry the record's `text` holds, `None` when it isn't `{Id}` then a path from `\`.
    pub fn parse(text: &str) -> Option<Untrusted> {
        let (id, path) = text.split_at_checked(text.find('}')? + 1)?;
        if !id.starts_with('{') || !path.starts_with('\\') {
            return None;
        }
        Some(Untrusted {
            path: path.to_owned(),
            id: id.to_owned(),
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
/// [`ANY_ID`]; an `Id` that can't be read holds it too, the owner staying in doubt.
fn holds(one: &Untrusted, id: &Result<Option<String>, Failed>) -> bool {
    match id {
        Ok(Some(id)) => one.id == ANY_ID || *id == one.id,
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

/// Sets the entry for `path` to `id`, or takes it off when `id` is `None`.
pub(super) fn set<S: Sweeper>(
    fs: &mut S,
    lock: bool,
    path: &str,
    id: Option<&str>,
) -> Result<(), Failed> {
    update(fs, lock, |_, list| {
        list.retain(|one| one.path != path);
        if let Some(id) = id {
            list.push(Untrusted {
                path: path.to_owned(),
                id: id.to_owned(),
            });
        }
    })
}
