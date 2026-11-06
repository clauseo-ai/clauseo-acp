//! Elicitation values withheld from tool output (0002, `elicitation_resolved`).
//!
//! The values the lawyer submits in an accepted form elicitation are never logged as the
//! answer, but the agent has them and a tool call may echo them in its output. The turn keeps
//! the submitted strings in memory, zeroized on drop, and every later tool call event of the
//! turn has them replaced with [`WITHHELD`] before `build.rs` decides inline or blob, so the
//! event log and the blobs never hold them:
//!
//! - a form scoped to a tool call (the scope's `toolCallId`): every non-empty string of the
//!   values, inside lists included, from that call's later updates, until the call completes
//!   or fails or the turn ends;
//! - a form with no tool call scope: every string of [`UNSCOPED_MIN_CHARS`] chars or more, from
//!   every tool call of the turn, until the turn ends. Shorter strings ("Yes", "Fees") would
//!   also blank ordinary words of unrelated output.
//!
//! Numbers and booleans are not withheld: they cannot be told apart from other output. A
//! declined or cancelled form, and a URL elicitation, keep nothing. The agent's own message
//! text, and a value it transforms (encodes, splits, rewords), are not covered.
//!
//! Matching is exact and case-sensitive, earliest match first and the longest value at a
//! position, so one value inside another leaves no remainder and the marker itself is never
//! matched again. JSON fields (`raw_input`, `raw_output`, an unsupported block's `raw`) are
//! parsed and scrubbed in their string leaves and object keys, so escapes cannot hide a match
//! and the JSON stays valid.

use std::collections::BTreeMap;

use clauseo_protocol::{
    CompactString, ContentBlock, ElicitationId, RawPayload, TextOrBlob, ToolCallContent,
    ToolCallPatch, ToolCallStatus, WITHHELD,
};
use serde_json::Value;
use zeroize::Zeroizing;

/// The shortest string, in chars, an elicitation with no tool call scope withholds.
pub(super) const UNSCOPED_MIN_CHARS: usize = 8;

/// The withheld strings of one turn. Its `Debug` prints counts only.
#[derive(Default)]
pub(super) struct Withheld {
    /// The pending form elicitations scoped to a tool call: the agent's `toolCallId`.
    scopes: BTreeMap<ElicitationId, CompactString>,
    /// Strings withheld from one tool call, by the agent's `toolCallId`.
    calls: BTreeMap<CompactString, Vec<Zeroizing<String>>>,
    /// Strings withheld from every tool call of the turn (unscoped forms).
    turn: Vec<Zeroizing<String>>,
}

impl std::fmt::Debug for Withheld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Withheld")
            .field("scopes", &self.scopes.len())
            .field("calls", &self.calls.len())
            .field("turn", &self.turn.len())
            .finish()
    }
}

impl Withheld {
    /// An elicitation of the turn was logged; `acp_tool_call_id` is its scope's tool call.
    pub(super) fn requested(
        &mut self,
        elicitation: ElicitationId,
        acp_tool_call_id: Option<CompactString>,
    ) {
        if let Some(acp) = acp_tool_call_id {
            self.scopes.insert(elicitation, acp);
        }
    }

    /// A form elicitation was accepted with `values`: keeps its strings.
    pub(super) fn accepted(&mut self, elicitation: ElicitationId, values: &RawPayload) {
        let mut strings = Vec::new();
        collect(values, &mut strings);
        match self.scopes.remove(&elicitation) {
            Some(acp) => {
                let kept = self.calls.entry(acp).or_default();
                for string in strings {
                    if !string.is_empty() && !kept.iter().any(|old| **old == *string) {
                        kept.push(Zeroizing::new(string.to_owned()));
                    }
                }
            }
            None => {
                for string in strings {
                    if string.chars().count() >= UNSCOPED_MIN_CHARS
                        && !self.turn.iter().any(|old| **old == *string)
                    {
                        self.turn.push(Zeroizing::new(string.to_owned()));
                    }
                }
            }
        }
    }

    /// An elicitation was declined, cancelled, or was not a form: it keeps nothing.
    pub(super) fn forget(&mut self, elicitation: ElicitationId) {
        self.scopes.remove(&elicitation);
    }

    /// Whether anything is withheld from tool call `acp_id`.
    #[cfg(test)]
    pub(super) fn covers(&self, acp_id: &str) -> bool {
        !self.turn.is_empty() || self.calls.contains_key(acp_id)
    }

    /// Scrubs a tool call's update before it is built; a `Completed` or `Failed` status then
    /// drops what is withheld from that call alone.
    pub(super) fn scrub_patch(&mut self, acp_id: &str, patch: &mut ToolCallPatch) {
        if let Some(needles) = self.needles(acp_id) {
            let needles = Needles::new(&needles);
            if let Some(title) = patch.title.as_mut()
                && let Some(new) = needles.replace(title)
            {
                *title = CompactString::from(new);
            }
            if let Some(value) = patch.raw_input.as_mut() {
                needles.json(value);
            }
            if let Some(value) = patch.raw_output.as_mut() {
                needles.json(value);
            }
            for item in patch.content.iter_mut().flatten() {
                needles.content(item);
            }
        }
        self.status(acp_id, patch.status);
    }

    /// Scrubs a tool call the agent starts after the answer.
    pub(super) fn scrub_call(&mut self, call: &mut clauseo_acp::ToolCall) {
        let acp_id = call.acp_id.clone();
        if let Some(needles) = self.needles(&acp_id) {
            let needles = Needles::new(&needles);
            if let Some(new) = needles.replace(&call.title) {
                call.title = CompactString::from(new);
            }
            if let Some(value) = call.raw_input.as_mut() {
                needles.json(value);
            }
            if let Some(value) = call.raw_output.as_mut() {
                needles.json(value);
            }
            for item in &mut call.content {
                needles.content(item);
            }
        }
        self.status(&acp_id, Some(call.status));
    }

    fn status(&mut self, acp_id: &str, status: Option<ToolCallStatus>) {
        if matches!(
            status,
            Some(ToolCallStatus::Completed | ToolCallStatus::Failed)
        ) {
            self.calls.remove(acp_id);
        }
    }

    /// The strings withheld from `acp_id`, longest first, or `None` when there are none.
    fn needles(&self, acp_id: &str) -> Option<Vec<&str>> {
        let mut needles: Vec<&str> = self
            .calls
            .get(acp_id)
            .into_iter()
            .flatten()
            .chain(&self.turn)
            .map(|string| string.as_str())
            .collect();
        if needles.is_empty() {
            return None;
        }
        longest_first(&mut needles);
        Some(needles)
    }
}

/// Sorts `needles` longest first and drops repeats.
fn longest_first(needles: &mut Vec<&str>) {
    needles.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    needles.dedup();
}

/// Every string in `value`, a map's values and a list's items included; keys are the form's
/// field names, not values.
fn collect<'a>(value: &'a RawPayload, out: &mut Vec<&'a str>) {
    match value {
        RawPayload::Str(string) => out.push(string),
        RawPayload::Seq(items) => items.iter().for_each(|item| collect(item, out)),
        RawPayload::Map(entries) => entries.iter().for_each(|(_, item)| collect(item, out)),
        RawPayload::Null
        | RawPayload::Bool(_)
        | RawPayload::I64(_)
        | RawPayload::U64(_)
        | RawPayload::F64(_)
        | RawPayload::Bytes(_) => {}
    }
}

/// The strings to replace, longest first.
struct Needles<'a> {
    plain: &'a [&'a str],
}

impl<'a> Needles<'a> {
    const fn new(plain: &'a [&'a str]) -> Self {
        Self { plain }
    }

    /// `text` with every needle replaced, or `None` when none occurs.
    fn replace(&self, text: &str) -> Option<String> {
        replace_all(text, self.plain)
    }

    fn text(&self, text: &mut String) {
        if let Some(new) = self.replace(text) {
            *text = new;
        }
    }

    fn inline(&self, value: &mut TextOrBlob) {
        // ACP hands every value over inline; `build.rs` makes the blobs after this.
        if let TextOrBlob::Inline(text) = value {
            self.text(text);
        }
    }

    /// A JSON text: scrubbed in its parsed string leaves and keys, written back compact. Text
    /// that does not parse is scrubbed as text, with each needle also in its JSON-escaped form.
    fn json(&self, value: &mut TextOrBlob) {
        let TextOrBlob::Inline(text) = value else {
            return;
        };
        if let Ok(mut parsed) = serde_json::from_str::<Value>(text) {
            if self.walk(&mut parsed)
                && let Ok(new) = serde_json::to_string(&parsed)
            {
                *text = new;
            }
            return;
        }
        // The escaped copies hold the values too: zeroized on drop.
        let escaped: Vec<Zeroizing<String>> = self
            .plain
            .iter()
            .filter_map(|needle| {
                let quoted = Zeroizing::new(serde_json::to_string(needle).ok()?);
                let inner = quoted.get(1..quoted.len().saturating_sub(1))?;
                (inner != *needle).then(|| Zeroizing::new(inner.to_owned()))
            })
            .collect();
        let mut all: Vec<&str> = self.plain.to_vec();
        all.extend(escaped.iter().map(|needle| needle.as_str()));
        longest_first(&mut all);
        if let Some(new) = replace_all(text, &all) {
            *text = new;
        }
    }

    /// Replaces in every string and key of `value`; whether anything changed.
    fn walk(&self, value: &mut Value) -> bool {
        match value {
            Value::String(string) => match self.replace(string) {
                Some(new) => {
                    *string = new;
                    true
                }
                None => false,
            },
            Value::Array(items) => items
                .iter_mut()
                .fold(false, |changed, item| self.walk(item) | changed),
            Value::Object(map) => {
                let mut changed = false;
                let entries = std::mem::take(map);
                for (key, mut item) in entries {
                    changed |= self.walk(&mut item);
                    let key = match self.replace(&key) {
                        Some(new) => {
                            changed = true;
                            new
                        }
                        None => key,
                    };
                    map.insert(key, item);
                }
                changed
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => false,
        }
    }

    fn block(&self, block: &mut ContentBlock) {
        match block {
            ContentBlock::Text { text } => self.text(text),
            ContentBlock::Resource { text, .. } => {
                if let Some(text) = text.as_mut() {
                    self.inline(text);
                }
            }
            ContentBlock::Unsupported { raw, .. } => {
                if let Some(raw) = raw.as_mut() {
                    self.json(raw);
                }
            }
            ContentBlock::Image { .. }
            | ContentBlock::Audio { .. }
            | ContentBlock::ResourceLink { .. } => {}
        }
    }

    fn content(&self, item: &mut ToolCallContent) {
        match item {
            ToolCallContent::Text { text } => self.inline(text),
            ToolCallContent::Content { content } => self.block(content),
            ToolCallContent::Diff { old, new, .. } => {
                if let Some(old) = old.as_mut() {
                    self.inline(old);
                }
                self.inline(new);
            }
            ToolCallContent::Unknown { .. } => {}
        }
    }
}

/// `text` with each occurrence of a needle replaced by [`WITHHELD`], scanning left to right:
/// at each position the earliest match wins and, among matches there, the longest (`needles`
/// come longest first). `None` when nothing matched.
fn replace_all(text: &str, needles: &[&str]) -> Option<String> {
    // The next occurrence of each needle at or after `pos`; `None` once it has no more.
    let mut next: Vec<Option<usize>> = needles.iter().map(|needle| text.find(needle)).collect();
    let mut out = String::new();
    let mut pos = 0;
    let mut changed = false;
    loop {
        let mut best: Option<(usize, usize)> = None;
        for (slot, needle) in next.iter_mut().zip(needles) {
            if needle.is_empty() {
                continue;
            }
            if slot.is_some_and(|at| at < pos) {
                *slot = text
                    .get(pos..)
                    .and_then(|rest| rest.find(needle))
                    .map(|found| pos + found);
            }
            if let Some(at) = *slot
                && best.is_none_or(|(best_at, _)| at < best_at)
            {
                best = Some((at, needle.len()));
            }
        }
        let Some((at, len)) = best else {
            break;
        };
        out.push_str(text.get(pos..at).unwrap_or_default());
        out.push_str(WITHHELD);
        pos = at + len;
        changed = true;
    }
    if !changed {
        return None;
    }
    out.push_str(text.get(pos..).unwrap_or_default());
    Some(out)
}

#[cfg(test)]
mod tests;
