//! How Claude (claude-agent-acp 0.110.1 on Claude Code 2.1.330) says a helper starts and ends,
//! from a recorded run (crates/acp's fixture `claude-agent-acp-0.110.1-helper.jsonl`) and the
//! adapter's code.
//!
//! - Launch: the lead's tool call named `Agent` (`Task` before Claude Code renamed it). Its input
//!   (`subagent_type`, `description`, `prompt`, optional `model`, `run_in_background`) usually
//!   comes in a later `tool_call_update`, so the helper starts when the input comes, or at the
//!   step's end if it never does. The helper's id is that call's `toolCallId`.
//! - Background, Claude Code's default: the adapter says so (`HelperInBackground`, from
//!   `toolResponse.status` `async_launched`) and the `Agent` call completes at once with a
//!   launch note; that end is not the helper's.
//! - The helper's end: its own stamped call `SubagentHandback`, whose `rawInput.message` is its
//!   report. Its unstamped update says `success` true ("Report delivered to your caller.") or
//!   false (nothing sent; the helper works on). Neither is a step.
//! - A foreground helper hands back too, and its `Agent` call's text then only points at the
//!   handback. One that does not (cut at its turn limit) ends with its `Agent` call: completed is
//!   done with the call's text (the limit's note), failed is failed, cancelled is stopped.

use clauseo_acp::ToolCall;
use clauseo_protocol::{
    CompactString, ContentBlock, HelperId, HelperOutcome, TextOrBlob, ThreadId, ToolCallContent,
    ToolCallId, ToolCallPatch, ToolCallStatus, TurnId,
};
use serde_json::Value;

use super::super::super::Actor;
use super::{Start, end, existing, helpers_mut, start};
use crate::error::Error;

/// The tool names of a lead's call that launches a helper.
const LAUNCH_TOOLS: [&str; 2] = ["Agent", "Task"];

/// The tool name of a helper's call that hands its report back.
const HANDBACK: &str = "SubagentHandback";

/// A launch step whose helper has not started yet.
#[derive(Debug, Clone)]
pub(in crate::actor::turn) struct Launch {
    /// Its helper's id, taken as the step is logged, so a stamped step that comes before the
    /// input still finds its helper.
    id: Option<HelperId>,
    /// The step, as logged.
    call: ToolCallId,
    turn: TurnId,
    /// The step's title: the helper's name when its input never says one.
    title: CompactString,
    /// The helper that made the call; `None` for the lead.
    parent: Option<HelperId>,
    /// The adapter said the helper works on in the background.
    pub(super) background: bool,
}

/// A handback call waiting for its update.
#[derive(Debug, Clone)]
pub(in crate::actor::turn) struct Handback {
    /// The agent's id for the helper handing back.
    helper: CompactString,
    /// Its report, when its input carried one.
    message: Option<String>,
}

impl Handback {
    /// The agent's id for the helper handing back.
    pub(super) fn helper(&self) -> &str {
        &self.helper
    }
}

/// What a step update means for a helper, read before the update is logged.
#[derive(Debug, Clone)]
pub(in crate::actor::turn) enum Before {
    /// Not a helper's launch step nor a handback: logged as it is.
    Plain,
    /// A handback's update: not logged.
    Swallow,
    /// A launch step's update: logged, then [`after_launch_update`].
    Launch(LaunchPatch),
}

/// What [`after_launch_update`] needs of a launch step's update.
#[derive(Debug, Clone)]
pub(in crate::actor::turn) struct LaunchPatch {
    raw_input: Option<TextOrBlob>,
    status: Option<ToolCallStatus>,
    text: Option<String>,
}

/// Whether a call named `tool_name` launches a helper.
pub(in crate::actor::turn) fn is_launch(tool_name: Option<&str>) -> bool {
    tool_name.is_some_and(|name| LAUNCH_TOOLS.contains(&name))
}

/// Whether a call named `tool_name` is a helper's handback.
pub(super) fn is_handback(tool_name: Option<&str>) -> bool {
    tool_name == Some(HANDBACK)
}

impl Launch {
    /// The launch step logged as `call` in `turn` with `title`, made by the helper `parent`
    /// (`None`: the lead).
    pub(in crate::actor::turn) const fn new(
        call: ToolCallId,
        turn: TurnId,
        title: CompactString,
        parent: Option<HelperId>,
    ) -> Self {
        Self {
            id: None,
            call,
            turn,
            title,
            parent,
            background: false,
        }
    }

    /// Its helper's id.
    pub(super) const fn id(&self) -> Option<HelperId> {
        self.id
    }
}

/// The launch step ACP knows as `acp_id`, just logged: starts its helper now when its input
/// (`raw_input`) is there, else when it comes.
pub(in crate::actor::turn) fn launched(
    actor: &mut Actor,
    thread: ThreadId,
    acp_id: &CompactString,
    mut launch: Launch,
    raw_input: Option<&TextOrBlob>,
) -> Result<(), Error> {
    launch.id = Some(actor.ids.mint(actor.clock.now().wall)?);
    let turn = launch.turn;
    let Some(input) = input_of(raw_input) else {
        if let Some(helpers) = helpers_mut(actor, thread) {
            helpers.launches.insert(acp_id.clone(), launch);
        }
        return Ok(());
    };
    start(
        actor,
        thread,
        turn,
        start_of(acp_id.clone(), launch, Some(input)),
    )
}

/// A helper's handback call (`SubagentHandback`): kept until its update says whether the
/// report landed; never logged as a step. Its report is its input's `message`, from the call or,
/// when the call has none yet, from a later update.
pub(super) fn handback_started(
    actor: &mut Actor,
    thread: ThreadId,
    helper: &CompactString,
    call: &ToolCall,
) {
    let message = report_of(call.raw_input.as_ref());
    if let Some(helpers) = existing(actor, thread) {
        helpers.handbacks.insert(
            call.acp_id.clone(),
            Handback {
                helper: helper.clone(),
                message,
            },
        );
    } else {
        tracing::debug!(%thread, %helper, "a handback of a helper not started");
    }
}

/// What the update of the step ACP knows as `acp_id` means for a helper. A handback's final
/// update ends its helper `Done` with its report, unless it says `success: false`.
pub(in crate::actor::turn) fn before_update(
    actor: &mut Actor,
    thread: ThreadId,
    acp_id: &str,
    patch: &ToolCallPatch,
) -> Result<Before, Error> {
    let Some(helpers) = existing(actor, thread) else {
        return Ok(Before::Plain);
    };
    if let Some(handback) = helpers.handbacks.get_mut(acp_id) {
        // The report may come in an update, as an `Agent` call's input does.
        if handback.message.is_none() {
            handback.message = report_of(patch.raw_input.as_ref());
        }
        if !patch.status.is_some_and(is_final) {
            return Ok(Before::Swallow);
        }
        let Some(handback) = helpers.handbacks.remove(acp_id) else {
            return Ok(Before::Swallow);
        };
        if delivered(patch) {
            end(
                actor,
                thread,
                &handback.helper,
                HelperOutcome::Done,
                handback.message,
            )?;
        }
        return Ok(Before::Swallow);
    }
    let launch = helpers.launches.contains_key(acp_id) || helpers.known.contains_key(acp_id);
    if !launch {
        return Ok(Before::Plain);
    }
    let status = patch.status;
    let text = status
        .filter(|status| *status == ToolCallStatus::Completed)
        .and(patch.content.as_deref())
        .and_then(text_of);
    Ok(Before::Launch(LaunchPatch {
        raw_input: patch.raw_input.clone(),
        status,
        text,
    }))
}

/// A launch step's update, logged: starts its helper when the input came (or the step ended
/// without one), and ends a foreground helper with the step.
pub(in crate::actor::turn) fn after_launch_update(
    actor: &mut Actor,
    thread: ThreadId,
    acp_id: &CompactString,
    patch: LaunchPatch,
) -> Result<(), Error> {
    let input = input_of(patch.raw_input.as_ref());
    let ends = patch.status.is_some_and(is_final);
    if input.is_some() || ends {
        let pending = existing(actor, thread).and_then(|helpers| helpers.launches.remove(acp_id));
        if let Some(launch) = pending {
            let turn = launch.turn;
            start(actor, thread, turn, start_of(acp_id.clone(), launch, input))?;
        }
    }
    let Some(status) = patch.status.filter(|status| is_final(*status)) else {
        return Ok(());
    };
    let Some(known) = super::known(actor, thread, acp_id) else {
        return Ok(());
    };
    let outcome = match status {
        ToolCallStatus::Failed => HelperOutcome::Failed,
        ToolCallStatus::Completed | ToolCallStatus::Cancelled if known.background => return Ok(()),
        // A foreground helper that hands back: the call's text only points at the handback,
        // which ends the helper with its report, before or after this.
        ToolCallStatus::Completed if hands_back(actor, thread, acp_id, patch.text.as_deref()) => {
            return Ok(());
        }
        ToolCallStatus::Completed => HelperOutcome::Done,
        ToolCallStatus::Cancelled => HelperOutcome::Stopped,
        ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown => {
            return Ok(());
        }
    };
    end(actor, thread, acp_id, outcome, patch.text)
}

/// The words of a launch call's result that point at the helper's handback instead of holding
/// its report, as Claude Code 2.1.330 writes them ("This agent's report was delivered to you as
/// a message from … (its `SubagentHandback` call). Read it there; it is not repeated here.";
/// recorded for two foreground helpers at once).
const HANDBACK_POINTER: &str = "(its SubagentHandback call)";

/// Whether the helper the agent knows as `helper` ends by its handback rather than by its launch
/// call's `text`: a handback of it is waiting for its update, or the text points at one.
fn hands_back(actor: &mut Actor, thread: ThreadId, helper: &str, text: Option<&str>) -> bool {
    let waiting = existing(actor, thread).is_some_and(|helpers| {
        helpers
            .handbacks
            .values()
            .any(|handback| handback.helper == helper)
    });
    waiting || text.is_some_and(|text| text.contains(HANDBACK_POINTER))
}

/// A helper's start from its launch step and the step's input (`None`: it never came).
fn start_of(acp_id: CompactString, launch: Launch, input: Option<Input>) -> Start {
    let input = input.unwrap_or_default();
    let model = input
        .model
        .or_else(|| defined_model(input.name.as_deref()?));
    let background = launch.background || input.background;
    Start {
        id: launch.id,
        acp_id,
        parent: launch.parent,
        call: Some(launch.call),
        name: input.name.unwrap_or(launch.title),
        label: input.label,
        task: input.task,
        model,
        background,
        codex: false,
    }
}

/// A handback's report: its input's `message`, when it is a string.
fn report_of(raw: Option<&TextOrBlob>) -> Option<String> {
    object_of(raw).and_then(|input| text(&input, "message").map(str::to_owned))
}

/// What a launch step's input says, each field when it is a string.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Input {
    /// `subagent_type`, else `name`.
    name: Option<CompactString>,
    /// `description`.
    label: Option<CompactString>,
    /// `prompt`.
    task: Option<String>,
    /// `model`.
    model: Option<CompactString>,
    /// `run_in_background` is true.
    background: bool,
}

/// The launch input in `raw`, when it is a JSON object naming the helper or its task.
fn input_of(raw: Option<&TextOrBlob>) -> Option<Input> {
    let input = object_of(raw)?;
    let name = text(&input, "subagent_type")
        .or_else(|| text(&input, "name"))
        .map(CompactString::from);
    let task = text(&input, "prompt").map(str::to_owned);
    if name.is_none() && task.is_none() {
        return None;
    }
    Some(Input {
        name,
        label: text(&input, "description").map(CompactString::from),
        task,
        model: text(&input, "model").map(CompactString::from),
        background: input.get("run_in_background") == Some(&Value::Bool(true)),
    })
}

/// A step's raw input as a JSON object, when it is inline text that parses as one (ACP hands
/// every raw input over inline; `build.rs` moves a large one to a blob only as it is logged).
fn object_of(raw: Option<&TextOrBlob>) -> Option<serde_json::Map<String, Value>> {
    let Some(TextOrBlob::Inline(text)) = raw else {
        return None;
    };
    if let Ok(Value::Object(object)) = serde_json::from_str(text) {
        Some(object)
    } else {
        None
    }
}

fn text<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key)?.as_str()
}

/// The model Clauseo's profile sets for the helper `name` (`clauseo_profile::subagents`).
fn defined_model(name: &str) -> Option<CompactString> {
    let agents = clauseo_profile::subagents::subagents();
    agents
        .get(name)?
        .get("model")?
        .as_str()
        .map(CompactString::from)
}

/// The text of a step's content: its text items, joined by blank lines; `None` without one.
fn text_of(content: &[ToolCallContent]) -> Option<String> {
    let mut joined = String::new();
    for item in content {
        let piece = match item {
            ToolCallContent::Text {
                text: TextOrBlob::Inline(text),
            }
            | ToolCallContent::Content {
                content: ContentBlock::Text { text },
            } => text.as_str(),
            ToolCallContent::Text {
                text: TextOrBlob::Blob(_),
            }
            | ToolCallContent::Content {
                content:
                    ContentBlock::Image { .. }
                    | ContentBlock::Audio { .. }
                    | ContentBlock::ResourceLink { .. }
                    | ContentBlock::Resource { .. }
                    | ContentBlock::Unsupported { .. },
            }
            | ToolCallContent::Diff { .. }
            | ToolCallContent::Unknown { .. } => continue,
        };
        if !joined.is_empty() {
            joined.push_str("\n\n");
        }
        joined.push_str(piece);
    }
    (!joined.is_empty()).then_some(joined)
}

/// Whether a handback's final update says the report landed: true unless its text is JSON with
/// `success: false`. Text it cannot read counts as landed, so a report is never lost.
fn delivered(patch: &ToolCallPatch) -> bool {
    let Some(text) = patch.content.as_deref().and_then(text_of) else {
        return true;
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => value.get("success") != Some(&Value::Bool(false)),
        Err(_) => true,
    }
}

const fn is_final(status: ToolCallStatus) -> bool {
    matches!(
        status,
        ToolCallStatus::Completed | ToolCallStatus::Failed | ToolCallStatus::Cancelled
    )
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;
