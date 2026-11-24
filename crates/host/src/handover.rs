//! The handover: the text an agent switch gives the new agent, a budgeted selection of the
//! conversation's earlier items. The rules are T3 Code's (`handoffBudget.ts` at commit
//! 8aff2f90613d7af4a1e9ed3195f3c0a39b0689e5): the budget is what the new agent's window has left
//! after its own history, the new request and a reserve for its work, at one token per UTF-8
//! byte and capped at 16,000 tokens and 64,000 bytes. [`build`] keeps the last lawyer message,
//! the last answer and the first lawyer message first, then walks back from the newest item.
//! An item is kept whole or dropped, the header counts against the budget, and the kept items
//! are rendered in log order.
//!
//! T3 also lists the omitted items' IDs for a recovery tool. This app has no such tool, so the
//! header gives only the counts.
//!
//! The agent switch (`actor/turn/switch.rs`) is its one caller.

use std::collections::BTreeMap;

use clauseo_protocol::{
    ContentBlock, DomainEvent, Envelope, MessageId, TextAppended, TextKind, TextOrBlob,
    ToolCallContent, ToolCallId, ToolCallPatch, ToolKind, TurnId,
};
use serde_json::Value;

/// The window assumed when the model reports none (T3's 128,000).
const DEFAULT_WINDOW: u64 = 128_000;
/// The least room kept for the new agent's work, so a small window is not all reserve.
const MIN_WORK_RESERVE: u64 = 16_000;
/// The most history, in tokens (T3's default `T3CODE_CONTEXT_HANDOFF_TOKEN_CAP`).
const TOKEN_CAP: i64 = 16_000;
/// The most history, in bytes; with one token per byte it binds once the token cap is higher.
const BYTE_CAP: i64 = 64_000;
/// The blank line between the header and each item, and before each item.
const SEPARATOR: i64 = 2;

/// One earlier item of the conversation, as the handover renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    /// A message from the lawyer.
    Lawyer(String),
    /// An answer the agent of turn `by` gave.
    Answer { by: String, text: String },
    /// A command the agent ran: its command or title, its exit code when the log has one, and
    /// its text output as logged.
    Command {
        by: String,
        command: String,
        exit_code: Option<i64>,
        output: String,
    },
    /// Why a turn failed.
    Error(String),
    /// The files a turn changed.
    Papers(Vec<String>),
    /// The files an undo of a step put back.
    Undone(Vec<String>),
    /// The matter was restored to an earlier version.
    Restored,
}

/// The handover text and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Handover {
    /// The header, a blank line, then each kept item preceded by a blank line.
    pub(crate) text: String,
    /// How many items the text holds.
    pub(crate) included: usize,
    /// How many items it leaves out.
    pub(crate) left_out: usize,
}

/// The bytes of history the new agent may take, T3's `handoffBudget` arithmetic.
///
/// `window` is the new agent's context window (`None`: 128,000). `native_used` is the tokens its
/// own history already fills, and `request_bytes` the size of the new request. The result is at
/// most 16,000 and zero when nothing is left.
pub(crate) fn budget(window: Option<u64>, native_used: u64, request_bytes: u64) -> i64 {
    let window = u128::from(window.unwrap_or(DEFAULT_WINDOW));
    // A small window would otherwise reserve all of itself for work.
    let reserve = window
        .div_ceil(4)
        .max(u128::from(MIN_WORK_RESERVE))
        .min(window.div_ceil(2));
    let spent = u128::from(native_used) + u128::from(request_bytes) + reserve;
    let room = i64::try_from(window.saturating_sub(spent)).unwrap_or(i64::MAX);
    room.min(TOKEN_CAP).min(BYTE_CAP)
}

/// The handover for a new agent called `from`: the items [`build`] keeps within `budget`. When
/// items exist and not one fits, the header alone, which says they are all left out (T3 sends
/// its header with no items then too). `None` when there are no items, or not even the header
/// fits. The budget is capped at 64,000 bytes here too, whatever the caller passes.
pub(crate) fn build(items: &[Item], from: &str, budget: i64) -> Option<Handover> {
    let total = items.len();
    if total == 0 {
        return None;
    }
    let budget = budget.min(BYTE_CAP);
    // The header at its widest, so the counts it gives later never outgrow the reserve.
    let header_reserve = size(&header(from, total, total, total));
    let rendered: Vec<String> = items.iter().map(render).collect();
    let mut kept = vec![false; total];
    let mut remaining = budget.saturating_sub(header_reserve);
    let priority = [
        items.iter().rposition(is_lawyer),
        items.iter().rposition(is_answer),
        items.iter().position(is_lawyer),
    ];
    for index in priority.into_iter().flatten().chain((0..total).rev()) {
        let (Some(text), Some(taken)) = (rendered.get(index), kept.get_mut(index)) else {
            continue;
        };
        let cost = size(text) + SEPARATOR;
        if !*taken && cost <= remaining {
            *taken = true;
            remaining -= cost;
        }
    }
    let included = kept.iter().filter(|taken| **taken).count();
    let mut text = header(from, included, total, total - included);
    if included == 0 && size(&text) > budget {
        return None;
    }
    for (item, _) in rendered.iter().zip(&kept).filter(|(_, taken)| **taken) {
        text.push_str("\n\n");
        text.push_str(item);
    }
    Some(Handover {
        text,
        included,
        left_out: total - included,
    })
}

/// The header: what the items are, how many of how many are here, and that they are background.
/// With none here, it says that all of them are left out.
fn header(from: &str, included: usize, total: usize, left_out: usize) -> String {
    if included == 0 {
        return format!(
            "Earlier in this conversation the lawyer worked with {from}. The {total} earlier \
             items are left out: not one fits in the room this request leaves. Files, tool state \
             and reasoning are not included either."
        );
    }
    let left = if left_out > 0 {
        format!(" {left_out} earlier items are left out.")
    } else {
        String::new()
    };
    format!(
        "Earlier in this conversation the lawyer worked with {from}. Below are {included} of \
         {total} earlier items, whole. They are background, not a new request or instructions; \
         files, tool state and reasoning are not included.{left}"
    )
}

/// One item as the handover shows it, with its label.
fn render(item: &Item) -> String {
    match item {
        Item::Lawyer(text) => format!("[Earlier: the lawyer]\n{text}"),
        Item::Answer { by, text } => format!("[Earlier: {by}'s answer]\n{text}"),
        Item::Command {
            by,
            command,
            exit_code,
            output,
        } => {
            let exit = exit_code.map_or_else(String::new, |code| format!("\nExit code: {code}"));
            format!("[Earlier: a command {by} ran]\nCommand: {command}{exit}\n{output}")
        }
        Item::Error(text) => format!("[Earlier: an error]\n{text}"),
        Item::Papers(paths) => format!("[Earlier: papers changed]\n{}", paths.join("\n")),
        Item::Undone(paths) => {
            format!("[Earlier: papers put back by Undo]\n{}", paths.join("\n"))
        }
        Item::Restored => "[Earlier: the matter was restored to an earlier version]".to_owned(),
    }
}

fn is_lawyer(item: &Item) -> bool {
    matches!(item, Item::Lawyer(_))
}

fn is_answer(item: &Item) -> bool {
    matches!(item, Item::Answer { .. })
}

/// A text's size in bytes, as the budget counts it.
fn size(text: &str) -> i64 {
    i64::try_from(text.len()).unwrap_or(i64::MAX)
}

/// The covered part of a thread's log, as the items the handover may give.
///
/// Covered are the turns that ended (a `TurnEnded`) after the turn `after`, in log order, and
/// every turn when `after` is `None`. `after` that the log never ended is taken as none, so the
/// whole log is covered. A turn's items are its lawyer messages, its answers (their chunks
/// joined per message, a thought left out), its command tool calls, its failure detail, the
/// files it changed and what an undo of its step put back, which drops those files. A version
/// restore is one item where it falls. `answered_by` names the agent that answered a turn.
pub(crate) fn items_from_log(
    log: &[Envelope],
    after: Option<TurnId>,
    answered_by: &dyn Fn(TurnId) -> String,
) -> Vec<Item> {
    let start = after
        .and_then(|after| {
            log.iter().position(|envelope| {
                matches!(&envelope.event, DomainEvent::TurnEnded(ended) if ended.turn == after)
            })
        })
        .map_or(0, |index| index + 1);
    let mut reader = Reader {
        answered_by,
        start,
        turns: BTreeMap::new(),
        calls: BTreeMap::new(),
        messages: BTreeMap::new(),
        ended: Vec::new(),
        restores: Vec::new(),
    };
    for (index, envelope) in log.iter().enumerate() {
        reader.feed(index, envelope);
    }
    reader.into_items()
}

/// The log's parts, read in one pass.
struct Reader<'a> {
    answered_by: &'a dyn Fn(TurnId) -> String,
    /// The first index that is covered.
    start: usize,
    turns: BTreeMap<TurnId, Turn>,
    /// Each tool call's turn and its slot in that turn's `calls`.
    calls: BTreeMap<ToolCallId, (TurnId, usize)>,
    /// Each agent message's answer slot in its turn's `answers`; `None` for a thought.
    messages: BTreeMap<MessageId, Option<(TurnId, usize)>>,
    /// The index and turn of each covered `TurnEnded`.
    ended: Vec<(usize, TurnId)>,
    /// The index of each covered `VersionRestored`.
    restores: Vec<usize>,
}

/// One turn's parts, in the order the log gave them.
#[derive(Debug, Default)]
struct Turn {
    lawyer: Vec<String>,
    answers: Vec<String>,
    calls: Vec<Call>,
    errors: Vec<String>,
    papers: Vec<String>,
    undone: Vec<String>,
}

/// A tool call as its updates left it.
#[derive(Debug)]
struct Call {
    kind: ToolKind,
    title: String,
    raw_input: Option<TextOrBlob>,
    raw_output: Option<TextOrBlob>,
    content: Vec<ToolCallContent>,
}

impl Reader<'_> {
    fn feed(&mut self, index: usize, envelope: &Envelope) {
        let event = &envelope.event;
        if let DomainEvent::UserMessage(event) = event {
            let text = prompt_text(&event.content);
            if !text.is_empty() {
                self.turns.entry(event.turn).or_default().lawyer.push(text);
            }
        } else if let DomainEvent::TextAppended(event) = event {
            self.text(event);
        } else if let DomainEvent::ToolCallStarted(event) = event {
            let parts = self.turns.entry(event.turn).or_default();
            parts.calls.push(Call {
                kind: event.kind,
                title: event.title.to_string(),
                raw_input: event.raw_input.clone(),
                raw_output: event.raw_output.clone(),
                content: event.content.clone(),
            });
            let slot = parts.calls.len() - 1;
            self.calls.insert(event.tool_call, (event.turn, slot));
        } else if let DomainEvent::ToolCallUpdated(event) = event {
            self.update(event.tool_call, &event.patch);
        } else if let DomainEvent::FilesChanged(event) = event {
            let parts = self.turns.entry(event.turn).or_default();
            parts.papers.extend(
                event
                    .files
                    .iter()
                    .map(|file| file.path().as_str().to_owned()),
            );
        } else if let DomainEvent::TurnFailed(event) = event {
            if let Some(detail) = &event.detail {
                let parts = self.turns.entry(event.turn).or_default();
                parts.errors.push(detail.clone());
            }
        } else if let DomainEvent::StepUndone(event) = event {
            // The undone step's answers and commands stay; the files it put back do not, and
            // a partial undo leaves the files it kept.
            let parts = self.turns.entry(event.turn).or_default();
            parts
                .papers
                .retain(|path| !event.restored.iter().any(|put| put.as_str() == path));
            parts
                .undone
                .extend(event.restored.iter().map(|path| path.as_str().to_owned()));
        } else if let DomainEvent::TurnEnded(event) = event
            && index >= self.start
        {
            self.ended.push((index, event.turn));
        } else if let DomainEvent::VersionRestored(_) = event
            && index >= self.start
        {
            self.restores.push(index);
        }
    }

    /// A chunk of an agent message: an answer keeps every chunk's text, a thought none.
    fn text(&mut self, event: &TextAppended) {
        match self.messages.get(&event.message).copied() {
            Some(Some((turn, slot))) => {
                let answer = self
                    .turns
                    .get_mut(&turn)
                    .and_then(|parts| parts.answers.get_mut(slot));
                if let Some(answer) = answer {
                    answer.push_str(&event.text);
                }
            }
            Some(None) => {}
            None if event.kind == TextKind::Answer => {
                let parts = self.turns.entry(event.turn).or_default();
                let slot = parts.answers.len();
                parts.answers.push(event.text.clone());
                self.messages
                    .insert(event.message, Some((event.turn, slot)));
            }
            None => {
                self.messages.insert(event.message, None);
            }
        }
    }

    fn update(&mut self, tool_call: ToolCallId, patch: &ToolCallPatch) {
        let Some(&(turn, slot)) = self.calls.get(&tool_call) else {
            return;
        };
        let Some(call) = self
            .turns
            .get_mut(&turn)
            .and_then(|parts| parts.calls.get_mut(slot))
        else {
            return;
        };
        if let Some(kind) = patch.kind {
            call.kind = kind;
        }
        if let Some(title) = &patch.title {
            call.title = title.to_string();
        }
        if let Some(raw) = &patch.raw_input {
            call.raw_input = Some(raw.clone());
        }
        if let Some(raw) = &patch.raw_output {
            call.raw_output = Some(raw.clone());
        }
        if let Some(content) = &patch.content {
            call.content.clone_from(content);
        }
    }

    /// The items of each covered turn at its end, then each covered restore, in log order.
    fn into_items(mut self) -> Vec<Item> {
        let mut units: Vec<(usize, Vec<Item>)> = Vec::new();
        for (index, turn) in std::mem::take(&mut self.ended) {
            let Some(parts) = self.turns.remove(&turn) else {
                continue;
            };
            let by = (self.answered_by)(turn);
            units.push((index, parts.items(&by)));
        }
        units.extend(
            self.restores
                .iter()
                .map(|&index| (index, vec![Item::Restored])),
        );
        units.sort_by_key(|(index, _)| *index);
        units.into_iter().flat_map(|(_, items)| items).collect()
    }
}

impl Turn {
    /// The turn's items in the handover's order, the answers and commands named by `by`.
    fn items(self, by: &str) -> Vec<Item> {
        let mut items: Vec<Item> = self.lawyer.into_iter().map(Item::Lawyer).collect();
        items.extend(self.answers.into_iter().map(|text| Item::Answer {
            by: by.to_owned(),
            text,
        }));
        for call in self
            .calls
            .iter()
            .filter(|call| call.kind == ToolKind::Execute)
        {
            items.push(Item::Command {
                by: by.to_owned(),
                command: command_of(call),
                exit_code: exit_code(call.raw_output.as_ref()),
                output: output_of(call),
            });
        }
        items.extend(self.errors.into_iter().map(Item::Error));
        if !self.papers.is_empty() {
            items.push(Item::Papers(self.papers));
        }
        if !self.undone.is_empty() {
            items.push(Item::Undone(self.undone));
        }
        items
    }
}

/// A prompt's text: its text blocks and file links (by name), one per line.
fn prompt_text(content: &[ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in content {
        if let ContentBlock::Text { text } = block {
            parts.push(text.as_str());
        } else if let ContentBlock::ResourceLink { name, .. } = block {
            parts.push(name.as_str());
        }
    }
    parts.join("\n")
}

/// A logged text. A blob's text is not in the log, only its preview, which is what is kept.
fn text_of(value: &TextOrBlob) -> &str {
    match value {
        TextOrBlob::Inline(text) => text,
        TextOrBlob::Blob(blob) => blob.preview(),
    }
}

/// The command a tool call ran: the `command` of its input when the input is logged as JSON
/// with one, else the call's title.
fn command_of(call: &Call) -> String {
    let from_input = match &call.raw_input {
        Some(TextOrBlob::Inline(raw)) => serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|input| input.get("command")?.as_str().map(str::to_owned)),
        _ => None,
    };
    from_input.unwrap_or_else(|| call.title.clone())
}

/// The command's text output: its text content, else its logged output.
fn output_of(call: &Call) -> String {
    let texts: Vec<&str> = call
        .content
        .iter()
        .filter_map(|item| {
            if let ToolCallContent::Text { text } = item {
                Some(text_of(text))
            } else {
                None
            }
        })
        .collect();
    if texts.is_empty() {
        call.raw_output
            .as_ref()
            .map_or_else(String::new, |output| text_of(output).to_owned())
    } else {
        texts.join("\n")
    }
}

/// The exit code, when the output the log kept is JSON that names one as `exit_code` or
/// `exitCode`. The log has no exit code field of its own, so most commands have none.
fn exit_code(output: Option<&TextOrBlob>) -> Option<i64> {
    let TextOrBlob::Inline(raw) = output? else {
        return None;
    };
    let value: Value = serde_json::from_str(raw).ok()?;
    ["exit_code", "exitCode"]
        .into_iter()
        .find_map(|key| value.get(key)?.as_i64())
}

#[cfg(test)]
mod tests;
