//! Elicitation values withheld from tool output (0002, `elicitation_resolved`).
//!
//! The values the lawyer submits in an accepted form elicitation are never logged as the
//! answer, but the agent has them and a tool call may echo them in its output. The turn keeps
//! the submitted strings in memory, zeroized on drop, and every later tool call event of the
//! turn has them replaced with [`WITHHELD`] before `build.rs` decides inline or blob, so the
//! event log and the blobs never hold them:
//!
//! - a form scoped to a tool call (the scope's `toolCallId`): every string of
//!   [`SCOPED_MIN_CHARS`] chars or more after trimming, inside lists included, from that call's
//!   later events, until the call completes or fails or the turn ends;
//! - a form with no tool call scope: every string of [`UNSCOPED_MIN_CHARS`] chars or more after
//!   trimming, from every tool call of the turn, until the turn ends. Shorter strings ("Yes",
//!   "Fees") would also blank ordinary words of unrelated output.
//!
//! Each value is matched in several forms ([`forms`]): as typed, trimmed, JSON-escaped, and
//! JSON-escaped with every non-ASCII char as `\uXXXX` (lower and upper hex). A value whose
//! trimmed text is a JSON number also replaces an equal number in parsed JSON. A declined or
//! cancelled form, and a URL elicitation, keep nothing. The agent's own message text, and a
//! value it transforms otherwise (encodes, splits, rewords), are not covered.
//!
//! Matching is exact and case-sensitive, earliest match first and the longest form at a
//! position, so one value inside another leaves no remainder and the marker itself is never
//! matched again. JSON fields (`raw_input`, `raw_output`, an unsupported block's `raw`) are
//! parsed and scrubbed in their string values, numbers and object keys, so the JSON stays
//! valid. Every other string field of the call is scrubbed as text (`fields.rs`).

mod fields;

use std::collections::BTreeMap;

use clauseo_protocol::{
    CompactString, ElicitationId, RawPayload, ToolCallPatch, ToolCallStatus, WITHHELD,
};
use serde_json::Value;
use zeroize::Zeroizing;

/// The shortest string, in chars after trimming, a form scoped to a tool call withholds.
pub(super) const SCOPED_MIN_CHARS: usize = 3;

/// The shortest string, in chars after trimming, a form with no tool call scope withholds.
pub(super) const UNSCOPED_MIN_CHARS: usize = 8;

/// The forms and numbers withheld from some tool calls.
#[derive(Default)]
struct Set {
    /// Every form of every value ([`forms`]).
    forms: Vec<Zeroizing<String>>,
    /// The values whose trimmed text is a JSON number, as `serde_json` writes that number.
    numbers: Vec<Zeroizing<String>>,
}

impl Set {
    fn add(&mut self, value: &str, min_chars: usize) {
        if value.trim().chars().count() < min_chars {
            return;
        }
        for form in forms(value) {
            if !self.forms.contains(&form) {
                self.forms.push(form);
            }
        }
        if let Ok(number) = serde_json::from_str::<serde_json::Number>(value.trim()) {
            let number = Zeroizing::new(number.to_string());
            if !self.numbers.contains(&number) {
                self.numbers.push(number);
            }
        }
    }
}

/// The withheld strings of one turn. Its `Debug` prints counts only.
#[derive(Default)]
pub(super) struct Withheld {
    /// The pending form elicitations scoped to a tool call: the agent's `toolCallId`.
    scopes: BTreeMap<ElicitationId, CompactString>,
    /// What is withheld from one tool call, by the agent's `toolCallId`.
    calls: BTreeMap<CompactString, Set>,
    /// What is withheld from every tool call of the turn (unscoped forms).
    turn: Set,
}

impl std::fmt::Debug for Withheld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Withheld")
            .field("scopes", &self.scopes.len())
            .field("calls", &self.calls.len())
            .field("turn", &self.turn.forms.len())
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
        let (set, min_chars) = match self.scopes.remove(&elicitation) {
            Some(acp) => (self.calls.entry(acp).or_default(), SCOPED_MIN_CHARS),
            None => (&mut self.turn, UNSCOPED_MIN_CHARS),
        };
        for string in strings {
            set.add(string, min_chars);
        }
    }

    /// An elicitation was declined, cancelled, or was not a form: it keeps nothing.
    pub(super) fn forget(&mut self, elicitation: ElicitationId) {
        self.scopes.remove(&elicitation);
    }

    /// Whether anything is withheld from tool call `acp_id`.
    #[cfg(test)]
    pub(super) fn covers(&self, acp_id: &str) -> bool {
        !self.turn.forms.is_empty() || self.calls.contains_key(acp_id)
    }

    /// Scrubs a tool call's update before it is built; a `Completed` or `Failed` status then
    /// drops what is withheld from that call alone.
    pub(super) fn scrub_patch(&mut self, acp_id: &str, patch: &mut ToolCallPatch) {
        if let Some((forms, numbers)) = self.needles(acp_id) {
            Needles::new(&forms, &numbers).patch(patch);
        }
        self.status(acp_id, patch.status);
    }

    /// Scrubs a tool call the agent starts after the answer.
    pub(super) fn scrub_call(&mut self, call: &mut clauseo_acp::ToolCall) {
        let acp_id = call.acp_id.clone();
        if let Some((forms, numbers)) = self.needles(&acp_id) {
            Needles::new(&forms, &numbers).call(call);
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

    /// The forms withheld from `acp_id`, longest first, and the numbers, or `None` when there
    /// are none.
    fn needles(&self, acp_id: &str) -> Option<(Vec<&str>, Vec<&str>)> {
        let sets = || self.calls.get(acp_id).into_iter().chain([&self.turn]);
        let mut forms: Vec<&str> = sets()
            .flat_map(|set| &set.forms)
            .map(|form| form.as_str())
            .collect();
        if forms.is_empty() {
            return None;
        }
        longest_first(&mut forms);
        let numbers = sets()
            .flat_map(|set| &set.numbers)
            .map(|number| number.as_str())
            .collect();
        Some((forms, numbers))
    }
}

/// The forms of `value` matched in tool output: as typed, trimmed (when that differs and is
/// not empty), and of each of those, the JSON-escaped text (what `serde_json` writes between
/// the quotes) and that text with every non-ASCII char as `\uXXXX`, in lower and in upper hex,
/// a char outside the BMP as its surrogate pair (Python's `ensure_ascii`). Without repeats.
pub(super) fn forms(value: &str) -> Vec<Zeroizing<String>> {
    let mut bases = vec![value];
    let trimmed = value.trim();
    if trimmed != value && !trimmed.is_empty() {
        bases.push(trimmed);
    }
    let mut out: Vec<Zeroizing<String>> = Vec::new();
    let mut push = |form: Zeroizing<String>| {
        if !form.is_empty() && !out.contains(&form) {
            out.push(form);
        }
    };
    for base in bases {
        push(Zeroizing::new(base.to_owned()));
        let quoted = Zeroizing::new(serde_json::to_string(base).unwrap_or_default());
        let escaped = Zeroizing::new(
            quoted
                .get(1..quoted.len().saturating_sub(1))
                .unwrap_or_default()
                .to_owned(),
        );
        if !escaped.is_ascii() {
            push(ascii_escaped(&escaped, false));
            push(ascii_escaped(&escaped, true));
        }
        push(escaped);
    }
    out
}

/// `text` with every non-ASCII char as `\uXXXX` UTF-16 units, in upper or lower hex.
fn ascii_escaped(text: &str, upper: bool) -> Zeroizing<String> {
    let digits: &[u8; 16] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut out = Zeroizing::new(String::with_capacity(text.len() * 2));
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
            continue;
        }
        let mut units = [0u16; 2];
        for unit in c.encode_utf16(&mut units) {
            out.push_str("\\u");
            for shift in [12, 8, 4, 0] {
                let digit = digits
                    .get(usize::from((*unit >> shift) & 0xF))
                    .copied()
                    .unwrap_or(b'0');
                out.push(char::from(digit));
            }
        }
    }
    out
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

/// The forms to replace, longest first, and the numbers.
struct Needles<'a> {
    forms: &'a [&'a str],
    numbers: &'a [&'a str],
}

impl<'a> Needles<'a> {
    const fn new(forms: &'a [&'a str], numbers: &'a [&'a str]) -> Self {
        Self { forms, numbers }
    }

    /// `text` with every form replaced, or `None` when none occurs.
    fn replace(&self, text: &str) -> Option<String> {
        replace_all(text, self.forms)
    }

    fn text(&self, text: &mut String) {
        if let Some(new) = self.replace(text) {
            *text = new;
        }
    }

    /// A JSON text, parsed and scrubbed in its strings, numbers and keys, written back compact.
    /// Text that does not parse is scrubbed as text.
    fn json_text(&self, text: &mut String) {
        if let Ok(mut parsed) = serde_json::from_str::<Value>(text) {
            if self.walk(&mut parsed)
                && let Ok(new) = serde_json::to_string(&parsed)
            {
                *text = new;
            }
            return;
        }
        self.text(text);
    }

    /// Replaces in every string, number and key of `value`; whether anything changed. Keys
    /// that become equal stay apart: the second is `[withheld] 2`, and so on.
    fn walk(&self, value: &mut Value) -> bool {
        match value {
            Value::String(string) => match self.replace(string) {
                Some(new) => {
                    *string = new;
                    true
                }
                None => false,
            },
            Value::Number(number) => {
                let text = Zeroizing::new(number.to_string());
                if self.numbers.contains(&text.as_str()) {
                    *value = Value::String(WITHHELD.to_owned());
                    true
                } else {
                    false
                }
            }
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
                    let key = free_key(map, key);
                    map.insert(key, item);
                }
                changed
            }
            Value::Null | Value::Bool(_) => false,
        }
    }
}

/// `key`, or `key 2`, `key 3`... the first that `map` does not hold yet.
fn free_key(map: &serde_json::Map<String, Value>, key: String) -> String {
    if !map.contains_key(&key) {
        return key;
    }
    // At most `map.len()` suffixes are taken, so one of these is free.
    (2..=map.len().saturating_add(2))
        .map(|n| format!("{key} {n}"))
        .find(|candidate| !map.contains_key(candidate))
        .unwrap_or(key)
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
