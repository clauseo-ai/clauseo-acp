//! A staged file renamed over its target ([`Staged::rename`]), or written in place where the
//! rename is refused (`Staged::rename_or_in_place`, macOS); on Windows a refused rename is
//! tried again after a short wait first ([`again_while_denied`]).

#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
use std::fs::File;
use std::io;

#[cfg(any(windows, test))]
use super::replace_keeping;
use super::{Failed, Lost, ReplaceFn, Staged, plain_rename};
#[cfg(test)]
use super::{injected, refusing};
use crate::{Error, fsync};

/// Why [`Staged::rename`] failed.
#[derive(Debug)]
pub(crate) struct RenameFailed {
    pub(crate) error: Error,
    /// The target is not there any more: the folder changed. The temp file, which holds the new
    /// content, is kept (Windows, after a failed `ReplaceFileW` that moved the old file away);
    /// or a write in place failed part way and was not put back ([`Self::torn`]).
    pub(crate) changed: bool,
    /// The write in place was refused, and why: the target is as it was.
    pub(crate) refused: Option<crate::RefusedBecause>,
    /// The write in place failed after it began (`Staged::rename_or_in_place`): whether the
    /// version from before was put back, so the target is as it was. `error` is the write's.
    pub(crate) torn: Option<bool>,
}

/// A staged file renamed into place ([`Staged::rename`]).
#[derive(Debug)]
pub(crate) struct Renamed {
    /// The target's folder, which the caller syncs.
    pub(crate) dir: std::path::PathBuf,
    /// What of the old file's metadata the new one did not keep; the caller logs the total once.
    pub(crate) lost: Lost,
    /// The step after the rename failed (macOS: the old file's ACL and mode not set): the file
    /// stays readable and writable by its owner alone, and the caller names it.
    pub(crate) owner_only: bool,
}

/// The history's copy of a file a write in place overwrites ([`Staged::rename`]'s
/// `rename_or_in_place`): the safety net of that write, which is not atomic. Its methods exist
/// where the write in place does (macOS; Linux in unit tests).
pub(crate) trait SavedCopy {
    /// Whether the open target `file`, read from its position (the start), holds the version
    /// the history holds, and the history's copy reads back whole.
    #[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
    fn confirm(&mut self, file: &mut File) -> bool;

    /// After a write in place that [`Self::confirm`] allowed failed part way: writes that
    /// confirmed version to `file` at its position (the start); returns its length.
    #[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
    fn write_back(&mut self, file: &mut File) -> io::Result<u64>;
}

impl Staged {
    /// Renames the temp file over the target, then syncs the folder. Never truncates the
    /// target in place.
    pub(crate) fn commit(self) -> Result<(), Error> {
        let Renamed { dir, .. } = self.rename().map_err(|failed| failed.error)?;
        fsync::sync_dir(&dir).map_err(|err| Error::io("sync", &dir, err))
    }

    /// Renames the temp file over the target without syncing the folder; returns the folder,
    /// which the caller syncs (once for many renames in it) before relying on the rename. A
    /// lawyer's file on Windows goes in by `ReplaceFileW` ([`replace_keeping`]).
    pub(crate) fn rename(self) -> Result<Renamed, RenameFailed> {
        #[cfg(windows)]
        let replace: Option<&ReplaceFn> = Some(&super::windows::replace_file);
        #[cfg(not(windows))]
        let replace: Option<&ReplaceFn> = None;
        self.rename_with(replace, None)
    }

    /// [`Self::rename`]; a lawyer's file the rename may not replace (macOS: its ACL denies
    /// `delete`) is written in place instead, once `saved` confirms the history holds what it
    /// has now, and put back by `saved` if that write fails part way (`super::in_place`).
    #[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
    pub(crate) fn rename_or_in_place(
        self,
        saved: &mut dyn SavedCopy,
    ) -> Result<Renamed, RenameFailed> {
        #[cfg(windows)]
        let replace: Option<&ReplaceFn> = Some(&super::windows::replace_file);
        #[cfg(not(windows))]
        let replace: Option<&ReplaceFn> = None;
        self.rename_with(replace, Some(saved))
    }

    /// [`Self::rename`], with `replace` (`ReplaceFileW`, or a simulation of it in tests) for a
    /// lawyer's file, or the plain rename. On Windows a rename refused with an access denied is
    /// tried again after each short wait ([`again_while_denied`]) before it fails.
    #[cfg_attr(
        not(any(target_os = "macos", all(test, target_os = "linux"))),
        expect(
            clippy::needless_pass_by_value,
            reason = "saved is taken where in place runs"
        )
    )]
    pub(super) fn rename_with(
        mut self,
        replace: Option<&ReplaceFn>,
        saved: Option<&mut dyn SavedCopy>,
    ) -> Result<Renamed, RenameFailed> {
        let dir = self.target.parent().unwrap_or(&self.target).to_owned();
        let Some(temp) = self.temp.take() else {
            return Ok(Renamed {
                dir,
                lost: self.lost,
                owner_only: false,
            });
        };
        #[cfg(test)]
        if let Err(err) = injected("rename") {
            self.temp = Some(temp);
            return Err(RenameFailed {
                error: Error::io("rename", &self.target, err),
                changed: false,
                refused: None,
                torn: None,
            });
        }
        let once = || -> Result<Lost, Failed> {
            #[cfg(any(windows, test))]
            let renamed = match replace {
                #[cfg(test)]
                _ if self.keep && refusing() => {
                    Err(Failed::Unchanged(io::ErrorKind::PermissionDenied.into()))
                }
                Some(replace) if self.keep => {
                    replace_keeping::replace_keeping(&temp, &self.target, replace)
                }
                _ => plain_rename(&temp, &self.target),
            };
            #[cfg(not(any(windows, test)))]
            let renamed = {
                // Only Windows keeps metadata at the rename; elsewhere `stage` copied it.
                let _ = (replace, self.keep);
                plain_rename(&temp, &self.target)
            };
            renamed
        };
        let renamed = again_while_denied(once, |failed| match failed {
            #[cfg(any(windows, test))]
            Failed::TargetGone(_) => false,
            #[cfg(any(windows, test))]
            Failed::Unchanged(err) => err.kind() == io::ErrorKind::PermissionDenied,
            #[cfg(not(any(windows, test)))]
            Failed(err) => err.kind() == io::ErrorKind::PermissionDenied,
        });
        match renamed {
            Ok(lost) => {
                self.lost.add(lost);
                let unfinished = finish(&mut self);
                if let Some(err) = &unfinished {
                    self.lost.add(Lost::one(err.raw_os_error()));
                }
                Ok(Renamed {
                    dir,
                    lost: self.lost,
                    owner_only: unfinished.is_some(),
                })
            }
            #[cfg(any(windows, test))]
            Err(replace_keeping::Failed::TargetGone(err)) => {
                // The temp file stays: it holds the new content, and the old file is not there.
                Err(RenameFailed {
                    error: Error::io("replace; the folder changed", &self.target, err),
                    changed: true,
                    refused: None,
                    torn: None,
                })
            }
            Err(failed) => {
                let err = failed.into_error();
                #[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
                if let Some(saved) = saved
                    && self.keep
                    && err.kind() == io::ErrorKind::PermissionDenied
                {
                    return super::in_place::write_in_place(self, temp, dir, err, saved);
                }
                #[cfg(not(any(target_os = "macos", all(test, target_os = "linux"))))]
                let _ = saved;
                self.temp = Some(temp);
                Err(RenameFailed {
                    error: Error::io("rename", &self.target, err),
                    changed: false,
                    refused: None,
                    torn: None,
                })
            }
        }
    }
}

/// `attempt`, and on Windows `attempt` again after each of the walk's short waits
/// ([`crate::checkpoint::VANISHED_WAITS`], 300 ms in all) while it fails as `denied` says. A
/// rename or delete of a matter file that Windows refuses with an access denied may be a hold
/// for a moment (an antivirus or the indexer scanning the file) that the check for a file held
/// open does not see; only one that still fails after the waits counts as the file's
/// permissions. Elsewhere tried once: a permission denied there is the file's or its folder's.
pub(crate) fn again_while_denied<T, E>(
    mut attempt: impl FnMut() -> Result<T, E>,
    denied: impl Fn(&E) -> bool,
) -> Result<T, E> {
    if cfg!(windows) {
        retried(attempt, denied)
    } else {
        attempt()
    }
}

/// [`again_while_denied`]'s waits, on every OS: tested off Windows too.
fn retried<T, E>(
    mut attempt: impl FnMut() -> Result<T, E>,
    denied: impl Fn(&E) -> bool,
) -> Result<T, E> {
    let mut outcome = attempt();
    for wait in crate::checkpoint::VANISHED_WAITS {
        match &outcome {
            Err(err) if denied(err) => {
                crate::checkpoint::pause(wait);
                outcome = attempt();
            }
            _ => break,
        }
    }
    outcome
}

/// The step after the rename of `staged`'s temp file: on macOS the old file's ACL and then its
/// mode, set on the target once it is that file ([`super::keep::After`]); until then the file is
/// its owner's alone. Its failure, if any: never a failure of the write.
fn finish(staged: &mut Staged) -> Option<io::Error> {
    #[cfg(test)]
    if let Err(err) = injected("after rename") {
        return Some(err);
    }
    #[cfg(target_os = "macos")]
    if let Some(after) = staged.after.take() {
        return after.apply(&staged.target, staged.id).err();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = staged;
    None
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Duration;

    use crate::checkpoint::pauses;

    /// A try refused as `denied` says is tried again after each of the walk's waits, and stops
    /// at the first that is not; one that is refused every time ends with the last refusal.
    #[test]
    fn a_denied_try_is_tried_again_after_each_short_wait() {
        let tries = Cell::new(0);
        let attempt = || {
            tries.set(tries.get() + 1);
            if tries.get() < 3 {
                Err("denied")
            } else {
                Ok(tries.get())
            }
        };
        assert_eq!(super::retried(attempt, |err| *err == "denied"), Ok(3));
        let (waits, waited) = pauses::take();
        assert_eq!((waits, waited), (2, Duration::from_millis(150)));

        let always = || Err::<(), _>("denied");
        assert_eq!(
            super::retried(always, |err| *err == "denied"),
            Err("denied")
        );
        assert_eq!(pauses::take().0, crate::checkpoint::VANISHED_WAITS.len());

        let other = || Err::<(), _>("other");
        assert_eq!(super::retried(other, |err| *err == "denied"), Err("other"));
        assert_eq!(pauses::take().0, 0);
    }
}
