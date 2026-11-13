//! The places a turn's tool calls wrote, for the after-turn checkpoint's lawyer-edits flag
//! (`clauseo_matters::Matter::after_turn_named`): a change at a place no call wrote may hold the
//! lawyer's edits, so Undo warns and keeps it by default.
//!
//! Each call is kept by a key unique in the turn (ACP's id live, the logged id when read back),
//! with its kind and status as last known and every place it named, so the rule is applied once,
//! at the turn's end ([`TurnPaths::written`]): a place counts when
//! [`clauseo_matters::names_a_write`] holds for it (a diff's path, an edit, delete or move
//! call's location; never a read's, a search's or a shell call's location; nothing of a call
//! that ended failed). A file the lawyer saves during a turn that reads it is so never taken for
//! the agent's. A shell call's writes are read from its command ([`crate::shell_writes`]).

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{
    MatterPath, TextOrBlob, ToolCallContent, ToolCallPatch, ToolCallStarted, ToolCallStatus,
    ToolKind, ToolLocation,
};

use crate::shell_writes;

/// Places as a tool call names them: matter paths, and the text of every location or diff `acp`
/// placed outside the session folder (the path as the agent sent it). A spelling of a matter
/// file that no text comparison with the folder places inside it (through a symlink, in another
/// case) is among `outside`; the matter thread resolves those on the disk (`matter_worker`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Places {
    /// The matter paths.
    pub(crate) inside: BTreeSet<MatterPath>,
    /// The `Outside` places, as `acp` displays them.
    pub(crate) outside: BTreeSet<String>,
}

impl Places {
    /// Adds `location`; an `Unknown` one names nothing.
    fn add(&mut self, location: &ToolLocation) {
        match location {
            ToolLocation::Inside { path, .. } => {
                if !self.inside.contains(path) {
                    self.inside.insert(path.clone());
                }
            }
            ToolLocation::Outside { display } => {
                if !self.outside.contains(display) {
                    self.outside.insert(display.clone());
                }
            }
            ToolLocation::Unknown { .. } => {}
        }
    }

    /// Adds every place of `other`.
    fn extend(&mut self, other: &Self) {
        self.inside.extend(other.inside.iter().cloned());
        self.outside.extend(other.outside.iter().cloned());
    }
}

/// One tool call of the turn as its start and updates have told it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Call {
    /// Its kind as last sent; `None` for an update of a call this turn never saw start.
    kind: Option<ToolKind>,
    /// Its status as last sent.
    status: Option<ToolCallStatus>,
    /// Whether it was ever a shell call (`ToolKind::Execute`): its input is then read for writes.
    shell: bool,
    /// The places its locations named.
    located: Places,
    /// The places its diffs named.
    diffed: Places,
}

/// The places a turn's tool calls named, kept until the turn's end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TurnPaths {
    /// Each tool call, by its key.
    calls: BTreeMap<String, Call>,
    /// The files the turn's shell commands write ([`crate::shell_writes`]): paths relative to
    /// the matter's folder, or absolute ones the matter thread places in it.
    pub(crate) writes: BTreeSet<String>,
}

/// One tool call's start or update as [`TurnPaths::note`] reads it: its kind, status, places
/// and input, each `None` or empty where an update leaves it as it was.
pub(crate) struct CallFields<'a> {
    pub(crate) kind: Option<ToolKind>,
    pub(crate) status: Option<ToolCallStatus>,
    pub(crate) locations: &'a [ToolLocation],
    pub(crate) content: &'a [ToolCallContent],
    pub(crate) raw_input: Option<&'a TextOrBlob>,
}

impl<'a> CallFields<'a> {
    /// A patch's fields.
    pub(crate) fn of_patch(patch: &'a ToolCallPatch) -> Self {
        Self {
            kind: patch.kind,
            status: patch.status,
            locations: patch.locations.as_deref().unwrap_or_default(),
            content: patch.content.as_deref().unwrap_or_default(),
            raw_input: patch.raw_input.as_ref(),
        }
    }

    /// A live call's fields, as the agent sent them.
    pub(crate) fn of_call(call: &'a clauseo_acp::ToolCall) -> Self {
        Self {
            kind: Some(call.kind),
            status: Some(call.status),
            locations: &call.locations,
            content: &call.content,
            raw_input: call.raw_input.as_ref(),
        }
    }

    /// A logged start's fields.
    pub(crate) fn of_started(started: &'a ToolCallStarted) -> Self {
        Self {
            kind: Some(started.kind),
            status: Some(started.status),
            locations: &started.locations,
            content: &started.content,
            raw_input: started.raw_input.as_ref(),
        }
    }
}

impl TurnPaths {
    /// Adds what one tool call's start or update (of the call `key`) tells: its kind and status,
    /// the places it names, and, for a shell call, the files its command writes, read from the
    /// command alone, never its output, which names files it only lists or reads. An input kept
    /// as a blob is not read.
    pub(crate) fn note(&mut self, key: &str, fields: &CallFields<'_>) {
        if !self.calls.contains_key(key) {
            self.calls.insert(key.to_owned(), Call::default());
        }
        let Some(call) = self.calls.get_mut(key) else {
            return;
        };
        if let Some(kind) = fields.kind {
            call.kind = Some(kind);
            call.shell |= kind == ToolKind::Execute;
        }
        if let Some(status) = fields.status {
            call.status = Some(status);
        }
        for location in fields.locations {
            call.located.add(location);
        }
        for location in diffs(fields.content) {
            call.diffed.add(location);
        }
        if call.shell
            && let Some(TextOrBlob::Inline(input)) = fields.raw_input
        {
            shell_writes::add_input(&mut self.writes, input);
        }
    }

    /// The places the turn's tool calls wrote ([`clauseo_matters::names_a_write`]).
    pub(crate) fn written(&self) -> Places {
        let mut written = Places::default();
        for call in self.calls.values() {
            // A call never seen start is of no known kind: only its diffs can count.
            let kind = call.kind.unwrap_or(ToolKind::Unknown);
            let status = call.status.unwrap_or(ToolCallStatus::Pending);
            if clauseo_matters::names_a_write(kind, status, false) {
                written.extend(&call.located);
            }
            if clauseo_matters::names_a_write(kind, status, true) {
                written.extend(&call.diffed);
            }
        }
        written
    }

    /// Paths as one completed edit call named them, for the (Unix-only) test of what reads them.
    #[cfg(all(test, unix))]
    pub(crate) fn edited(places: Places) -> Self {
        let call = Call {
            kind: Some(ToolKind::Edit),
            status: Some(ToolCallStatus::Completed),
            located: places,
            ..Call::default()
        };
        Self {
            calls: [("edit".to_owned(), call)].into(),
            ..Self::default()
        }
    }
}

/// The locations of the diffs among `content`.
pub(crate) fn diffs(content: &[ToolCallContent]) -> impl Iterator<Item = &ToolLocation> {
    content.iter().filter_map(|item| match item {
        ToolCallContent::Diff { path, .. } => Some(path),
        ToolCallContent::Text { .. }
        | ToolCallContent::Content { .. }
        | ToolCallContent::Unknown { .. } => None,
    })
}

#[cfg(test)]
mod tests;
