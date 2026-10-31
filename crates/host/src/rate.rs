//! The rate cap of one agent session: past [`RATE_CAP_PER_SEC`] updates in a 1 s `mono` window,
//! tool call patches merge per tool call and replace-style updates keep their last value until the
//! window ends. Time comes in as an argument; this module reads no clock.
//!
//! Held items keep their arrival order (a merge or a replace keeps the place of the first one
//! held), and any other item first releases everything held, so the agent's order holds between
//! kinds that do not merge.

use std::time::Duration;

use clauseo_acp::{AcpEvent, AgentUpdate, Origin};
use clauseo_protocol::ToolCallPatch;

/// Updates per second a session passes before merging starts.
pub(crate) const RATE_CAP_PER_SEC: u32 = 2_000;

/// The length of a rate window.
pub(crate) const RATE_WINDOW: Duration = Duration::from_secs(1);

/// How an item behaves over the cap. Items merge or replace only with held items of the same
/// variant and an equal key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateClass<K> {
    /// Merged into the held item with the same key ([`Rated::merge`]).
    Merge(K),
    /// Replaces the held item with the same key: the last value is kept.
    Replace(K),
    /// Never held: releases everything held first, then passes.
    Other,
}

/// An item the rate cap can hold.
pub(crate) trait Rated: Sized {
    /// What two items must share to merge or replace; borrowed from the item.
    type Key<'a>: PartialEq
    where
        Self: 'a;
    /// How this item behaves over the cap.
    fn class(&self) -> RateClass<Self::Key<'_>>;
    /// Folds `later`, an item of the same `Merge` key, into this one.
    fn merge(&mut self, later: Self);
}

/// What [`RateCap::admit`] decided.
#[derive(Debug, PartialEq)]
#[must_use]
pub(crate) enum Admitted<T> {
    /// Pass the item on now; nothing was held.
    Pass(T),
    /// Pass these on now, in this order: what was held, then the item.
    Flush(Vec<T>),
    /// The item is held. When `arm` is set, arm a timer for that `mono` deadline and call
    /// [`RateCap::window_end`] when it fires.
    Held {
        /// The window's end, the first time something is held in it.
        arm: Option<Duration>,
    },
}

impl<T> IntoIterator for Admitted<T> {
    type Item = T;
    type IntoIter = AdmittedIter<T>;

    /// The items to pass on now, in order.
    fn into_iter(self) -> AdmittedIter<T> {
        match self {
            Self::Pass(item) => AdmittedIter::One(Some(item)),
            Self::Flush(items) => AdmittedIter::Many(items.into_iter()),
            Self::Held { .. } => AdmittedIter::One(None),
        }
    }
}

/// The items an [`Admitted`] passes on.
#[derive(Debug)]
pub(crate) enum AdmittedIter<T> {
    One(Option<T>),
    Many(std::vec::IntoIter<T>),
}

impl<T> Iterator for AdmittedIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        match self {
            Self::One(item) => item.take(),
            Self::Many(items) => items.next(),
        }
    }
}

/// The rate cap of one session.
#[derive(Debug)]
pub(crate) struct RateCap<T> {
    cap: u32,
    /// Where the current window started; `None` before the first item and after its end.
    window_start: Option<Duration>,
    /// Items counted in the current window.
    count: u32,
    /// Items held over the cap, in arrival order.
    held: Vec<T>,
}

impl<T: Rated> Default for RateCap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Rated> RateCap<T> {
    /// A cap of [`RATE_CAP_PER_SEC`].
    pub(crate) fn new() -> Self {
        Self::with_cap(RATE_CAP_PER_SEC)
    }

    /// A cap of `cap` items per window, for tests.
    pub(crate) fn with_cap(cap: u32) -> Self {
        Self {
            cap,
            window_start: None,
            count: 0,
            held: Vec::new(),
        }
    }

    /// Whether anything is held.
    pub(crate) fn is_holding(&self) -> bool {
        !self.held.is_empty()
    }

    /// Counts `item` in the window at `now`. Under the cap it passes, after anything still held.
    /// Over it a `Merge` or `Replace` item is held, folded into a held item with the same key;
    /// an `Other` item passes after everything held.
    pub(crate) fn admit(&mut self, now: Duration, item: T) -> Admitted<T> {
        let start = match self.window_start {
            Some(start) if now < start.saturating_add(RATE_WINDOW) => start,
            Some(_) | None => {
                self.window_start = Some(now);
                self.count = 0;
                now
            }
        };
        self.count = self.count.saturating_add(1);
        if self.count <= self.cap {
            return self.release_with(item);
        }
        let was_empty = self.held.is_empty();
        let place = place_of(&self.held, &item);
        let Some((found, merge)) = place else {
            return self.release_with(item);
        };
        match found.and_then(|at| self.held.get_mut(at)) {
            Some(held) if merge => held.merge(item),
            Some(held) => *held = item,
            None => self.held.push(item),
        }
        Admitted::Held {
            arm: was_empty.then(|| start.saturating_add(RATE_WINDOW)),
        }
    }

    /// The window's timer fired: returns everything held, in arrival order, and ends the window
    /// when `now` is past it.
    pub(crate) fn window_end(&mut self, now: Duration) -> Vec<T> {
        if self
            .window_start
            .is_some_and(|start| now >= start.saturating_add(RATE_WINDOW))
        {
            self.window_start = None;
            self.count = 0;
        }
        std::mem::take(&mut self.held)
    }

    /// Returns everything held, in arrival order, without ending the window: for a turn's end,
    /// so nothing of it waits behind the end.
    pub(crate) fn flush(&mut self) -> Vec<T> {
        std::mem::take(&mut self.held)
    }

    fn release_with(&mut self, item: T) -> Admitted<T> {
        if self.held.is_empty() {
            return Admitted::Pass(item);
        }
        let mut items = std::mem::take(&mut self.held);
        items.push(item);
        Admitted::Flush(items)
    }
}

/// Where `item` goes among `held`: `None` when it is never held, else the held item with the
/// same key (when there is one) and whether it merges (`true`) or replaces (`false`). Both
/// borrows share one lifetime because a key type's variance is unknown.
fn place_of<'a, T: Rated>(held: &'a [T], item: &'a T) -> Option<(Option<usize>, bool)> {
    match item.class() {
        RateClass::Other => None,
        RateClass::Merge(key) => {
            let same = |other: &'a T| match other.class() {
                RateClass::Merge(other) => other == key,
                RateClass::Replace(_) | RateClass::Other => false,
            };
            Some((held.iter().position(same), true))
        }
        RateClass::Replace(key) => {
            let same = |other: &'a T| match other.class() {
                RateClass::Replace(other) => other == key,
                RateClass::Merge(_) | RateClass::Other => false,
            };
            Some((held.iter().position(same), false))
        }
    }
}

/// An agent event in the rate cap: a `ToolCallUpdate` merges per tool call; `Plan`, `Commands`,
/// `CurrentMode`, `ConfigOptions`, `Usage` and `SessionTitle` keep their last value per kind;
/// everything else (text, new tool calls, permission requests, limits, ...) is never held.
/// Updates merge only within the same [`Origin`], so a late update never folds into a turn's.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RatedUpdate(pub(crate) AcpEvent);

/// What [`RatedUpdate`]s must share to merge or replace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UpdateKey<'a> {
    origin: Origin,
    what: UpdateWhat<'a>,
}

/// The tool call a patch is for, or the replace-style kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateWhat<'a> {
    ToolCall(&'a str),
    Plan,
    Commands,
    CurrentMode,
    ConfigOptions,
    Usage,
    SessionTitle,
}

impl Rated for RatedUpdate {
    type Key<'a> = UpdateKey<'a>;

    fn class(&self) -> RateClass<UpdateKey<'_>> {
        let AcpEvent::Update { origin, update, .. } = &self.0 else {
            return RateClass::Other;
        };
        let replace = |what| {
            RateClass::Replace(UpdateKey {
                origin: *origin,
                what,
            })
        };
        match update {
            AgentUpdate::ToolCallUpdate { acp_id, .. } => RateClass::Merge(UpdateKey {
                origin: *origin,
                what: UpdateWhat::ToolCall(acp_id),
            }),
            AgentUpdate::Plan(_) => replace(UpdateWhat::Plan),
            AgentUpdate::Commands(_) => replace(UpdateWhat::Commands),
            AgentUpdate::CurrentMode { .. } => replace(UpdateWhat::CurrentMode),
            AgentUpdate::ConfigOptions(_) => replace(UpdateWhat::ConfigOptions),
            AgentUpdate::Usage { .. } => replace(UpdateWhat::Usage),
            AgentUpdate::SessionTitle { .. } => replace(UpdateWhat::SessionTitle),
            AgentUpdate::MessageChunk { .. }
            | AgentUpdate::ToolCall(_)
            | AgentUpdate::Notice { .. }
            | AgentUpdate::CompactionStarted { .. }
            | AgentUpdate::CompactionEnded { .. }
            | AgentUpdate::CompactionSummaryChunk { .. } => RateClass::Other,
        }
    }

    fn merge(&mut self, later: Self) {
        let (
            AcpEvent::Update {
                update: AgentUpdate::ToolCallUpdate { patch, .. },
                ..
            },
            AcpEvent::Update {
                update:
                    AgentUpdate::ToolCallUpdate {
                        patch: later_patch, ..
                    },
                ..
            },
        ) = (&mut self.0, later.0)
        else {
            // Only tool call patches merge; the cap never calls this for anything else.
            return;
        };
        merge_patch(patch, later_patch);
    }
}

/// Folds `later` into `patch`: every field `later` names replaces the earlier one.
fn merge_patch(patch: &mut ToolCallPatch, later: ToolCallPatch) {
    let ToolCallPatch {
        title,
        kind,
        tool_name,
        status,
        locations,
        raw_input,
        raw_output,
        content,
    } = later;
    if title.is_some() {
        patch.title = title;
    }
    if kind.is_some() {
        patch.kind = kind;
    }
    if tool_name.is_some() {
        patch.tool_name = tool_name;
    }
    if status.is_some() {
        patch.status = status;
    }
    if locations.is_some() {
        patch.locations = locations;
    }
    if raw_input.is_some() {
        patch.raw_input = raw_input;
    }
    if raw_output.is_some() {
        patch.raw_output = raw_output;
    }
    if content.is_some() {
        patch.content = content;
    }
}

#[cfg(test)]
mod tests;
