//! One live check in progress ([`Run`]): the scans below what the watch reported, each item
//! found moved at once, the session ended first (kill first), and the budget kept.
//!
//! The session ends before anything is moved, and before anything inside an unknown git folder
//! is read: its listing says it is one, and its `commondir` (which an agent can hold an oplock on)
//! is read only after the end. When ending can't be confirmed ([`Hooks::end`] says the agent's
//! job isn't empty yet), the check moves nothing more and stops: what it found is left for the
//! check after the session ([`Live::left`]), since a move could race what is still alive.

use std::ffi::OsString;
use std::time::{Duration, SystemTime};

use super::super::files::FileId;
use super::super::moves::set_aside_owned;
use super::super::run::{Need, Refused, Volume};
use super::super::scan::{Found, Item, Kept, Missed, Now, Step, reach, scan_part_visiting};
use super::super::{Reason, SetAsideFailure};
use super::keys::{below_git, ends_in_git, holds};
use super::{Ending, Failed, Hooks, Live, Moved, SCAN_BUDGET, Watched};
use crate::SetAsideWhat;

/// One live check in progress: what it moved so far, and when it began.
pub(super) struct Run<'a, V: Volume, H: Hooks> {
    watched: &'a Watched,
    volume: &'a V,
    now: SystemTime,
    hooks: &'a H,
    pub(super) live: &'a mut Live,
    began: Duration,
    slow: bool,
    /// Past [`SCAN_BUDGET`] in a folder made or moved in: the scan stops ([`Step::Stop`]).
    stopping: bool,
    /// What this check knows of the session's end.
    ended: Ended,
}

/// What a check knows of the session's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    /// It hasn't asked.
    Not,
    /// The session ended with the agent's job empty: the check asks no more.
    Empty,
    /// The end couldn't confirm the job is empty: nothing more is moved, and scans stop.
    Unsure,
}

impl<'a, V: Volume, H: Hooks> Run<'a, V, H> {
    pub(super) fn new(
        watched: &'a Watched,
        volume: &'a V,
        now: SystemTime,
        hooks: &'a H,
        live: &'a mut Live,
    ) -> Self {
        let began = hooks.now();
        Run {
            watched,
            volume,
            now,
            hooks,
            live,
            began,
            slow: false,
            stopping: false,
            ended: Ended::Not,
        }
    }

    /// Ends the session for an item found ([`Ending::Found`]), once for the check; whether the
    /// agent's job is empty, so moves may go on. Once it isn't, it stays so for this check.
    pub(super) fn end_found(&mut self) -> bool {
        self.end(Ending::Found)
    }

    /// Ends the session for `why` unless this check already did; whether the job is empty.
    fn end(&mut self, why: Ending) -> bool {
        if self.ended == Ended::Not {
            self.ended = if self.hooks.end(why) {
                Ended::Empty
            } else {
                tracing::warn!("the agent's job isn't empty yet; the watch moves nothing more now");
                Ended::Unsure
            };
        }
        self.ended == Ended::Empty
    }

    /// Whether the end couldn't confirm the job is empty: the check moves nothing more.
    fn halted(&self) -> bool {
        self.ended == Ended::Unsure
    }

    /// Scans below the folder `key`, made or moved in, moving as it goes. A folder gone is
    /// nothing to scan. Past [`SCAN_BUDGET`] the session has ended and the scan stops: an agent
    /// moving in a vast tree with a repository last would keep it there a long while. The folder
    /// is then set aside whole when a pool user (the agent) owns it, as read from the handle the
    /// move renames (`Need::Pool`); anyone else's (the lawyer's, dragged in while the agent ran),
    /// or one whose owner can't be read, stays, for the check after the session.
    pub(super) fn below(&mut self, key: &[OsString]) {
        let (volume, root) = (self.volume, self.watched.root.clone());
        let scanned = scan_part_visiting(volume, &root, key, true, &mut |found| {
            self.visit(found, true)
        });
        match scanned {
            Ok(Some(scan)) if scan.stopped && !self.halted() => {
                self.stopping = false;
                self.check(key, SetAsideWhat::UncheckedFolder);
            }
            Ok(_) => {}
            Err(error) => {
                let error = error.kind();
                tracing::debug!(%error, "a reported folder the watch can't scan");
            }
        }
    }

    /// What the scan found, moved at once when the watch moves it and doesn't know it: in a
    /// folder made or moved in (`new`), a git folder whole; what is below a `.git` is left to
    /// that one (the known one's tree is denied, an unknown one goes whole). [`Step::Prune`]
    /// once moved, so nothing below it is scanned. An unknown git folder ends the session
    /// before anything in it is read, in a rescan too (which then moves only its `HEAD`).
    pub(super) fn visit(&mut self, found: Found<'_>, new: bool) -> Step {
        if self.stopping || self.halted() {
            return Step::Stop;
        }
        if !self.slow && self.hooks.now().saturating_sub(self.began) > SCAN_BUDGET {
            self.slow = true;
            if !self.end(Ending::Slow) {
                return Step::Stop;
            }
            if new {
                self.stopping = true;
                return Step::Stop;
            }
        }
        let watched = self.watched;
        let in_git = |id: Option<FileId>| id.is_some_and(|id| watched.git_folder(id));
        match found {
            Found::Folder => Step::Go,
            Found::Bare(folder, id) => {
                if in_git(id) || below_git(folder) || self.head_known(folder) {
                    return Step::Go;
                }
                if !self.end_found() {
                    self.live.left.push(watched.path(folder));
                    return Step::Stop;
                }
                if !new {
                    return Step::Go;
                }
                let item = watched.item(folder, Now::Shut);
                self.step(&item, SetAsideWhat::GitRepository)
            }
            Found::Item(item) => {
                if below_git(&item.key) {
                    return Step::Go;
                }
                let what = match item.kept {
                    Kept::Link if watched.leads_out(self.volume, &item.key) => {
                        SetAsideWhat::LinkOut
                    }
                    Kept::Head if in_git(item.through.last().copied()) => return Step::Go,
                    Kept::Head | Kept::Common => SetAsideWhat::GitRepository,
                    Kept::Root | Kept::Nested if ends_in_git(&item.key) => {
                        SetAsideWhat::GitRepository
                    }
                    Kept::Root | Kept::Nested | Kept::Link => return Step::Go,
                };
                match item.now {
                    Now::Absent => Step::Go,
                    Now::At(id) if watched.knows_as(&item.key, id, what) => Step::Go,
                    Now::At(_) | Now::Shut | Now::Failed(_) => self.step(item, what),
                }
            }
        }
    }

    /// Moves `item`; [`Step::Prune`] when it moved, [`Step::Stop`] once the check halted.
    fn step(&mut self, item: &Item, what: SetAsideWhat) -> Step {
        if self.move_one(item, what) {
            Step::Prune
        } else if self.halted() {
            Step::Stop
        } else {
            Step::Go
        }
    }

    /// Whether the `HEAD` in the folder `folder` is one the watch knows. A key the record
    /// doesn't hold at all is unknown without opening anything in the folder.
    fn head_known(&self, folder: &[OsString]) -> bool {
        let mut key = folder.to_vec();
        key.push(OsString::from("HEAD"));
        if !holds(&self.watched.known, &key, None) {
            return false;
        }
        matches!(
            reach(self.volume, &self.watched.root, &key),
            Ok((_, Some((id, _)))) if self.watched.knows_as(&key, id, SetAsideWhat::GitRepository)
        )
    }

    /// Moves the item at `key` as `what` unless nothing is there or the watch knows it there,
    /// read through handles from the watched folder ([`reach`]). One the watch was told of and
    /// that is gone is listed in [`Live::gone`], as is one past a folder that is a link now: it
    /// isn't in the watched folder, and the link is never followed.
    pub(super) fn check(&mut self, key: &[OsString], what: SetAsideWhat) {
        let now_at = match reach(self.volume, &self.watched.root, key) {
            Ok((_, None)) | Err(Missed::Gone | Missed::Link) => {
                self.live.gone.push(key.iter().collect());
                return;
            }
            Ok((_, Some((id, _)))) if self.watched.knows_as(key, id, what) => return,
            Ok((_, Some((id, _)))) => Now::At(id),
            Err(Missed::Failed(_)) => Now::Shut,
        };
        let item = self.watched.item(key, now_at);
        self.move_one(&item, what);
    }

    /// Ends the session, then moves `item` as `what` under the matter's lock into this pass's
    /// folder at its own path, as the after-session pass does. Whether it moved. One gone
    /// meanwhile is listed in [`Live::gone`]; with the end unconfirmed, it is left
    /// ([`Live::left`]). A folder set aside unchecked must be a pool user's, read from the
    /// handle the move renames; the owner read there also tells the lawyer's notice whose it
    /// was ([`Moved::by_agent`]). The item is first reached through handles from the watched
    /// folder: one gone, or past a folder that is a link now, is listed gone and never moved
    /// through the link; the intent's owner is read there, relative to the item's folder.
    fn move_one(&mut self, item: &Item, what: SetAsideWhat) -> bool {
        // Kill first: nothing of the agent's is alive to race the move, or to put another back.
        if !self.end_found() {
            self.live.left.push(item.path.clone());
            return false;
        }
        let watched = self.watched;
        let owner = match reach(self.volume, &watched.root, &item.key) {
            Ok((folder, Some(_))) => item
                .key
                .last()
                .and_then(|name| self.volume.owner_at(&folder, name).ok()),
            Ok((_, None)) | Err(Missed::Gone | Missed::Link) => {
                tracing::debug!("an item went, or is past a link now, before it could be moved");
                self.live.gone.push(item.key.iter().collect());
                return false;
            }
            Err(Missed::Failed(_)) => None,
        };
        let seen = match what {
            SetAsideWhat::LinkOut => holds(&watched.links, &item.key, None),
            SetAsideWhat::GitRepository => holds(&watched.known, &item.key, None),
            SetAsideWhat::UncheckedFolder => false,
        };
        let reason = if seen { Reason::Changed } else { Reason::New };
        let pool = self.volume.pool().unwrap_or_else(|error| {
            let error = error.kind();
            tracing::warn!(%error, "the pool record is unreadable; no item counts as the agent's");
            Vec::new()
        });
        let need = match what {
            SetAsideWhat::UncheckedFolder => Need::Pool(&pool),
            SetAsideWhat::GitRepository | SetAsideWhat::LinkOut => Need::Anyone,
        };
        let place = &watched.place;
        let moved = place
            .lock()
            .map_err(|failure| Refused::at(failure, &item.path))
            .and_then(|held| {
                let source = (item.path.as_path(), item.within.as_slice(), owner);
                let moved = set_aside_owned(self.volume, place, source, reason, self.now, need);
                drop(held);
                moved
            });
        match moved {
            Ok(entry) => {
                // An owner that can't be read counts as the agent's for the notice
                // (`Moved::by_agent`); only a `.git` or link moves with one (`Need::Anyone`).
                let by_agent = entry
                    .owner
                    .as_ref()
                    .is_none_or(|owner| pool.contains(owner));
                self.live.moved.push(Moved {
                    entry,
                    path: item.key.iter().collect(),
                    what,
                    by_agent,
                });
                true
            }
            Err(refused) if refused.failure == SetAsideFailure::NotTheAgents => {
                tracing::info!("a folder too big to check in time isn't the agent's; left");
                false
            }
            Err(refused) if refused.failure == SetAsideFailure::Gone => {
                tracing::debug!("an item went before the watch could set it aside");
                self.live.gone.push(item.key.iter().collect());
                false
            }
            Err(refused) => {
                self.live.failed.push(Failed {
                    path: item.path.clone(),
                    failure: refused.failure,
                    what,
                });
                false
            }
        }
    }
}
