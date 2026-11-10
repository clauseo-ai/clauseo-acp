//! The watch task's removal (`refresh::unregister_task`, which `--remove` and the uninstaller's
//! teardown call elevated), with no Windows call: [`remove_watch`] holds the order, and
//! [`WatchParts`] the effects, which `refresh/task.rs` implements and the tests fake.
//!
//! 1. the task is disabled and a running watcher ended, so nothing starts or serves again;
//! 2. the trace session is stopped by name;
//! 3. the published name ([`super::protocol::WATCH_KEY`]) is deleted. It is opened as itself,
//!    never through a registry link (`REG_OPTION_OPEN_LINK`), and looked at first:
//!    - a link is refused, untouched;
//!    - a key whose owner isn't SYSTEM, or can't be read, is refused, untouched: the watcher
//!      never makes one (it refuses one too, 74), so it is left for an administrator to check;
//!    - SYSTEM's key is deleted. A watcher that was ended (or killed) leaves it, and
//!      administrators may only read it ([`super::protocol::KEY_SDDL`]): a delete refused for
//!      that is followed by taking the key for Administrators (`SeTakeOwnershipPrivilege`, as
//!      `--reclaim` takes a file), who may then delete it;
//! 4. the task is deleted, with `\Clauseo` if nothing else is in it.
//!
//! Each step runs whatever an earlier one gave, so the task never outlives a failure; the
//! first failure is the result. Each finds what is gone already as done, so a second run
//! finishes whatever the first left: a key without a task, a task without a key, a session.

/// What the key is, opened as itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchKey {
    /// There is none.
    Absent,
    /// A registry link (its `SymbolicLinkValue` is `REG_LINK`).
    Link,
    /// A key SYSTEM owns, as the watcher makes it.
    System,
    /// A key anyone else owns.
    Other,
}

/// What a plain delete of the key gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDelete {
    /// It is gone: deleted, or it wasn't there.
    Gone,
    /// Refused (`ERROR_ACCESS_DENIED`): SYSTEM owns it and administrators may only read it.
    Denied,
}

/// Why the key was left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyLeft {
    /// It is a registry link.
    Link,
    /// Its owner isn't SYSTEM.
    NotSystems,
    /// Its delete was still refused once it was taken.
    StillDenied,
}

impl KeyLeft {
    /// A line for the log.
    pub fn line(self) -> &'static str {
        match self {
            KeyLeft::Link => "it is a registry link, so it is left alone",
            KeyLeft::NotSystems => "its owner isn't SYSTEM, so it is left for an administrator",
            KeyLeft::StillDenied => "its delete was refused after it was taken",
        }
    }
}

/// The removal's effects. A task, session or key that isn't there is `Ok`.
pub trait WatchParts {
    /// Why an effect failed.
    type Error;
    /// Disables the task and ends a running instance.
    fn end_task(&mut self) -> Result<(), Self::Error>;
    /// Stops the trace session by name.
    fn stop_session(&mut self) -> Result<(), Self::Error>;
    /// What the key is; `Err` when its owner can't be read.
    fn inspect_key(&mut self) -> Result<WatchKey, Self::Error>;
    /// Deletes the key itself, as this process's token may.
    fn delete_key(&mut self) -> Result<KeyDelete, Self::Error>;
    /// Makes Administrators the key's owner, then gives them full control of it.
    fn take_key(&mut self) -> Result<(), Self::Error>;
    /// The failure of a key left for `why`.
    fn left(&self, why: KeyLeft) -> Self::Error;
    /// Deletes the task, then `\Clauseo` if it is empty.
    fn delete_task(&mut self) -> Result<(), Self::Error>;
}

/// Removes the watch task, its session and its key, as the module says.
pub fn remove_watch<P: WatchParts>(parts: &mut P) -> Result<(), P::Error> {
    let ended = parts.end_task();
    let stopped = parts.stop_session();
    let key = remove_key(parts);
    let deleted = parts.delete_task();
    ended.and(stopped).and(key).and(deleted)
}

/// Step 3: the key looked at, deleted, and taken first when it is SYSTEM's and the delete is
/// refused.
fn remove_key<P: WatchParts>(parts: &mut P) -> Result<(), P::Error> {
    match parts.inspect_key()? {
        WatchKey::Absent => return Ok(()),
        WatchKey::Link => return Err(parts.left(KeyLeft::Link)),
        WatchKey::Other => return Err(parts.left(KeyLeft::NotSystems)),
        WatchKey::System => {}
    }
    if parts.delete_key()? == KeyDelete::Gone {
        return Ok(());
    }
    parts.take_key()?;
    match parts.delete_key()? {
        KeyDelete::Gone => Ok(()),
        // Taken, so the token may delete it: a refusal now is a failure, not a loop.
        KeyDelete::Denied => Err(parts.left(KeyLeft::StillDenied)),
    }
}
