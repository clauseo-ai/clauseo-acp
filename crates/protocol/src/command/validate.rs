//! Shape validation for commands: lengths, counts and formats, shared by every surface that
//! takes commands (`desktop`, `transport_ws`, `cli`). State checks are the host's.
//!
//! Limits (doc 16 §3.2, plus bounds chosen here for ids and lists):
//! - prompt text ([`MAX_PROMPT_TEXT_BYTES`]): the UTF-8 bytes of every `Text` block, summed;
//! - file references per prompt ([`MAX_PROMPT_FILES`]);
//! - thread titles and matter names ([`MAX_TITLE_CHARS`]), counted in `char`s;
//! - elicitation values ([`MAX_ELICITATION_VALUES_BYTES`]): their compact JSON length;
//! - agent-chosen ids (`option_id`, `mode_id`, a select value) ([`MAX_ID_TEXT_BYTES`]);
//! - files named in an undo or a restore ([`MAX_FILE_LIST`]).

use std::io;

use super::{CommandEnvelope, ConfigValue, HostCommand};
use crate::{
    Delivery, ElicitationResponse, Error, FileDecision, MatterPath, PromptBlock, RawPayload,
    RestoreScope,
};

/// Most prompt text, in UTF-8 bytes summed over the prompt's `Text` blocks (256 KiB). Raw bytes,
/// not JSON-escaped ones: the lawyer typed them, and the limit guards the agent's input.
pub const MAX_PROMPT_TEXT_BYTES: usize = 256 * 1024;

/// Most `File` blocks in one prompt.
pub const MAX_PROMPT_FILES: usize = 50;

/// Most characters (`char`s, Unicode scalar values) in a thread title or a matter name.
pub const MAX_TITLE_CHARS: usize = 200;

/// Most elicitation values, as the length of their compact JSON text (64 KiB), counted without
/// building the text.
pub const MAX_ELICITATION_VALUES_BYTES: usize = 64 * 1024;

/// Most UTF-8 bytes in an agent-chosen id a command echoes: `option_id`, `mode_id` and a select
/// value. ACP's ids are short words; 256 leaves room without letting a client send megabytes.
pub const MAX_ID_TEXT_BYTES: usize = 256;

/// Most files named in an `UndoStep` or a `RestoreVersion` scope. Far above what one step or one
/// restore touches in a legal matter, so it only stops a client sending an unbounded list.
pub const MAX_FILE_LIST: usize = 10_000;

impl CommandEnvelope {
    /// Checks the command's shape; see [`HostCommand::validate`].
    ///
    /// # Errors
    ///
    /// As [`HostCommand::validate`].
    pub fn validate(&self) -> Result<(), Error> {
        self.command.validate()
    }
}

impl HostCommand {
    /// How the prompt is delivered: `delivery`, or [`Delivery::Queue`] when it is missing.
    /// `None` for every command but `SendPrompt`.
    pub fn delivery(&self) -> Option<Delivery> {
        match self {
            Self::SendPrompt { delivery, .. } => Some(delivery.unwrap_or(Delivery::Queue)),
            Self::CreateMatter { .. }
            | Self::CreateThread { .. }
            | Self::EditQueuedPrompt { .. }
            | Self::RemoveQueuedPrompt { .. }
            | Self::SendQueuedNow { .. }
            | Self::ResumeQueue { .. }
            | Self::Cancel { .. }
            | Self::RestartAgent { .. }
            | Self::AnswerPermission { .. }
            | Self::AnswerElicitation { .. }
            | Self::SetConfigOption { .. }
            | Self::SetMode { .. }
            | Self::RenameThread { .. }
            | Self::ArchiveThread { .. }
            | Self::UndoStep { .. }
            | Self::RestoreVersion { .. } => None,
        }
    }
    /// Checks lengths, counts and formats, never state. Ids and paths were already checked when
    /// they were decoded or built.
    ///
    /// - `SendPrompt` and `EditQueuedPrompt`: at least one block, at most
    ///   [`MAX_PROMPT_TEXT_BYTES`] of text and [`MAX_PROMPT_FILES`] file references; a
    ///   `Delivery::Unknown` (a newer client's mode) is refused, never run as another mode.
    /// - `CreateMatter.name`: 1 to [`MAX_TITLE_CHARS`] characters, not only whitespace, no control
    ///   character, no `/` or `\`, not `.` or `..` (it may become a folder name).
    /// - `RenameThread.title`: 1 to [`MAX_TITLE_CHARS`] characters, not only whitespace, no
    ///   control character.
    /// - `option_id`, `mode_id` and a select value: 1 to [`MAX_ID_TEXT_BYTES`] bytes.
    /// - `AnswerElicitation` with `Accept`: the values are a JSON object of at most
    ///   [`MAX_ELICITATION_VALUES_BYTES`] as compact JSON.
    /// - `UndoStep.files` and `RestoreScope::Files`: at most [`MAX_FILE_LIST`] paths, no path
    ///   twice; a `Files` scope names at least one.
    ///
    /// # Errors
    ///
    /// [`Error::TooLarge`] for a size over its limit, [`Error::Format`] for every other rule.
    /// Both map to `command.invalid`.
    pub fn validate(&self) -> Result<(), Error> {
        match self {
            Self::CreateMatter { name, .. } => {
                check_title("CreateMatter", name)?;
                check_folder_name(name)
            }
            Self::SendPrompt {
                content, delivery, ..
            } => {
                check_prompt("SendPrompt", content)?;
                match delivery {
                    Some(Delivery::Unknown) => Err(Error::Format {
                        kind: "SendPrompt",
                        rule: "delivery is from a newer Clauseo; it is refused, not guessed",
                    }),
                    Some(Delivery::Queue | Delivery::SendNow | Delivery::Steer) | None => Ok(()),
                }
            }
            Self::EditQueuedPrompt { content, .. } => check_prompt("EditQueuedPrompt", content),
            Self::AnswerPermission { option_id, .. } => {
                check_id("AnswerPermission", "option_id", option_id)
            }
            Self::AnswerElicitation { response, .. } => check_response(response),
            Self::SetConfigOption {
                option_id, value, ..
            } => {
                check_id("SetConfigOption", "option_id", option_id)?;
                match value {
                    ConfigValue::Select(choice) => check_id("SetConfigOption", "value", choice),
                    ConfigValue::Boolean(_) => Ok(()),
                }
            }
            Self::SetMode { mode_id, .. } => check_id("SetMode", "mode_id", mode_id),
            Self::RenameThread { title, .. } => check_title("RenameThread", title),
            Self::UndoStep { files, .. } => check_paths(
                "UndoStep",
                files.iter().map(|file: &FileDecision| &file.path),
            ),
            Self::RestoreVersion { scope, .. } => match scope {
                RestoreScope::Matter => Ok(()),
                RestoreScope::Files(paths) if paths.is_empty() => Err(Error::Format {
                    kind: "RestoreVersion",
                    rule: "a files scope names at least one file",
                }),
                RestoreScope::Files(paths) => check_paths("RestoreVersion", paths.iter()),
            },
            Self::CreateThread { .. }
            | Self::RemoveQueuedPrompt { .. }
            | Self::SendQueuedNow { .. }
            | Self::ResumeQueue { .. }
            | Self::Cancel { .. }
            | Self::RestartAgent { .. }
            | Self::ArchiveThread { .. } => Ok(()),
        }
    }
}

fn too_large(kind: &'static str, what: &'static str, size: usize, max: usize) -> Error {
    Error::TooLarge {
        kind,
        what,
        size,
        max,
    }
}

fn check_prompt(kind: &'static str, content: &[PromptBlock]) -> Result<(), Error> {
    if content.is_empty() {
        return Err(Error::Format {
            kind,
            rule: "a prompt has at least one block",
        });
    }
    let mut text_bytes = 0usize;
    let mut files = 0usize;
    for block in content {
        match block {
            PromptBlock::Text { text } => text_bytes = text_bytes.saturating_add(text.len()),
            PromptBlock::File { .. } => files += 1,
        }
    }
    if text_bytes > MAX_PROMPT_TEXT_BYTES {
        return Err(too_large(
            kind,
            "prompt text in UTF-8 bytes",
            text_bytes,
            MAX_PROMPT_TEXT_BYTES,
        ));
    }
    if files > MAX_PROMPT_FILES {
        return Err(too_large(kind, "file references", files, MAX_PROMPT_FILES));
    }
    Ok(())
}

fn check_title(kind: &'static str, title: &str) -> Result<(), Error> {
    if title.trim().is_empty() {
        return Err(Error::Format {
            kind,
            rule: "a title or name is not empty or only whitespace",
        });
    }
    let chars = title.chars().count();
    if chars > MAX_TITLE_CHARS {
        return Err(too_large(
            kind,
            "title or name in characters",
            chars,
            MAX_TITLE_CHARS,
        ));
    }
    if title.chars().any(char::is_control) {
        return Err(Error::Format {
            kind,
            rule: "a title or name holds no control character",
        });
    }
    Ok(())
}

/// A matter name may become a folder name (for `NewFolder`), so it is checked as one for every
/// source: the same name rules whichever way the matter was made.
fn check_folder_name(name: &str) -> Result<(), Error> {
    if name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(Error::Format {
            kind: "CreateMatter",
            rule: "a matter name holds no `/` or `\\` and is not `.` or `..`",
        });
    }
    Ok(())
}

fn check_id(kind: &'static str, what: &'static str, id: &str) -> Result<(), Error> {
    if id.is_empty() {
        return Err(Error::Format {
            kind,
            rule: "an option, mode or value id is not empty",
        });
    }
    if id.len() > MAX_ID_TEXT_BYTES {
        return Err(too_large(kind, what, id.len(), MAX_ID_TEXT_BYTES));
    }
    Ok(())
}

fn check_response(response: &ElicitationResponse) -> Result<(), Error> {
    let values = match response {
        ElicitationResponse::Accept { values } => values,
        ElicitationResponse::Decline | ElicitationResponse::Cancel => return Ok(()),
    };
    if !matches!(values, RawPayload::Map(_)) {
        return Err(Error::Format {
            kind: "AnswerElicitation",
            rule: "accepted values are a JSON object",
        });
    }
    let Some(size) = json_len(values) else {
        return Err(Error::Format {
            kind: "AnswerElicitation",
            rule: "accepted values have a JSON form (string keys only)",
        });
    };
    if size > MAX_ELICITATION_VALUES_BYTES {
        return Err(too_large(
            "AnswerElicitation",
            "values as compact JSON in bytes",
            size,
            MAX_ELICITATION_VALUES_BYTES,
        ));
    }
    Ok(())
}

/// The length of `value`'s compact JSON text, written into a counter, never into a buffer;
/// `None` when it has no JSON form (a non-string map key, possible only from `MessagePack`).
fn json_len(value: &RawPayload) -> Option<usize> {
    let mut counter = ByteCounter(0);
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.0)
}

/// An `io::Write` that only counts.
struct ByteCounter(usize);

impl io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(buf.len());
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn check_paths<'a>(
    kind: &'static str,
    paths: impl ExactSizeIterator<Item = &'a MatterPath>,
) -> Result<(), Error> {
    if paths.len() > MAX_FILE_LIST {
        return Err(too_large(kind, "files named", paths.len(), MAX_FILE_LIST));
    }
    let mut sorted: Vec<&str> = paths.map(MatterPath::as_str).collect();
    sorted.sort_unstable();
    if sorted
        .windows(2)
        .any(|pair| matches!(pair, [a, b] if a == b))
    {
        return Err(Error::Format {
            kind,
            rule: "no file is named twice",
        });
    }
    Ok(())
}
