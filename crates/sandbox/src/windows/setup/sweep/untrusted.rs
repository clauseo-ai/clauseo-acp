//! The tasks whose owner hardening may have written (`harden.rs`): when a task's
//! `TaskCache\Tree` `Id` changed after hardening wrote its descriptor, a pool user may have
//! swapped in its own task under the name, and the write stamped our `O:SY` or `O:BA` owner
//! onto it. Its path and its new `Id` go to the record's `OwnerUntrusted` (`REG_MULTI_SZ` in
//! `HKLM\SOFTWARE\Clauseo\Sandbox`: SYSTEM and Administrators write, Users read), each entry
//! `{Id}\A\name`.
//!
//! A task listed there whose `Id` still matches has its owner ignored ([`distrusts`]): it is
//! judged by its principal and its file alone, deleted when they name the pool, and 56
//! otherwise, since only its owner could have passed it. An entry clears once its task is gone
//! or its `Id` no longer matches ([`still_listed`]). A list that can't be read is 56, every
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

/// Whether the task at `path` is one of `list` whose `Id` still matches, so its owner is
/// ignored. An `Id` that can't be read counts as a match: the owner stays in doubt.
pub(super) fn distrusts<S: Sweeper>(fs: &mut S, list: &[Untrusted], path: &str) -> bool {
    for one in list {
        let same = one.path == path || fs.same_task_path(&one.path, path).unwrap_or(true);
        if same
            && fs
                .task_cache_id(path)
                .map_or(true, |id| id.as_ref() == Some(&one.id))
        {
            return true;
        }
    }
    false
}

/// Those of `list` still to keep: each whose task is there with the same `Id`, or whose `Id`
/// can't be read; one gone, or with another `Id`, clears.
pub(super) fn still_listed<S: Sweeper>(fs: &mut S, list: &[Untrusted]) -> Vec<Untrusted> {
    let mut kept = Vec::new();
    for one in list {
        if fs
            .task_cache_id(&one.path)
            .map_or(true, |id| id.as_ref() == Some(&one.id))
        {
            kept.push(one.clone());
        }
    }
    kept
}

/// `list` with `added` (path and the `Id` read after the write, `None` where it couldn't be),
/// each `None` read once more; one still unread, or gone, isn't listed. Duplicates go.
pub(super) fn with_added<S: Sweeper>(
    fs: &mut S,
    mut list: Vec<Untrusted>,
    added: &[(String, Option<String>)],
) -> Vec<Untrusted> {
    for (path, id) in added {
        let id = match id {
            Some(id) => Some(id.clone()),
            None => fs.task_cache_id(path).ok().flatten(),
        };
        if let Some(id) = id {
            let one = Untrusted {
                path: path.clone(),
                id,
            };
            if !list.contains(&one) {
                list.push(one);
            }
        }
    }
    list
}

/// Writes `list` as the record's `OwnerUntrusted` when it differs from `read`, under the
/// set-up lock when `lock` (the sweep; set-up holds it already).
pub(super) fn store<S: Sweeper>(
    fs: &mut S,
    read: &[Untrusted],
    list: &[Untrusted],
    lock: bool,
) -> Result<(), Failed> {
    if read == list {
        return Ok(());
    }
    let texts: Vec<String> = list.iter().map(Untrusted::text).collect();
    if !lock {
        return fs.set_owner_untrusted(&texts);
    }
    if !fs.lock()? {
        return Err("the set-up lock is held".to_owned());
    }
    let written = fs.set_owner_untrusted(&texts);
    fs.unlock();
    written
}
